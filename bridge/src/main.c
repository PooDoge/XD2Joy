/*
 * XD2Joy USB host bridge: reads a wired keyboard and a mouse (or a 2.4 GHz
 * receiver) through a hub on the STM32's USB OTG FS port and streams them to
 * the two nRF52840 boards over UART. See bridge/README.md for wiring.
 */
#include <stdio.h>

#include "stm32f4xx.h"

#include "bsp/board_api.h"
#include "tusb.h"

#include "bridge_proto.h"
#include "input.h"
#include "link.h"

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

static void led_task(void)
{
	static uint32_t last;
	static bool on;
	uint32_t now = tusb_time_millis_api();
	/* slow blink: waiting for devices; fast blink: forwarding input */
	uint32_t period = tuh_hid_itf_get_total_count() ? 100 : 500;

	if (now - last < period) {
		return;
	}
	last = now;
	on = !on;
	board_led_write(on);
}

int main(void)
{
	board_init();
	link_init();
	printf("\r\nXD2Joy USB bridge, protocol v%u, %lu baud\r\n",
	       JC_BR_PROTO_VER, (unsigned long)JC_BR_BAUD);

	/* report protocol keeps wheel, 16-bit motion and NKRO; input.c falls
	 * back to boot protocol per interface if a descriptor can't be parsed
	 */
	tuh_hid_set_default_protocol(HID_PROTOCOL_REPORT);

	tusb_rhport_init_t host_init = {
		.role = TUSB_ROLE_HOST,
		.speed = TUSB_SPEED_AUTO,
	};
	tusb_init(BOARD_TUH_RHPORT, &host_init);
	board_init_after_tusb();
	if (!(RCC->PLLCFGR & RCC_PLLCFGR_PLLSRC)) {
		printf("[BRIDGE] WARNING: no 8 MHz clock from the ST-LINK, "
		       "running on the internal RC; USB may be unreliable\r\n");
	}
	if (RCC->CSR & RCC_CSR_IWDGRSTF) {
		printf("[BRIDGE] restarted by the watchdog\r\n");
	}
	RCC->CSR |= RCC_CSR_RMVF;
	wdg_start();

	while (1) {
		IWDG->KR = 0xAAAA;
		tuh_task();
		input_task();
		led_task();
	}
	return 0;
}

void tuh_mount_cb(uint8_t daddr)
{
	uint16_t vid = 0, pid = 0;
	tuh_bus_info_t bus = { 0 };

	tuh_vid_pid_get(daddr, &vid, &pid);
	tuh_bus_info_get(daddr, &bus);
	printf("[USB] device %u attached %04x:%04x %s-speed%s\r\n", daddr, vid,
	       pid, bus.speed == TUSB_SPEED_LOW ? "low" : "full",
	       bus.hub_addr ? " via hub" : "");
	/*
	 * STM32F446 errata ES0298 2.16.2: talking to a low-speed device through
	 * a hub can hang the host's transmitter, and there is no workaround.
	 */
	if (bus.speed == TUSB_SPEED_LOW && bus.hub_addr) {
		printf("[USB] WARNING: low-speed device behind a hub may hang "
		       "this STM32's USB host. Plug it straight into the "
		       "socket.\r\n");
	}
}

void tuh_umount_cb(uint8_t daddr)
{
	printf("[USB] device %u removed\r\n", daddr);
}
