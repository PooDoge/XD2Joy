#include <stdio.h>
#include <string.h>

#include "tusb.h"

#include "bridge_proto.h"
#include "hid_parse.h"
#include "input.h"
#include "link.h"

/*
 * Each HID interface keeps its own last state. Keyboards are merged
 * (modifiers OR'd, keys unioned), mice share one motion accumulator, and the
 * result goes out as soon as it changes. TinyUSB calls back from tuh_task(),
 * which runs in the same main loop as input_task(), so nothing here races.
 */

#define MAX_ITF CFG_TUH_HID
#define REFRESH_MS 100
/*
 * Motion-only frames go out at most every 2 ms. The nRF only reports to the
 * console every 15 ms or more, so this costs no latency, and it halves the
 * UART interrupt load a 1000 Hz mouse puts on the right board. Button
 * changes still go out at once.
 */
#define MOUSE_MIN_MS 2

typedef struct {
	bool used;
	uint8_t daddr;
	uint8_t idx;
	uint8_t kinds;
	uint8_t proto; /* bInterfaceProtocol: HID_ITF_PROTOCOL_* */
	uint8_t boot; /* HID_ITF_PROTOCOL_* while in boot protocol, else 0 */
	hp_desc_t desc;
	hp_kbd_t kbd;
	uint8_t buttons;
} itf_t;

static itf_t itfs[MAX_ITF];

static int32_t acc_dx, acc_dy, acc_wheel;
static bool mouse_dirty;
static uint8_t sent_buttons;
static uint8_t sent_mods;
static uint8_t sent_keys[6];
static bool kbd_dirty;

static uint32_t last_kbd_tx, last_mouse_tx, last_status_tx;

static itf_t *itf_find(uint8_t daddr, uint8_t idx)
{
	for (int i = 0; i < MAX_ITF; i++) {
		if (itfs[i].used && itfs[i].daddr == daddr && itfs[i].idx == idx) {
			return &itfs[i];
		}
	}
	return NULL;
}

static itf_t *itf_alloc(uint8_t daddr, uint8_t idx)
{
	itf_t *it = itf_find(daddr, idx);

	if (it) {
		return it;
	}
	for (int i = 0; i < MAX_ITF; i++) {
		if (!itfs[i].used) {
			memset(&itfs[i], 0, sizeof(itfs[i]));
			itfs[i].used = true;
			itfs[i].daddr = daddr;
			itfs[i].idx = idx;
			return &itfs[i];
		}
	}
	return NULL;
}

/*
 * What a device is, judged by its boot interfaces: a gaming mouse often has
 * an extra keyboard collection for macro keys, and a keyboard may carry a
 * mouse collection. Those shouldn't make a board think a wired keyboard (or
 * mouse) is attached and stop listening to its Bluetooth one.
 */
static uint8_t device_role(uint8_t daddr)
{
	uint8_t boot = 0;

	for (int i = 0; i < MAX_ITF; i++) {
		if (!itfs[i].used || itfs[i].daddr != daddr) {
			continue;
		}
		if (itfs[i].proto == HID_ITF_PROTOCOL_KEYBOARD) {
			boot |= HP_KIND_KBD;
		} else if (itfs[i].proto == HID_ITF_PROTOCOL_MOUSE) {
			boot |= HP_KIND_MOUSE;
		}
	}
	/* a combo receiver has both; unknown devices get the benefit of the doubt */
	return boot ? boot : (HP_KIND_KBD | HP_KIND_MOUSE);
}

static uint8_t present_kinds(void)
{
	uint8_t k = 0;

	for (int i = 0; i < MAX_ITF; i++) {
		if (itfs[i].used) {
			k |= itfs[i].kinds & device_role(itfs[i].daddr);
		}
	}
	return k;
}

static void merged_kbd(uint8_t *mods, uint8_t keys[6])
{
	uint8_t n = 0;

	*mods = 0;
	memset(keys, 0, 6);
	for (int i = 0; i < MAX_ITF; i++) {
		const itf_t *it = &itfs[i];

		if (!it->used || !(it->kinds & HP_KIND_KBD)) {
			continue;
		}
		*mods |= it->kbd.mods;
		for (int k = 0; k < 6 && it->kbd.keys[k]; k++) {
			bool dup = false;

			for (uint8_t j = 0; j < n; j++) {
				dup = dup || keys[j] == it->kbd.keys[k];
			}
			if (!dup && n < 6) {
				keys[n++] = it->kbd.keys[k];
			}
		}
	}
}

static uint8_t merged_buttons(void)
{
	uint8_t b = 0;

	for (int i = 0; i < MAX_ITF; i++) {
		if (itfs[i].used && (itfs[i].kinds & HP_KIND_MOUSE)) {
			b |= itfs[i].buttons;
		}
	}
	return b;
}

static void kbd_changed(void)
{
	uint8_t mods, keys[6];

	merged_kbd(&mods, keys);
	if (mods != sent_mods || memcmp(keys, sent_keys, 6) != 0) {
		kbd_dirty = true;
	}
}

static void mouse_add(itf_t *it, const hp_mouse_t *m)
{
	acc_dx += m->dx;
	acc_dy += m->dy;
	acc_wheel += m->wheel;
	it->buttons = m->buttons;
	if (m->dx || m->dy || m->wheel || merged_buttons() != sent_buttons) {
		mouse_dirty = true;
	}
}

static void decode_boot(itf_t *it, const uint8_t *r, uint16_t len)
{
	if (it->boot == HID_ITF_PROTOCOL_KEYBOARD && len >= 8) {
		hp_kbd_t k = { .mods = r[0] };

		for (int i = 0; i < 6; i++) {
			if (r[2 + i] == 0x01) {
				return; /* ErrorRollOver: keep the previous state */
			}
			k.keys[i] = r[2 + i] >= 0x04 ? r[2 + i] : 0;
		}
		it->kbd = k;
		kbd_changed();
	} else if (it->boot == HID_ITF_PROTOCOL_MOUSE && len >= 3) {
		hp_mouse_t m = {
			.buttons = r[0],
			.dx = (int8_t)r[1],
			.dy = (int8_t)r[2],
			.wheel = len >= 4 ? (int8_t)r[3] : 0,
		};

		mouse_add(it, &m);
	}
}

static void decode_report(itf_t *it, const uint8_t *r, uint16_t len)
{
	const uint8_t *payload;
	uint16_t plen;
	const hp_report_t *rep = hp_lookup(&it->desc, r, len, &payload, &plen);
	hp_kbd_t k;
	hp_mouse_t m;
	uint8_t got;

	if (!rep) {
		return;
	}
	got = hp_decode(rep, payload, plen, &k, &m);
	if ((got & HP_KIND_KBD) && !k.rollover) {
		it->kbd = k;
		kbd_changed();
	}
	if (got & HP_KIND_MOUSE) {
		mouse_add(it, &m);
	}
}

void tuh_hid_mount_cb(uint8_t daddr, uint8_t idx, const uint8_t *desc, uint16_t desc_len)
{
	uint16_t vid = 0, pid = 0;
	uint8_t proto = tuh_hid_interface_protocol(daddr, idx);
	itf_t *it = itf_alloc(daddr, idx);

	tuh_vid_pid_get(daddr, &vid, &pid);
	if (!it) {
		printf("[HID] %04x:%04x itf %u ignored: too many interfaces\r\n", vid, pid, idx);
		return;
	}
	it->proto = proto;

	if (!hp_parse(desc, desc_len, &it->desc)) {
		printf("[HID] %04x:%04x itf %u: malformed report descriptor\r\n", vid, pid, idx);
	}
	it->kinds = hp_kinds(&it->desc);

	if (!it->kinds && proto != HID_ITF_PROTOCOL_NONE) {
		/* Couldn't make sense of the descriptor; fall back to the fixed boot layout */
		it->kinds = proto == HID_ITF_PROTOCOL_KEYBOARD ? HP_KIND_KBD : HP_KIND_MOUSE;
		printf("[HID] %04x:%04x itf %u: switching to boot protocol\r\n", vid, pid, idx);
		if (tuh_hid_set_protocol(daddr, idx, HID_PROTOCOL_BOOT)) {
			return; /* receiving starts in tuh_hid_set_protocol_complete_cb */
		}
	}

	printf("[HID] %04x:%04x itf %u mounted:%s%s\r\n", vid, pid, idx,
	       (it->kinds & HP_KIND_KBD) ? " keyboard" : "",
	       (it->kinds & HP_KIND_MOUSE) ? " mouse" : "");
	if (!tuh_hid_receive_report(daddr, idx)) {
		printf("[HID] itf %u: cannot request reports\r\n", idx);
	}
}

void tuh_hid_set_protocol_complete_cb(uint8_t daddr, uint8_t idx, uint8_t protocol)
{
	itf_t *it = itf_find(daddr, idx);

	if (!it) {
		return;
	}
	if (protocol == HID_PROTOCOL_BOOT) {
		it->boot = tuh_hid_interface_protocol(daddr, idx);
	} else {
		/* device refused boot protocol; nothing we can decode */
		it->kinds = 0;
	}
	if (!tuh_hid_receive_report(daddr, idx)) {
		printf("[HID] itf %u: cannot request reports\r\n", idx);
	}
}

void tuh_hid_umount_cb(uint8_t daddr, uint8_t idx)
{
	itf_t *it = itf_find(daddr, idx);

	if (!it) {
		return;
	}
	printf("[HID] dev %u itf %u removed\r\n", daddr, idx);
	it->used = false;
	/* whatever it was holding is released on the next pass */
	kbd_changed();
	if (merged_buttons() != sent_buttons) {
		mouse_dirty = true;
	}
}

void tuh_hid_report_received_cb(uint8_t daddr, uint8_t idx, const uint8_t *report, uint16_t len)
{
	itf_t *it = itf_find(daddr, idx);

	if (it && it->kinds) {
		if (it->boot) {
			decode_boot(it, report, len);
		} else {
			decode_report(it, report, len);
		}
	}
	tuh_hid_receive_report(daddr, idx);
}

static int16_t take16(int32_t *acc)
{
	int32_t v = *acc;

	if (v > INT16_MAX) {
		v = INT16_MAX;
	} else if (v < INT16_MIN) {
		v = INT16_MIN;
	}
	*acc -= v;
	return (int16_t)v;
}

static int8_t take8(int32_t *acc)
{
	int32_t v = *acc;

	if (v > INT8_MAX) {
		v = INT8_MAX;
	} else if (v < INT8_MIN) {
		v = INT8_MIN;
	}
	*acc -= v;
	return (int8_t)v;
}

static void send_mouse(uint32_t now)
{
	int32_t sx = acc_dx, sy = acc_dy, sw = acc_wheel;
	uint8_t b = merged_buttons();
	int16_t dx = take16(&acc_dx);
	int16_t dy = take16(&acc_dy);
	int8_t w = take8(&acc_wheel);
	uint8_t p[JC_BR_MOUSE_LEN] = {
		b,
		(uint8_t)((uint16_t)dx & 0xFF), (uint8_t)((uint16_t)dx >> 8),
		(uint8_t)((uint16_t)dy & 0xFF), (uint8_t)((uint16_t)dy >> 8),
		(uint8_t)w,
	};

	if (!link_send(LINK_RIGHT, JC_BR_MOUSE, p, sizeof(p))) {
		/* no room: put the motion back and try on the next pass */
		acc_dx = sx;
		acc_dy = sy;
		acc_wheel = sw;
		return;
	}
	sent_buttons = b;
	last_mouse_tx = now;
	mouse_dirty = acc_dx || acc_dy || acc_wheel;
}

static void send_kbd(uint32_t now)
{
	uint8_t p[JC_BR_KBD_LEN];

	merged_kbd(&p[0], &p[1]);
	if (!link_send(LINK_LEFT, JC_BR_KBD, p, sizeof(p))) {
		return;
	}
	sent_mods = p[0];
	memcpy(sent_keys, &p[1], 6);
	last_kbd_tx = now;
	kbd_dirty = false;
}

static void send_status(uint32_t now, uint8_t kinds)
{
	uint8_t flags = (uint8_t)(((kinds & HP_KIND_KBD) ? JC_BR_HAS_KBD : 0) |
				  ((kinds & HP_KIND_MOUSE) ? JC_BR_HAS_MOUSE : 0));
	uint8_t p[JC_BR_STATUS_LEN] = { JC_BR_PROTO_VER, flags, 0 };

	for (int i = 0; i < MAX_ITF; i++) {
		p[2] = (uint8_t)(p[2] + (itfs[i].used ? 1 : 0));
	}
	/* status is periodic, so a dropped one is simply retried next time */
	link_send(LINK_LEFT, JC_BR_STATUS, p, sizeof(p));
	link_send(LINK_RIGHT, JC_BR_STATUS, p, sizeof(p));
	last_status_tx = now;
}

void input_task(void)
{
	uint32_t now = tusb_time_millis_api();
	uint8_t kinds = present_kinds();

	if (!(kinds & HP_KIND_MOUSE)) {
		/* nothing to deliver it to; don't replay it when a mouse appears */
		acc_dx = acc_dy = acc_wheel = 0;
		mouse_dirty = false;
	}
	if (!(kinds & HP_KIND_KBD)) {
		kbd_dirty = false;
	}

	/* the nRF boards drop frames for a kind the status says is absent */
	if (mouse_dirty && (kinds & HP_KIND_MOUSE) &&
	    (merged_buttons() != sent_buttons ||
	     now - last_mouse_tx >= MOUSE_MIN_MS)) {
		send_mouse(now);
	}
	if (kbd_dirty && (kinds & HP_KIND_KBD)) {
		send_kbd(now);
	}

	/* periodic full state, so a corrupted frame can't leave a key stuck */
	if ((kinds & HP_KIND_KBD) && now - last_kbd_tx >= REFRESH_MS) {
		send_kbd(now);
	}
	if ((kinds & HP_KIND_MOUSE) && now - last_mouse_tx >= REFRESH_MS) {
		send_mouse(now);
	}
	if (now - last_status_tx >= JC_BR_STATUS_MS) {
		send_status(now, kinds);
	}
}
