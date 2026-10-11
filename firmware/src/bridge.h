#ifndef XD2JOY_BRIDGE_H_
#define XD2JOY_BRIDGE_H_

#include <stdbool.h>
#include <stdint.h>

/* UART receiver for the USB host bridge (bridge/ in this repo). */

/* JC_BR_HAS_* flags from the last status frame, 0 when the bridge is silent. */
uint8_t jc_bridge_devices(void);
uint32_t jc_bridge_frames(void);
uint32_t jc_bridge_bad_frames(void);

/*
 * Implemented in main.c. Called from the bridge thread, which is cooperative
 * like the Bluetooth RX thread, so they see the same guarantees the BLE
 * notify path does.
 */
void jc_bridge_kbd(uint8_t mods, const uint8_t keys[6]);
void jc_bridge_mouse(int16_t dx, int16_t dy, int8_t wheel, uint8_t buttons);
/* devices changed; main.c releases held input when its device goes away */
void jc_bridge_changed(uint8_t devices);

#endif /* XD2JOY_BRIDGE_H_ */
