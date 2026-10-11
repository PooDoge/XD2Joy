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
  STM32F103 cannot act as a USB host), or a Seeed XIAO RP2040 (see
  [Using a XIAO RP2040 instead](#using-a-xiao-rp2040-instead))
- USB-A female breakout board
- A small USB hub if you want a keyboard *and* a mouse receiver. Use a powered
  hub for RGB keyboards (see Power below). Devices behind the hub should be
  full-speed (see [Low-speed devices](#low-speed-devices)).
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

### Powering everything from one 5 V supply

One fused 5 V supply can run the socket, the hub, an RGB keyboard and both
nRF boards:

- Put a 2 A fuse or polyfuse on the supply's +5 V before anything branches
  off. Go to 3 A if a bright RGB keyboard trips it.
- Feed the USB-A socket's VBUS from the supply, and remove the wire from the
  Nucleo's 5V (CN7 pin 18) to VBUS so two sources never meet. Put a 470 µF
  capacitor, rated 10 V or more, across VBUS and GND near the socket.
- Power each nRF board through its own USB-C socket with a short pigtail
  that carries only VBUS and GND. Don't feed 5 V into B+ (battery input,
  4.2 V max) or 3V3 (regulator output). Swap the lead for a data cable when
  you need the web tool, flashing or keyboard mode.
- Never power the nRF boards from the hub's ports, because they would
  enumerate on the bridge.
- Optional: run the Nucleo from the supply too, by moving jumper JP5 to E5V
  and feeding CN7 pin 6. Power the supply on before plugging in the ST-LINK
  USB. These pins come from UM1724 and haven't been checked on a board.
- Join every ground. Before connecting devices, the supply should read
  5.0 to 5.2 V unloaded, and VBUS at the socket should stay above 4.75 V
  with the keyboard lit.

### Low-speed devices

STM32F446 errata ES0298 (2.16.2) says the USB host's transmitter can hang
when it talks to a *low-speed* device through a hub, and ST lists no
workaround. Gaming and RGB keyboards and most 2.4 GHz receivers are
full-speed, but cheap office keyboards and mice are often low-speed. The
bridge log prints each device's speed when it attaches, and a warning when a
low-speed device is behind a hub. Plug such a device straight into the
socket instead, or swap it for a full-speed one.

## Build and flash

Needs `arm-none-eabi-gcc` (e.g. `sudo apt install gcc-arm-none-eabi` or
the Arm GNU Toolchain on Windows/macOS), `cmake`, `ninja`, `python3` and `git`.

```sh
bash bridge/build.sh
```

The first run downloads TinyUSB (pinned to a fixed commit) into
`bridge/.deps/`. Then plug the Nucleo into a computer: it shows up as a drive
named **NOD_F446RE**. Drag `bridge/out/nucleo/xd2joy_bridge.bin` onto it.

Rebuild and flash the XD2Joy boards from this branch as well. They need the
new UART1 receiver.

## Using a XIAO RP2040 instead

A Seeed XIAO RP2040 can replace the Nucleo. It is thumbnail-sized, and it
doesn't have the STM32's low-speed-through-a-hub bug, because its USB host
port is software running on the RP2040's PIO blocks (the
[Pico-PIO-USB](https://github.com/sekigon-gonnoc/Pico-PIO-USB) library). The
nRF boards can't tell the difference: same frames, same 1 Mbaud, same input
pin, so their firmware doesn't change.

This port builds, but hasn't been run on hardware yet.

**Wiring** (pads seen from the top, USB-C up):

| XIAO RP2040 pad | GPIO | To |
|---|---|---|
| D0 | GPIO26 | USB-A socket D+ |
| D1 | GPIO27 | USB-A socket D− |
| D6 | GPIO0 (UART0 TX) | 1 kΩ, then the **left** nRF board's bridge input (D0 on a XIAO nRF52840) |
| D9 | GPIO4 (UART1 TX) | 1 kΩ, then the **right** nRF board's bridge input (D0 on a XIAO nRF52840) |
| GND | | USB-A socket GND, both nRF boards' GND |

- Power the USB-A socket's VBUS from 5 V that doesn't pass through the XIAO:
  the fused supply rail (see
  [Powering everything from one 5 V supply](#powering-everything-from-one-5-v-supply)).
  Treat the XIAO's own 5V pad as wired to its USB-C VBUS, like the nRF
  boards' 5V pads: USB or the pad, or the pad through a Schottky diode.
- D+ and D− are bit-banged at 12 Mbit/s, so keep those two wires under about
  10 cm and twisted together.
- The keyboard and the mouse receiver still share the one port through a hub.
  Low-speed devices behind the hub are fine here.

**Build and flash:**

```sh
bash bridge/build.sh xiao-rp2040
```

The first run also downloads the Pico SDK (2.1.1) and builds its `picotool`
helper. Hold the XIAO's **B** (boot) button while plugging it in: it shows up
as a drive named **RPI-RP2**. Drag `bridge/out/xiao-rp2040/xd2joy_bridge.uf2`
onto it.

**Checking it works:** the XIAO's USB-C port is now a serial port named
*XD2Joy USB bridge* (any baud rate). Open it to see the same `[USB]` and
`[HID]` log lines as the Nucleo prints. The banner repeats each time a
terminal connects. The blue LED blinks slowly while waiting for devices and
fast while forwarding input.

## Checking it works

- **LED LD2:** slow blink means it's waiting for devices; fast blink means it's
  forwarding input.
- **Bridge log:** open the Nucleo's virtual COM port at 115200 baud. You
  should see `[USB] device ... attached ... full-speed via hub` and
  `[HID] ... mounted: keyboard` or `mouse` lines. A watchdog restarts the
  bridge if it ever stops running, and the log says so when it does.
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
