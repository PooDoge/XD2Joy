/*
 * Seeed XIAO RP2040 as the XD2Joy USB host bridge.
 *
 * The keyboard and mouse receiver hang off a software (PIO) USB port on two
 * pads; the board's own USB-C port stays a normal USB device for flashing
 * and the serial log. Pins and LED come from pico-sdk's seeed_xiao_rp2040.h.
 *
 *   D0 (GPIO26)  USB D+  to the USB-A socket
 *   D1 (GPIO27)  USB D-  (Pico-PIO-USB puts D- on the pin after D+)
 *   D6 (GPIO0)   UART0 TX -> left nRF board D0
 *   D9 (GPIO4)   UART1 TX -> right nRF board D0
 */
#ifndef TUSB_BOARD_H
#define TUSB_BOARD_H

#define PICO_DEFAULT_PIO_USB_DP_PIN 26

#endif /* TUSB_BOARD_H */
