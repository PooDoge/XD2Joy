#ifndef TUSB_CONFIG_H_
#define TUSB_CONFIG_H_

#ifndef CFG_TUSB_MCU
#error CFG_TUSB_MCU must be defined
#endif

#ifndef CFG_TUSB_OS
#define CFG_TUSB_OS OPT_OS_NONE
#endif

#ifndef CFG_TUSB_DEBUG
#define CFG_TUSB_DEBUG 0
#endif

#define CFG_TUH_MEM_SECTION
#define CFG_TUH_MEM_ALIGN __attribute__((aligned(4)))

#define CFG_TUH_ENABLED 1

#if CFG_TUSB_MCU == OPT_MCU_RP2040
// XIAO RP2040: the host is a software (PIO) USB port on rhport 1, and the
// native USB-C port is a device carrying the log as a CDC serial port.
#define CFG_TUH_RPI_PIO_USB 1
#define BOARD_TUH_RHPORT 1
#define BOARD_TUD_RHPORT 0
#define CFG_TUD_ENABLED 1
#define CFG_TUD_MAX_SPEED OPT_MODE_DEFAULT_SPEED
#define CFG_TUD_MEM_SECTION
#define CFG_TUD_MEM_ALIGN __attribute__((aligned(4)))
#define CFG_TUD_ENDPOINT0_SIZE 64
#define CFG_TUD_CDC 1
#define CFG_TUD_CDC_RX_BUFSIZE 64
#define CFG_TUD_CDC_TX_BUFSIZE 1024
#define CFG_TUD_CDC_EP_BUFSIZE 64
#endif

#ifndef BOARD_TUH_RHPORT
#define BOARD_TUH_RHPORT 0
#endif
#ifndef BOARD_TUH_MAX_SPEED
#define BOARD_TUH_MAX_SPEED OPT_MODE_DEFAULT_SPEED
#endif
#define CFG_TUH_MAX_SPEED BOARD_TUH_MAX_SPEED

// Gaming keyboards with many interfaces have configuration descriptors
// well over 256 bytes.
#define CFG_TUH_ENUMERATION_BUFSIZE 1024
#define CFG_TUH_TASK_QUEUE_SZ 64

// A keyboard and a mouse dongle share the port through a hub. Keyboards
// with a passthrough USB port contain a second hub.
#define CFG_TUH_HUB 2
#define CFG_TUH_DEVICE_MAX (4 * CFG_TUH_HUB + 1)

// Gaming keyboards and combo dongles expose 2-4 HID interfaces each.
#define CFG_TUH_HID 8
#define CFG_TUH_HID_EPIN_BUFSIZE 64
#define CFG_TUH_HID_EPOUT_BUFSIZE 64

#endif /* TUSB_CONFIG_H_ */
