#include "hardware/gpio.h"
#include "hardware/uart.h"

#include "bridge_proto.h"
#include "link.h"

/*
 * Transmit-only UARTs to the two nRF52840 boards.
 *   LINK_LEFT : UART0 TX on GPIO0 (XIAO pad D6)
 *   LINK_RIGHT: UART1 TX on GPIO4 (XIAO pad D9)
 *
 * No interrupts: link_task() tops up each 32-byte hardware FIFO from a ring
 * every main-loop pass. A UART interrupt could preempt the timer interrupt
 * that bit-bangs the PIO USB port and break its timing, and at 1 Mbaud a
 * full FIFO lasts 320 us, far longer than a loop pass.
 */
typedef struct {
	uart_inst_t *uart;
	uint pin;
	uint8_t head;
	uint8_t tail;
	uint8_t buf[256]; /* indexed by uint8_t so the indices wrap for free */
} port_t;

static port_t ports[LINK_COUNT] = {
	[LINK_LEFT] = { .uart = uart0, .pin = 0 },
	[LINK_RIGHT] = { .uart = uart1, .pin = 4 },
};

void link_init(void)
{
	for (int i = 0; i < LINK_COUNT; i++) {
		/* clk_peri is 120 MHz, which divides to exactly 1 Mbaud */
		uart_init(ports[i].uart, JC_BR_BAUD);
		uart_set_fifo_enabled(ports[i].uart, true);
		gpio_set_function(ports[i].pin, GPIO_FUNC_UART);
	}
}

void link_task(void)
{
	for (int i = 0; i < LINK_COUNT; i++) {
		port_t *p = &ports[i];

		while (p->tail != p->head && uart_is_writable(p->uart)) {
			uart_get_hw(p->uart)->dr = p->buf[p->tail];
			p->tail = (uint8_t)(p->tail + 1U);
		}
	}
}

bool link_send(link_port_t port, uint8_t type, const uint8_t *payload, uint8_t len)
{
	port_t *p = &ports[port];
	uint8_t frame[JC_BR_MAX_PAYLOAD + JC_BR_OVERHEAD];
	uint8_t n = (uint8_t)(len + JC_BR_OVERHEAD);
	uint8_t used = (uint8_t)(p->head - p->tail);

	if (len > JC_BR_MAX_PAYLOAD || n > (uint8_t)(255U - used)) {
		return false;
	}

	frame[0] = JC_BR_SYNC;
	frame[1] = type;
	frame[2] = len;
	for (uint8_t i = 0; i < len; i++) {
		frame[3 + i] = payload[i];
	}
	frame[3 + len] = jc_br_crc8(&frame[1], (size_t)len + 2U);

	for (uint8_t i = 0; i < n; i++) {
		p->buf[p->head] = frame[i];
		p->head = (uint8_t)(p->head + 1U);
	}
	link_task();
	return true;
}
