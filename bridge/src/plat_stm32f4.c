#include <stdio.h>

#include "stm32f4xx.h"

#include "tusb.h"

#include "platform.h"

void plat_init(void)
{
}

/*
 * Independent watchdog, about 1 s (LSI ~32 kHz / 32, reload 1000). Driven
 * by registers so the HAL IWDG module needn't be enabled.
 */
static void wdg_start(void)
{
	IWDG->KR = 0xCCCC;
	IWDG->KR = 0x5555;
	IWDG->PR = 3; /* /32 */
	IWDG->RLR = 1000;
	while (IWDG->SR) {
	}
	IWDG->KR = 0xAAAA;
}

void plat_start(void)
{
	if (!(RCC->PLLCFGR & RCC_PLLCFGR_PLLSRC)) {
		printf("[BRIDGE] WARNING: no 8 MHz clock from the ST-LINK, "
		       "running on the internal RC; USB may be unreliable\r\n");
	}
	if (RCC->CSR & RCC_CSR_IWDGRSTF) {
		printf("[BRIDGE] restarted by the watchdog\r\n");
	}
	RCC->CSR |= RCC_CSR_RMVF;
	wdg_start();
}

void plat_task(void)
{
	IWDG->KR = 0xAAAA;
}

void plat_device_attached(uint8_t speed, uint8_t hub_addr)
{
	/*
	 * STM32F446 errata ES0298 2.16.2: talking to a low-speed device through
	 * a hub can hang the host's transmitter, and there is no workaround.
	 */
	if (speed == TUSB_SPEED_LOW && hub_addr) {
		printf("[USB] WARNING: low-speed device behind a hub may hang "
		       "this STM32's USB host. Plug it straight into the "
		       "socket.\r\n");
	}
}
