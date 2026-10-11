#ifndef PLATFORM_H_
#define PLATFORM_H_

#include <stdint.h>

/*
 * The few things that differ between the bridge boards: watchdog, clock and
 * reset diagnostics, the log port, and chip-specific USB host warnings.
 * Implemented in plat_stm32f4.c and plat_rp2040.c.
 */

/* After board_init(), before the USB stack starts. */
void plat_init(void);
/* After the USB stack starts: prints diagnostics, starts the watchdog. */
void plat_start(void);
/* Every main-loop pass. */
void plat_task(void);
/* A device enumerated; speed is a tusb_speed_t, hub_addr 0 if direct. */
void plat_device_attached(uint8_t speed, uint8_t hub_addr);

#endif /* PLATFORM_H_ */
