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

#ifndef BOARD_TUH_RHPORT
#define BOARD_TUH_RHPORT 0
#endif
#ifndef BOARD_TUH_MAX_SPEED
#define BOARD_TUH_MAX_SPEED OPT_MODE_DEFAULT_SPEED
#endif
#define CFG_TUH_MAX_SPEED BOARD_TUH_MAX_SPEED

#define CFG_TUH_ENUMERATION_BUFSIZE 256
#define CFG_TUH_TASK_QUEUE_SZ 64

// One hub so a keyboard and a mouse dongle can share the single port.
#define CFG_TUH_HUB 1
#define CFG_TUH_DEVICE_MAX (4 * CFG_TUH_HUB + 1)

// Gaming keyboards and combo dongles expose 2-4 HID interfaces each.
#define CFG_TUH_HID 8
#define CFG_TUH_HID_EPIN_BUFSIZE 64
#define CFG_TUH_HID_EPOUT_BUFSIZE 64

#endif /* TUSB_CONFIG_H_ */
