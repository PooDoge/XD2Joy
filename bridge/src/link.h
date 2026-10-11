#ifndef LINK_H_
#define LINK_H_

#include <stdbool.h>
#include <stdint.h>

/*
 * Transmit-only UARTs to the two nRF52840 boards, at JC_BR_BAUD. Pins are
 * in link_stm32f4.c (Nucleo) and link_rp2040.c (XIAO RP2040).
 */
typedef enum {
	LINK_LEFT = 0,
	LINK_RIGHT = 1,
	LINK_COUNT
} link_port_t;

void link_init(void);

/* Moves queued bytes to the UART; a no-op where an interrupt does it. */
void link_task(void);

/* Queues one frame. Returns false, sending nothing, if it doesn't fit. */
bool link_send(link_port_t port, uint8_t type, const uint8_t *payload, uint8_t len);

#endif /* LINK_H_ */
