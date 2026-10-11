/*
 * Wire format between the USB host bridge (bridge/) and each nRF52840.
 * Shared verbatim by both firmwares, so change it in one place only.
 *
 * Frame: SYNC | TYPE | LEN | PAYLOAD[LEN] | CRC8(TYPE..PAYLOAD)
 * CRC-8 poly 0x07, init 0x00, MSB first (same as the board-to-board link).
 * One UART per nRF: the bridge sends keyboard frames to the left board and
 * mouse frames to the right board, and status frames to both.
 */
#ifndef XD2JOY_BRIDGE_PROTO_H_
#define XD2JOY_BRIDGE_PROTO_H_

#include <stdint.h>
#include <stddef.h>

#define JC_BR_BAUD 1000000

#define JC_BR_SYNC 0xC3
#define JC_BR_MAX_PAYLOAD 12
#define JC_BR_OVERHEAD 4 /* sync, type, len, crc */

#define JC_BR_PROTO_VER 1

/* mods, keys[6] (boot keyboard layout, usage page 0x07) */
#define JC_BR_KBD 0x01
#define JC_BR_KBD_LEN 7

/* buttons, dx (i16 LE), dy (i16 LE), wheel (i8). dx/dy/wheel are deltas
 * accumulated since the previous mouse frame; buttons is the full state.
 */
#define JC_BR_MOUSE 0x02
#define JC_BR_MOUSE_LEN 6

/* version, flags, devices. Sent every JC_BR_STATUS_MS; a board that hears
 * nothing for JC_BR_LOST_MS treats the bridge as gone and releases input.
 */
#define JC_BR_STATUS 0x03
#define JC_BR_STATUS_LEN 3
#define JC_BR_HAS_KBD 0x01
#define JC_BR_HAS_MOUSE 0x02

#define JC_BR_STATUS_MS 100
#define JC_BR_LOST_MS 500

static inline uint8_t jc_br_crc8(const uint8_t *p, size_t n)
{
	uint8_t crc = 0;

	while (n--) {
		crc ^= *p++;
		for (int i = 0; i < 8; i++) {
			crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07)
					   : (uint8_t)(crc << 1);
		}
	}
	return crc;
}

#endif /* XD2JOY_BRIDGE_PROTO_H_ */
