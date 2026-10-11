#include <stdio.h>

#include "hardware/watchdog.h"
#include "pico/stdio/driver.h"

#include "tusb.h"

#include "bridge_proto.h"
#include "platform.h"

/*
 * The log goes out the board's own USB-C port as a CDC serial port, so the
 * UART pads are free for the links. Output is dropped while no terminal is
 * open, so a full buffer never stalls the bridge.
 */
static void cdc_out(const char *buf, int len)
{
	if (len <= 0 || !tud_cdc_connected()) {
		return;
	}
	tud_cdc_write(buf, (uint32_t)len);
	tud_cdc_write_flush();
}

static stdio_driver_t cdc_stdio = {
	.out_chars = cdc_out,
};

void plat_init(void)
{
	stdio_set_driver_enabled(&cdc_stdio, true);

	tusb_rhport_init_t dev_init = {
		.role = TUSB_ROLE_DEVICE,
		.speed = TUSB_SPEED_AUTO,
	};
	tusb_init(BOARD_TUD_RHPORT, &dev_init);
}

void plat_start(void)
{
	if (watchdog_caused_reboot()) {
		printf("[BRIDGE] restarted by the watchdog\r\n");
	}
	/* 1 s; also pauses while a debugger halts the chip */
	watchdog_enable(1000, true);
}

void plat_task(void)
{
	watchdog_update();
	tud_task();
}

void plat_device_attached(uint8_t speed, uint8_t hub_addr)
{
	/* the software USB port sends PRE packets, so low-speed via a hub works */
	(void)speed;
	(void)hub_addr;
}

void tud_cdc_line_state_cb(uint8_t itf, bool dtr, bool rts)
{
	(void)itf;
	(void)rts;
	/* the startup banner went nowhere, so repeat it when a terminal opens */
	if (dtr) {
		printf("\r\nXD2Joy USB bridge (XIAO RP2040), protocol v%u, %lu baud\r\n",
		       JC_BR_PROTO_VER, (unsigned long)JC_BR_BAUD);
	}
}
