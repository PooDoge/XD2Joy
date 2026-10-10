/* Host-side tests for hid_parse.c:  cc -I../src -o t test_hid_parse.c ../src/hid_parse.c && ./t */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "hid_parse.h"

static int fails;

#define CHECK(cond)                                                          \
	do {                                                                 \
		if (!(cond)) {                                               \
			printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
			fails++;                                             \
		}                                                            \
	} while (0)

/* HID 1.11 appendix B.1 boot keyboard, as most keyboards report it */
static const uint8_t kbd_desc[] = {
	0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7,
	0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01,
	0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01,
	0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
	0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65,
	0x81, 0x00, 0xC0,
};

/* Gaming mouse style: report ID 1, 5 buttons, 16-bit X/Y, 8-bit wheel */
static const uint8_t mouse_desc[] = {
	0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xA1, 0x00,
	0x05, 0x09, 0x19, 0x01, 0x29, 0x05, 0x15, 0x00, 0x25, 0x01, 0x95, 0x05,
	0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x81, 0x01, 0x05, 0x01,
	0x16, 0x01, 0x80, 0x26, 0xFF, 0x7F, 0x75, 0x10, 0x95, 0x02, 0x09, 0x30,
	0x09, 0x31, 0x81, 0x06, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01,
	0x09, 0x38, 0x81, 0x06, 0xC0,
	/* report ID 2: consumer control, must be ignored */
	0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x02, 0x15, 0x00, 0x26, 0xFF,
	0x03, 0x19, 0x00, 0x2A, 0xFF, 0x03, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00,
	0xC0, 0xC0,
};

/* NKRO: modifiers + a 120-bit key bitmap (usages 0x00..0x77), report ID 3 */
static const uint8_t nkro_desc[] = {
	0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, 0x03, 0x05, 0x07, 0x19, 0xE0,
	0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
	0x19, 0x00, 0x29, 0x77, 0x95, 0x78, 0x81, 0x02, 0xC0,
};

static void test_boot_keyboard(void)
{
	hp_desc_t d;
	hp_kbd_t k;
	hp_mouse_t m;
	const uint8_t *p;
	uint16_t n;
	const uint8_t rep[8] = { 0x02, 0, 0x04, 0x05, 0, 0, 0, 0 }; /* LShift + A + B */
	const uint8_t rollover[8] = { 0, 0, 1, 1, 1, 1, 1, 1 };

	CHECK(hp_parse(kbd_desc, sizeof(kbd_desc), &d));
	CHECK(hp_kinds(&d) == HP_KIND_KBD);
	const hp_report_t *r = hp_lookup(&d, rep, sizeof(rep), &p, &n);

	CHECK(r != NULL);
	CHECK(hp_decode(r, p, n, &k, &m) == HP_KIND_KBD);
	CHECK(k.mods == 0x02);
	CHECK(k.keys[0] == 0x04 && k.keys[1] == 0x05 && k.keys[2] == 0);
	CHECK(!k.rollover);

	r = hp_lookup(&d, rollover, sizeof(rollover), &p, &n);
	hp_decode(r, p, n, &k, &m);
	CHECK(k.rollover);
}

static void test_mouse(void)
{
	hp_desc_t d;
	hp_kbd_t k;
	hp_mouse_t m;
	const uint8_t *p;
	uint16_t n;
	/* id 1, buttons 1+3, dx = -300, dy = +1000, wheel = -1 */
	const uint8_t rep[7] = { 0x01, 0x05, 0xD4, 0xFE, 0xE8, 0x03, 0xFF };
	const uint8_t cc[3] = { 0x02, 0xE9, 0x00 };

	CHECK(hp_parse(mouse_desc, sizeof(mouse_desc), &d));
	CHECK(d.uses_ids);
	CHECK(hp_kinds(&d) == HP_KIND_MOUSE);
	const hp_report_t *r = hp_lookup(&d, rep, sizeof(rep), &p, &n);

	CHECK(r != NULL && r->id == 1);
	CHECK(hp_decode(r, p, n, &k, &m) == HP_KIND_MOUSE);
	CHECK(m.buttons == 0x05);
	CHECK(m.dx == -300);
	CHECK(m.dy == 1000);
	CHECK(m.wheel == -1);

	r = hp_lookup(&d, cc, sizeof(cc), &p, &n);
	CHECK(r != NULL && r->id == 2);
	CHECK(hp_decode(r, p, n, &k, &m) == 0);

	/* short report must not read past the end */
	r = hp_lookup(&d, rep, 3, &p, &n);
	hp_decode(r, p, n, &k, &m);
	CHECK(m.dy == 0);
}

static void test_nkro(void)
{
	hp_desc_t d;
	hp_kbd_t k;
	hp_mouse_t m;
	const uint8_t *p;
	uint16_t n;
	uint8_t rep[1 + 1 + 15] = { 0x03, 0x11 }; /* LCtrl + RCtrl */

	/* press 8 keys: a..h (0x04..0x0B); only 6 fit the boot format */
	for (int u = 0x04; u <= 0x0B; u++) {
		rep[2 + u / 8] |= (uint8_t)(1U << (u % 8));
	}
	CHECK(hp_parse(nkro_desc, sizeof(nkro_desc), &d));
	CHECK(hp_kinds(&d) == HP_KIND_KBD);
	const hp_report_t *r = hp_lookup(&d, rep, sizeof(rep), &p, &n);

	CHECK(r != NULL);
	CHECK(hp_decode(r, p, n, &k, &m) == HP_KIND_KBD);
	CHECK(k.mods == 0x11);
	CHECK(k.keys[0] == 0x04 && k.keys[5] == 0x09);
}

static void test_garbage(void)
{
	hp_desc_t d;
	uint8_t junk[300];

	srand(1);
	for (int round = 0; round < 2000; round++) {
		for (size_t i = 0; i < sizeof(junk); i++) {
			junk[i] = (uint8_t)rand();
		}
		hp_parse(junk, (uint16_t)(rand() % (int)sizeof(junk)), &d);
		CHECK(d.n <= HP_MAX_REPORTS);
	}
	CHECK(!hp_parse((const uint8_t[]){ 0x26, 0xFF }, 2, &d));
}

int main(void)
{
	test_boot_keyboard();
	test_mouse();
	test_nkro();
	test_garbage();
	printf("%s (%d failures)\n", fails ? "FAILED" : "ok", fails);
	return fails ? 1 : 0;
}
