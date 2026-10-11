/*
 * Seeed XIAO RP2040 as the XD2Joy USB host bridge.
 *
 * The keyboard and mouse receiver hang off a software (PIO) USB port on two
 * pads; the board's own USB-C port stays a normal USB device for flashing
 * and the serial log. Pins and LED come from pico-sdk's seeed_xiao_rp2040.h.
 *
 *   D7 (GPIO1)   USB D+  to the USB-A socket
 *   D8 (GPIO2)   USB D-  (Pico-PIO-USB puts D- on the pin after D+)
 *   D6 (GPIO0)   UART0 TX -> left nRF board D0
 *   D9 (GPIO4)   UART1 TX -> right nRF board D0
 *
 * USB sits on the right-hand pads next to D9 so the wiring to the socket
 * and the two links don't cross; GPIO1 is UART0's RX pin, unused here.
 */
#ifndef TUSB_BOARD_H
#define TUSB_BOARD_H

#define PICO_DEFAULT_PIO_USB_DP_PIN 1

#endif /* TUSB_BOARD_H */
