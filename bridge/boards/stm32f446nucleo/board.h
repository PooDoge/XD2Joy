/*
 * NUCLEO-F446RE (MB1136) board definition for the TinyUSB stm32f4 family.
 *
 * Clock: 8 MHz HSE in bypass mode, fed by the ST-LINK MCO (the board's
 * default solder-bridge setup), PLL to 168 MHz SYSCLK and 48 MHz for USB.
 * Debug log: USART2 on PA2/PA3, which is the ST-LINK virtual COM port.
 */
#ifndef BOARD_H_
#define BOARD_H_

#ifdef __cplusplus
extern "C" {
#endif

#define UART_ID 2

#define PINID_LED 0
#define PINID_BUTTON 1
#define PINID_UART_TX 2
#define PINID_UART_RX 3

#define VBUS_SENSE_EN 0

static board_pindef_t board_pindef[] = {
  { // LD2 (green)
    .port = GPIOA,
    .pin_init = { .Pin = GPIO_PIN_5, .Mode = GPIO_MODE_OUTPUT_PP, .Pull = GPIO_NOPULL, .Speed = GPIO_SPEED_LOW, .Alternate = 0 },
    .active_state = 1
  },
  { // B1 (blue), pulled up on the board, low when pressed
    .port = GPIOC,
    .pin_init = { .Pin = GPIO_PIN_13, .Mode = GPIO_MODE_INPUT, .Pull = GPIO_NOPULL, .Speed = GPIO_SPEED_LOW, .Alternate = 0 },
    .active_state = 0
  },
  { // USART2 TX -> ST-LINK VCP
    .port = GPIOA,
    .pin_init = { .Pin = GPIO_PIN_2, .Mode = GPIO_MODE_AF_PP, .Pull = GPIO_PULLUP, .Speed = GPIO_SPEED_HIGH, .Alternate = GPIO_AF7_USART2 },
    .active_state = 0
  },
  { // USART2 RX <- ST-LINK VCP
    .port = GPIOA,
    .pin_init = { .Pin = GPIO_PIN_3, .Mode = GPIO_MODE_AF_PP, .Pull = GPIO_PULLUP, .Speed = GPIO_SPEED_HIGH, .Alternate = GPIO_AF7_USART2 },
    .active_state = 0
  },
};

static inline void board_clock_init(void)
{
  RCC_OscInitTypeDef osc = { 0 };
  RCC_ClkInitTypeDef clk = { 0 };
  RCC_PeriphCLKInitTypeDef per = { 0 };

  __HAL_RCC_PWR_CLK_ENABLE();
  __HAL_PWR_VOLTAGESCALING_CONFIG(PWR_REGULATOR_VOLTAGE_SCALE1);

  osc.OscillatorType = RCC_OSCILLATORTYPE_HSE;
  osc.HSEState = RCC_HSE_BYPASS;
  osc.PLL.PLLState = RCC_PLL_ON;
  osc.PLL.PLLSource = RCC_PLLSOURCE_HSE;
  osc.PLL.PLLM = HSE_VALUE / 1000000; // 1 MHz VCO input
  osc.PLL.PLLN = 336;                 // 336 MHz VCO
  osc.PLL.PLLP = RCC_PLLP_DIV2;       // 168 MHz SYSCLK
  osc.PLL.PLLQ = 7;                   // 48 MHz for USB
  osc.PLL.PLLR = 2;
  HAL_RCC_OscConfig(&osc);

  per.PeriphClockSelection = RCC_PERIPHCLK_CK48;
  per.Clk48ClockSelection = RCC_CK48CLKSOURCE_PLLQ;
  HAL_RCCEx_PeriphCLKConfig(&per);

  clk.ClockType = RCC_CLOCKTYPE_SYSCLK | RCC_CLOCKTYPE_HCLK |
                  RCC_CLOCKTYPE_PCLK1 | RCC_CLOCKTYPE_PCLK2;
  clk.SYSCLKSource = RCC_SYSCLKSOURCE_PLLCLK;
  clk.AHBCLKDivider = RCC_SYSCLK_DIV1;
  clk.APB1CLKDivider = RCC_HCLK_DIV4; // 42 MHz
  clk.APB2CLKDivider = RCC_HCLK_DIV2; // 84 MHz: USART1/6 run 1 Mbaud exactly
  HAL_RCC_ClockConfig(&clk, FLASH_LATENCY_5);
}

// No VBUS switch on this board: the USB-A socket takes 5 V straight from the
// supply (see bridge/README.md).
static inline void board_vbus_set(uint8_t rhport, bool state)
{
  (void) rhport;
  (void) state;
}

#ifdef __cplusplus
}
#endif

#endif /* BOARD_H_ */
