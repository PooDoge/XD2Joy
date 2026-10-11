#ifndef HID_PARSE_H_
#define HID_PARSE_H_

#include <stdbool.h>
#include <stdint.h>

/*
 * Small HID report descriptor parser: finds just the keyboard and mouse
 * fields this bridge forwards, with their bit positions, so devices can stay
 * in report protocol (wheel, 16-bit motion, NKRO) instead of boot protocol.
 */

#define HP_MAX_REPORTS 8
#define HP_MAX_ARRAYS 2
#define HP_MAX_BITMAPS 4
#define HP_MAX_BUTTONS 8

typedef struct {
	uint16_t off;
	uint8_t size;
	bool is_signed;
	bool present;
} hp_value_t;

/* Keyboard key array: each slot holds a usage index. */
typedef struct {
	uint16_t off;
	uint8_t size;
	uint8_t count;
	int32_t logical_min;
	uint16_t usage_min;
} hp_array_t;

/* Run of 1-bit keyboard fields (modifiers or an NKRO bitmap). */
typedef struct {
	uint16_t off;
	uint16_t usage_min;
	uint16_t count;
} hp_bitmap_t;

typedef struct {
	uint8_t id;
	uint16_t bits;

	uint8_t button_mask;
	uint16_t button_off[HP_MAX_BUTTONS];
	hp_value_t x, y, wheel;

	uint8_t n_arrays;
	hp_array_t arrays[HP_MAX_ARRAYS];
	uint8_t n_bitmaps;
	hp_bitmap_t bitmaps[HP_MAX_BITMAPS];
} hp_report_t;

typedef struct {
	bool uses_ids;
	uint8_t n;
	hp_report_t r[HP_MAX_REPORTS];
} hp_desc_t;

#define HP_KIND_KBD 0x01
#define HP_KIND_MOUSE 0x02

typedef struct {
	uint8_t mods;
	uint8_t keys[6];
	bool rollover; /* device reported ErrorRollOver: keep the last state */
} hp_kbd_t;

typedef struct {
	uint8_t buttons;
	int32_t dx, dy, wheel;
} hp_mouse_t;

/* Returns false on a malformed descriptor; out is still usable for the reports parsed so far. */
bool hp_parse(const uint8_t *desc, uint16_t len, hp_desc_t *out);

/* HP_KIND_* of the whole descriptor. */
uint8_t hp_kinds(const hp_desc_t *d);

/* Finds the layout for a raw input report (report ID byte included when the descriptor uses IDs). */
const hp_report_t *hp_lookup(const hp_desc_t *d, const uint8_t *report, uint16_t len,
			     const uint8_t **payload, uint16_t *payload_len);

/* HP_KIND_* the report carried; fills kbd and/or mouse for those kinds. */
uint8_t hp_decode(const hp_report_t *r, const uint8_t *payload, uint16_t len,
		  hp_kbd_t *kbd, hp_mouse_t *mouse);

#endif /* HID_PARSE_H_ */
