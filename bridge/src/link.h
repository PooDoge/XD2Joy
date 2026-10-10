#ifndef LINK_H_
#define LINK_H_

#include <stdbool.h>
#include <stdint.h>

/*
 * Transmit-only UARTs to the two nRF52840 boards, at JC_BR_BAUD.
 *   LINK_LEFT : USART1 TX on PB6 (Arduino D10) -> left board P0.08
 *   LINK_RIGHT: USART6 TX on PC6 (CN10 pin 4)  -> right board P0.08
 */
typedef enum {
	LINK_LEFT = 0,
	LINK_RIGHT = 1,
	LINK_COUNT
} link_port_t;

void link_init(void);

/* Queues one frame. Returns false, sending nothing, if it doesn't fit. */
bool link_send(link_port_t port, uint8_t type, const uint8_t *payload, uint8_t len);

#endif /* LINK_H_ */
