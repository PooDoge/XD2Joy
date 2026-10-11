#include <string.h>

#include "hid_parse.h"

#define PAGE_DESKTOP 0x01
#define PAGE_KEYBOARD 0x07
#define PAGE_BUTTON 0x09

#define USAGE_X 0x30
#define USAGE_Y 0x31
#define USAGE_WHEEL 0x38

#define KEY_ROLLOVER 0x01
#define KEY_FIRST 0x04
#define KEY_LCTRL 0xE0
#define KEY_RGUI 0xE7

#define MAX_USAGES 16
#define MAX_PUSH 2

/* Input item flag bits */
#define IN_CONSTANT 0x01
#define IN_VARIABLE 0x02
#define IN_RELATIVE 0x04

typedef struct {
	uint16_t page;
	int32_t logical_min;
	uint16_t report_size;
	uint16_t report_count;
	uint8_t report_id;
} globals_t;

/* Usages are stored as (page << 16) | id. */
typedef struct {
	uint32_t list[MAX_USAGES];
	uint8_t n;
	uint32_t min, max;
	bool has_min, has_max;
} locals_t;

static uint32_t item_unsigned(const uint8_t *p, uint8_t n)
{
	uint32_t v = 0;

	for (uint8_t i = 0; i < n; i++) {
		v |= (uint32_t)p[i] << (8U * i);
	}
	return v;
}

static int32_t item_signed(const uint8_t *p, uint8_t n)
{
	uint32_t v = item_unsigned(p, n);

	if (n == 1) {
		return (int8_t)v;
	}
	if (n == 2) {
		return (int16_t)v;
	}
	return (int32_t)v;
}

static uint32_t full_usage(const globals_t *g, uint32_t u, uint8_t item_size)
{
	/* a 4-byte usage carries its own page in the high half */
	return item_size == 4 ? u : ((uint32_t)g->page << 16) | (u & 0xFFFFU);
}

static bool usage_at(const locals_t *l, uint16_t i, uint32_t *out)
{
	if (l->n) {
		/* the last usage repeats for any remaining fields */
		*out = l->list[i < l->n ? i : l->n - 1U];
		return true;
	}
	if (l->has_min) {
		uint32_t u = l->min + i;

		if (l->has_max && u > l->max) {
			u = l->max;
		}
		*out = u;
		return true;
	}
	return false;
}

static hp_report_t *report_for(hp_desc_t *d, uint8_t id)
{
	for (uint8_t i = 0; i < d->n; i++) {
		if (d->r[i].id == id) {
			return &d->r[i];
		}
	}
	if (d->n >= HP_MAX_REPORTS) {
		return NULL;
	}
	hp_report_t *r = &d->r[d->n++];

	memset(r, 0, sizeof(*r));
	r->id = id;
	return r;
}

static void bitmap_add(hp_report_t *r, uint16_t off, uint16_t usage)
{
	if (r->n_bitmaps) {
		hp_bitmap_t *last = &r->bitmaps[r->n_bitmaps - 1U];

		if (last->off + last->count == off &&
		    last->usage_min + last->count == usage) {
			last->count++;
			return;
		}
	}
	if (r->n_bitmaps < HP_MAX_BITMAPS) {
		r->bitmaps[r->n_bitmaps++] = (hp_bitmap_t){ .off = off, .usage_min = usage, .count = 1 };
	}
}

static void add_input(hp_report_t *r, const globals_t *g, const locals_t *l, uint32_t flags)
{
	uint16_t size = g->report_size;
	uint16_t count = g->report_count;
	uint16_t base = r->bits;

	r->bits = (uint16_t)(r->bits + (uint32_t)size * count);
	if ((flags & IN_CONSTANT) || size == 0) {
		return;
	}

	if (!(flags & IN_VARIABLE)) {
		uint32_t u0;

		if (!usage_at(l, 0, &u0)) {
			return;
		}
		if ((u0 >> 16) == PAGE_KEYBOARD && size <= 16 && r->n_arrays < HP_MAX_ARRAYS) {
			r->arrays[r->n_arrays++] = (hp_array_t){
				.off = base,
				.size = (uint8_t)size,
				.count = count > 255 ? 255 : (uint8_t)count,
				.logical_min = g->logical_min,
				.usage_min = (uint16_t)u0,
			};
		}
		return;
	}

	for (uint16_t i = 0; i < count; i++) {
		uint32_t u;

		if (!usage_at(l, i, &u)) {
			break;
		}
		uint16_t page = (uint16_t)(u >> 16);
		uint16_t id = (uint16_t)u;
		uint16_t off = (uint16_t)(base + (uint32_t)i * size);

		if (page == PAGE_BUTTON && size == 1 && id >= 1 && id <= HP_MAX_BUTTONS) {
			r->button_off[id - 1U] = off;
			r->button_mask |= (uint8_t)(1U << (id - 1U));
		} else if (page == PAGE_DESKTOP && (flags & IN_RELATIVE) && size >= 2 && size <= 32 &&
			   (id == USAGE_X || id == USAGE_Y || id == USAGE_WHEEL)) {
			hp_value_t *v = id == USAGE_X ? &r->x : id == USAGE_Y ? &r->y : &r->wheel;

			*v = (hp_value_t){
				.off = off,
				.size = (uint8_t)size,
				.is_signed = g->logical_min < 0,
				.present = true,
			};
		} else if (page == PAGE_KEYBOARD && size == 1) {
			bitmap_add(r, off, id);
		}
	}
}

bool hp_parse(const uint8_t *p, uint16_t len, hp_desc_t *d)
{
	globals_t g = { 0 };
	globals_t stack[MAX_PUSH];
	uint8_t sp = 0;
	locals_t l = { 0 };
	hp_report_t *cur = NULL;
	uint16_t i = 0;

	memset(d, 0, sizeof(*d));

	while (i < len) {
		uint8_t b = p[i++];

		if (b == 0xFE) {
			/* long item: bDataSize, bLongItemTag, data */
			if (i >= len) {
				return false;
			}
			i = (uint16_t)(i + 2U + p[i]);
			continue;
		}

		uint8_t sz = (uint8_t)(b & 0x03U);

		if (sz == 3) {
			sz = 4;
		}
		if ((uint32_t)i + sz > len) {
			return false;
		}
		const uint8_t *data = &p[i];

		i = (uint16_t)(i + sz);

		uint8_t type = (uint8_t)((b >> 2) & 0x03U);
		uint8_t tag = (uint8_t)(b >> 4);
		uint32_t u = item_unsigned(data, sz);

		switch (type) {
		case 0: /* main */
			if (tag == 0x8) {
				if (!cur) {
					cur = report_for(d, g.report_id);
				}
				if (cur) {
					add_input(cur, &g, &l, u);
				}
			}
			memset(&l, 0, sizeof(l));
			break;
		case 1: /* global */
			switch (tag) {
			case 0x0:
				g.page = (uint16_t)u;
				break;
			case 0x1:
				g.logical_min = item_signed(data, sz);
				break;
			case 0x7:
				g.report_size = (uint16_t)u;
				break;
			case 0x8:
				g.report_id = (uint8_t)u;
				d->uses_ids = true;
				cur = report_for(d, g.report_id);
				break;
			case 0x9:
				g.report_count = (uint16_t)u;
				break;
			case 0xA:
				if (sp < MAX_PUSH) {
					stack[sp++] = g;
				}
				break;
			case 0xB:
				if (sp) {
					g = stack[--sp];
					cur = report_for(d, g.report_id);
				}
				break;
			default:
				break;
			}
			break;
		case 2: /* local */
			if (tag == 0x0 && l.n < MAX_USAGES) {
				l.list[l.n++] = full_usage(&g, u, sz);
			} else if (tag == 0x1) {
				l.min = full_usage(&g, u, sz);
				l.has_min = true;
			} else if (tag == 0x2) {
				l.max = full_usage(&g, u, sz);
				l.has_max = true;
			}
			break;
		default:
			break;
		}
	}
	return true;
}

static bool report_is_mouse(const hp_report_t *r)
{
	return r->x.present || r->y.present;
}

static bool report_is_kbd(const hp_report_t *r)
{
	return r->n_arrays || r->n_bitmaps;
}

uint8_t hp_kinds(const hp_desc_t *d)
{
	uint8_t k = 0;

	for (uint8_t i = 0; i < d->n; i++) {
		if (report_is_mouse(&d->r[i])) {
			k |= HP_KIND_MOUSE;
		}
		if (report_is_kbd(&d->r[i])) {
			k |= HP_KIND_KBD;
		}
	}
	return k;
}

const hp_report_t *hp_lookup(const hp_desc_t *d, const uint8_t *report, uint16_t len,
			     const uint8_t **payload, uint16_t *payload_len)
{
	if (!d->uses_ids) {
		*payload = report;
		*payload_len = len;
		return d->n ? &d->r[0] : NULL;
	}
	if (len < 1) {
		return NULL;
	}
	for (uint8_t i = 0; i < d->n; i++) {
		if (d->r[i].id == report[0]) {
			*payload = report + 1;
			*payload_len = (uint16_t)(len - 1U);
			return &d->r[i];
		}
	}
	return NULL;
}

static bool bit_at(const uint8_t *p, uint16_t len, uint16_t off)
{
	if ((off >> 3) >= len) {
		return false;
	}
	return (p[off >> 3] >> (off & 7U)) & 1U;
}

static uint32_t bits_at(const uint8_t *p, uint16_t len, uint16_t off, uint8_t size)
{
	uint32_t v = 0;

	for (uint8_t k = 0; k < size; k++) {
		if (bit_at(p, len, (uint16_t)(off + k))) {
			v |= 1UL << k;
		}
	}
	return v;
}

static int32_t value_at(const hp_value_t *f, const uint8_t *p, uint16_t len)
{
	uint32_t v;

	if (!f->present) {
		return 0;
	}
	v = bits_at(p, len, f->off, f->size);
	if (f->is_signed && f->size < 32 && (v & (1UL << (f->size - 1U)))) {
		v |= ~((1UL << f->size) - 1U);
	}
	return (int32_t)v;
}

static void kbd_add(hp_kbd_t *k, uint8_t *n, uint32_t usage)
{
	if (usage == KEY_ROLLOVER) {
		k->rollover = true;
	} else if (usage >= KEY_LCTRL && usage <= KEY_RGUI) {
		k->mods |= (uint8_t)(1U << (usage - KEY_LCTRL));
	} else if (usage >= KEY_FIRST && usage <= 0xFF && *n < 6) {
		for (uint8_t j = 0; j < *n; j++) {
			if (k->keys[j] == usage) {
				return;
			}
		}
		k->keys[(*n)++] = (uint8_t)usage;
	}
}

uint8_t hp_decode(const hp_report_t *r, const uint8_t *p, uint16_t len,
		  hp_kbd_t *kbd, hp_mouse_t *mouse)
{
	uint8_t kinds = 0;

	if (report_is_mouse(r)) {
		memset(mouse, 0, sizeof(*mouse));
		for (uint8_t b = 0; b < HP_MAX_BUTTONS; b++) {
			if ((r->button_mask & (1U << b)) && bit_at(p, len, r->button_off[b])) {
				mouse->buttons |= (uint8_t)(1U << b);
			}
		}
		mouse->dx = value_at(&r->x, p, len);
		mouse->dy = value_at(&r->y, p, len);
		mouse->wheel = value_at(&r->wheel, p, len);
		kinds |= HP_KIND_MOUSE;
	}

	if (report_is_kbd(r)) {
		uint8_t n = 0;

		memset(kbd, 0, sizeof(*kbd));
		for (uint8_t i = 0; i < r->n_bitmaps; i++) {
			const hp_bitmap_t *bm = &r->bitmaps[i];

			for (uint16_t j = 0; j < bm->count; j++) {
				if (bit_at(p, len, (uint16_t)(bm->off + j))) {
					kbd_add(kbd, &n, (uint32_t)bm->usage_min + j);
				}
			}
		}
		for (uint8_t i = 0; i < r->n_arrays; i++) {
			const hp_array_t *a = &r->arrays[i];

			for (uint8_t s = 0; s < a->count; s++) {
				int32_t v = (int32_t)bits_at(p, len, (uint16_t)(a->off + (uint32_t)s * a->size), a->size);
				int32_t idx = v - a->logical_min;

				if (idx < 0) {
					continue;
				}
				uint32_t usage = (uint32_t)a->usage_min + (uint32_t)idx;

				if (usage) {
					kbd_add(kbd, &n, usage);
				}
			}
		}
		kinds |= HP_KIND_KBD;
	}
	return kinds;
}
