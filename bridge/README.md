# XD2Joy USB bridge

The nRF52840 can only be a USB *device*, so it can't read a wired keyboard or a
2.4 GHz mouse receiver by itself. This bridge is a small USB *host* that does
that and streams the input to both XD2Joy boards over a serial wire:

```
 wired keyboard ─┐                       UART 1 Mbaud
                 ├─ USB hub ─ NUCLEO-F446RE ──── PB6 ──► left nRF  P0.08  (keyboard)
 mouse receiver ─┘                         └─── PC6 ──► right nRF P0.08  (mouse)
```

The boards treat the wired devices exactly like Bluetooth ones: remapping,
profiles, crossmapping, keyboard mode, hotkeys and thumbstick mode all still
work. Bluetooth keeps working too; see [Choosing the input](#choosing-the-input).

## Parts

- NUCLEO-F446RE (any STM32F4 with USB OTG FS can be ported; the Blue Pill
  STM32F103 cannot act as a USB host)
- USB-A female breakout board
- A small USB hub if you want a keyboard *and* a mouse receiver. Use a powered
  hub for RGB keyboards (see Power below).
- 4 jumper wires (+ ground)

## Wiring

> Check the pin labels against the silkscreen or ST's UM1724 pinout. The CN10
> pin numbers below are from that manual's morpho header table, and I haven't
> checked them against a physical board.

| From (Nucleo) | Header pin | To |
|---|---|---|
| PA11 (USB D−) | CN10 pin 14 | USB-A breakout D− |
| PA12 (USB D+) | CN10 pin 12 | USB-A breakout D+ |
| 5V | CN7 pin 18 | USB-A breakout VBUS |
| GND | CN7 pin 8 (or 19, 20, 22) | USB-A breakout GND |
| PB6 (USART1 TX) | CN10 pin 17 / Arduino D10 | **left** board P0.08 (marked RX or D0) |
| PC6 (USART6 TX) | CN10 pin 4 | **right** board P0.08 (marked RX or D0) |
| GND | any GND | GND on both nRF boards |

All signals are 3.3 V on both sides, so no level shifting is needed. The
existing link wire between the two nRF boards (P0.09/P0.10 on the left, P1.04/P1.06 on
the right) stays as it is.

**Power:** the 5 V pin is fed by the Nucleo's ST-LINK USB port, which is
current limited to roughly 500 mA. A plain keyboard plus a mouse receiver fits.
A keyboard with lots of RGB may not; use a powered hub for that.

## Build and flash

Needs `arm-none-eabi-gcc` (e.g. `sudo apt install gcc-arm-none-eabi` or
the Arm GNU Toolchain on Windows/macOS), `cmake`, `ninja`, `python3` and `git`.

```sh
bash bridge/build.sh
```

The first run downloads TinyUSB (pinned to a fixed commit) into
`bridge/.deps/`. Then plug the Nucleo into a computer: it shows up as a drive
named **NOD_F446RE**. Drag `bridge/out/xd2joy_bridge.bin` onto it.

Rebuild and flash the XD2Joy boards from this branch as well. They need the
new UART1 receiver.

## Checking it works

- **LED LD2:** slow blink means it's waiting for devices; fast blink means it's
  forwarding input.
- **Bridge log:** open the Nucleo's virtual COM port at 115200 baud. You
  should see `[USB] device ... attached` and `[HID] ... mounted: keyboard` or
  `mouse` lines.
- **Board side:** send `input` to a board over its USB serial port (the web
  tool's port, or any serial terminal). It answers with
  `[INPUT] src=auto wired=active bridge=0x03 frames=... bad=...`.
  `bad` should stay at or near 0. If it climbs, check the ground wire.

## Choosing the input

Each board has a setting, saved in flash, sent as a line over its USB serial port:

| Command | Behavior |
|---|---|
| `input auto` (default) | A wired device wins when one is plugged in, and Bluetooth scanning pauses so the radio only serves the console. Unplug it and Bluetooth takes over again. |
| `input usb` | Wired only. Drops the Bluetooth device and stops scanning. |
| `input bt` | Ignores the bridge, same as before this change. |

The setting is per board, so you can run, for example, a wired keyboard on the left and
a Bluetooth mouse on the right. The web tool doesn't have a control for this
yet.

A gaming mouse that also exposes a keyboard collection (for macro keys) does
not count as a wired keyboard. The bridge judges devices by their boot
interfaces, so the left board keeps using your Bluetooth keyboard. If a device
is misjudged, set that board to `input bt` or `input usb`.

## What it supports

- Keyboards in report protocol, including NKRO bitmaps. The first 6 keys
  held are forwarded, since that's what the nRF side tracks.
- Mice with up to 8 buttons, 8–32-bit relative X/Y, and a wheel.
- 2.4 GHz combo receivers (keyboard + mouse on one dongle).
- One hub, up to 8 HID interfaces in total.
- Devices whose report descriptor can't be parsed fall back to boot protocol
  (6 keys / 3 buttons + X/Y).

## Protocol

`firmware/src/bridge_proto.h` is the single definition used by both firmwares.
Frames are `C3 | type | len | payload | crc8`. Keyboard frames carry the full
state, and mouse frames carry motion since the last frame. Both are resent at
least every 100 ms, so a corrupted frame can't leave a key stuck. Status frames
every 100 ms tell each board what's plugged in. After 500 ms of silence a
board releases everything and falls back to Bluetooth.

## Tests

The descriptor parser has host-side tests, including a fuzz pass:

```sh
cc -std=c11 -Wall -Wextra -fsanitize=address,undefined -Ibridge/src \
   -o /tmp/hidtest bridge/test/test_hid_parse.c bridge/src/hid_parse.c && /tmp/hidtest
```
