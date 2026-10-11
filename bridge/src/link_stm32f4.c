#include "stm32f4xx_hal.h"

#include "bridge_proto.h"
#include "link.h"

/*
 *   LINK_LEFT : USART1 TX on PB6 (Arduino D10) -> left board
 *   LINK_RIGHT: USART6 TX on PC6 (CN10 pin 4)  -> right board
 */

/* 256-entry ring indexed by uint8_t so the indices wrap for free. */
typedef struct {
	USART_TypeDef *usart;
	volatile uint8_t head; /* written by the main loop */
	volatile uint8_t tail; /* written by the TXE interrupt */
	uint8_t buf[256];
} port_t;

static port_t ports[LINK_COUNT] = {
	[LINK_LEFT] = { .usart = USART1 },
	[LINK_RIGHT] = { .usart = USART6 },
};

static void port_irq(port_t *p)
{
	USART_TypeDef *u = p->usart;

	if (!(u->SR & USART_SR_TXE) || !(u->CR1 & USART_CR1_TXEIE)) {
		return;
	}
	if (p->tail != p->head) {
		u->DR = p->buf[p->tail];
		p->tail = (uint8_t)(p->tail + 1U);
	} else {
		u->CR1 &= ~USART_CR1_TXEIE;
	}
}

void USART1_IRQHandler(void);
void USART1_IRQHandler(void)
{
	port_irq(&ports[LINK_LEFT]);
}

void USART6_IRQHandler(void);
void USART6_IRQHandler(void)
{
	port_irq(&ports[LINK_RIGHT]);
}

static void usart_setup(USART_TypeDef *u, IRQn_Type irq)
{
	u->CR1 = 0;
	u->CR2 = 0;
	u->CR3 = 0;
	u->BRR = UART_BRR_SAMPLING16(HAL_RCC_GetPCLK2Freq(), JC_BR_BAUD);
	u->CR1 = USART_CR1_UE | USART_CR1_TE;
	NVIC_SetPriority(irq, 2);
	NVIC_EnableIRQ(irq);
}

void link_init(void)
{
	GPIO_InitTypeDef pin = {
		.Mode = GPIO_MODE_AF_PP,
		.Pull = GPIO_PULLUP,
		.Speed = GPIO_SPEED_FREQ_VERY_HIGH,
	};

	__HAL_RCC_GPIOB_CLK_ENABLE();
	__HAL_RCC_GPIOC_CLK_ENABLE();
	__HAL_RCC_USART1_CLK_ENABLE();
	__HAL_RCC_USART6_CLK_ENABLE();

	pin.Pin = GPIO_PIN_6;
	pin.Alternate = GPIO_AF7_USART1;
	HAL_GPIO_Init(GPIOB, &pin);

	pin.Pin = GPIO_PIN_6;
	pin.Alternate = GPIO_AF8_USART6;
	HAL_GPIO_Init(GPIOC, &pin);

	/* USART1 and USART6 both sit on APB2 (84 MHz) */
	usart_setup(USART1, USART1_IRQn);
	usart_setup(USART6, USART6_IRQn);
}

void link_task(void)
{
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

	uint8_t h = p->head;

	for (uint8_t i = 0; i < n; i++) {
		p->buf[h] = frame[i];
		h = (uint8_t)(h + 1U);
	}
	__DMB(); /* bytes land before the ISR can see the new head */
	p->head = h;

	/* CR1 is also written by the ISR, so set TXEIE with interrupts masked */
	uint32_t primask = __get_PRIMASK();

	__disable_irq();
	p->usart->CR1 |= USART_CR1_TXEIE;
	__set_PRIMASK(primask);
	return true;
}
