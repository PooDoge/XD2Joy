# What the protocol means for a "Switch 2 Multi-Peripheral Adapter"

Jim's framing: grow XD2Joy from keyboard + mouse into an adapter for joysticks, racing
wheels and other PC peripherals. This page says what the console-side protocol allows.
Most of it is our inference from the facts in [joycon2-protocol.md](joycon2-protocol.md);
tags and keys as in [sources.md](sources.md). Nothing is renamed here.

## The console only understands a few identities

There is no generic "HID gamepad over BLE" path into a Switch 2. A device has to look like
one of Nintendo's controllers, each with its own advert PID, GATT UUIDs, report layout,
10/01 type byte and 0x13000 factory block [V: ND-BT, ND-HID]. ESPP reports the console
rejects identities whose parts disagree [U]. So **switching identity is a whole GATT table
plus flash data plus firmware-info, not a PID change** [I]. joypad-os's PID-only override
(`switch2_ble_set_pid`) is unlikely to work for that reason [I].

| Identity | Links | Analog | Mouse | Gyro | Wake | Fit |
|---|---|---|---|---|---|---|
| **Joy-Con 2 pair** (0x2067 + 0x2066), what XD2Joy is | 2 (one board each) | 1 × 12-bit stick per side | **Yes; the only documented way** [I] | real one yes; XD2Joy no | yes | KB/M; mouse-aim games |
| **Pro Controller 2** (0x2069) | 1 | 2 × 12-bit sticks | no | yes (field exists) | yes | joysticks, flight sticks, wheels, gamepads. Frees the second board. Self-reported working by ESPP; ZHANTSS "passed basic tests" on C6 |
| **NSO GameCube** (0x2073) | 1 | 2 sticks + **8-bit analog L/R triggers** | no | unclear: console mask 0x27 includes the IMU bit [I] | yes [I] | analog pedals, but only ~7 games read GC triggers [V: NL-TRIG] |
| Switch 1 Pro over USB (057E:2009) | USB | 2 sticks | no | yes | **no** | wired fallback; needs Pro Controller Wired Communication ON [V: NIN-WIRED]; nRF52840 can't do BT Classic |
| USB HID mouse / keyboard | USB | n/a | per-game USB mouse, **overrides Joy-Con 2 mouse** | n/a | no | XD2Joy's keyboard mode already uses USB keyboard [V: VGC-USBM, XD] |

Table sources: ND-HID, ND-CMD, ND-BT for report fields and masks; NIN-XFER and NIN-WIRED
for the Switch 1 rows; VGC-USBM for USB mouse. Wake for Pro 2 and GC is inferred from the
shared advert format.

Key consequences:

- **Mouse mode requires the Joy-Con 2 identity.** Feature 0x10 and the mouse fields exist
  only in Joy-Con reports [V: ND-HID, ND-CMD]. So the two-board Joy-Con pair stays the right
  shape for KB/M.
- **Wheels, flight sticks and HOTAS** map onto 12-bit stick axes and digital buttons. ZL/ZR
  are digital on every identity except GC [V: ND-HID]. Practical mapping: steering or roll →
  left stick X; pitch or throttle → a stick Y; pedals → stick axes or digital ZR/ZL (Mario
  Kart accelerates digitally) [I]. Licensed HORI Switch 2 wheels map the wheel to the stick
  and have a C button [V: product pages; protocol unknown].
- **A second identity at the same time** (Joy-Con 2 R for the mouse plus a Pro 2 for a stick
  device) is not tested by anyone [U]. It would need a third radio or time-sharing one board,
  and each identity probably needs its own public address and bond, because consoles cache
  per address (joypad-os gives each BLE mode its own address) [I].

## Radio and hardware constraints

- The console requests a **5 ms connection interval** and LE 2M PHY [V: ND-CAP], so the
  console-facing radio has to accept both to hold the link [I]. Stacks reported working: nRF52840 + patched Zephyr open link layer (XD2Joy),
  ESP32-C6/C61 with patched NimBLE [V: ESPP, ZHANTSS, self-reported], ESP32-S3 (ESPP: ESP-IDF
  ≥ 6.1; ZHANTSS: with Espressif's patch) [C]. Reported not to work: Nordic SoftDevice
  Controller, Pico W (CYW43) [V: JPOS].
- The same chip is also a BLE central (to the keyboard/mouse) or reads a USB-host bridge.
  joypad-os scoped the 5 ms allowance to peripheral role only because loosening it globally
  made its central path assert [V: JPOS]. XD2Joy's patch is global and works [V], but that
  is a known risk when adding more central links.
- The nRF52840 is USB-device only, so every wired peripheral goes through a host chip
  (PR #1's STM32 bridge today). The same bridge pattern scales to wheels and sticks: the
  bridge parses the HID report and sends normalised axes/buttons over UART [I].

## Code we could borrow (licence permitting)

- **joypad-os** (Apache-2.0): broad USB-host and BLE-host input drivers (Xbox, PlayStation,
  Switch, 8BitDo, HORI, keyboards, mice, a SideWinder Strategic Commander with twist axes)
  and a router/profile system [V: JPOS README, CHANGELOG]. The best candidate to borrow
  input-side parsers from instead of writing wheel/flight-stick parsers ourselves [I].
- **ESPP switch2_pro** (MIT) and **ZHANTSS** (MIT): a reference Pro Controller 2 identity
  (GATT, init replies, pairing vectors) if we add a Pro 2 mode.
- **GP2040-CE** (MIT): Switch 1 Pro over USB, if a wired persona is wanted.
- Not copyable: ndeadly (no licence; facts only), HOJA / NS-LIB-HID (non-commercial),
  PadForge (non-commercial), pair-raught (AGPL, compatible with GPL-3.0 in one direction
  only; check before copying).

## Suggested order (if Jim wants to go this way)

1. Keep the Joy-Con 2 pair for KB/M; finish the wired bridge (PR #1).
2. Add a "stick device" input class on the bridge (HID joystick/wheel → axes/buttons) and
   map it onto the existing Joy-Con stick and button fields. No new console identity needed.
3. Only then consider a Pro Controller 2 identity on one board, for devices that need two
   sticks, and measure whether the console accepts a Joy-Con 2 R + Pro 2 combination.
