# Sources

Every claim in these docs cites one of the keys below. "Our decode" means one of our
research agents parsed a decrypted capture from ndeadly's repo with its own script and
read the bytes directly; the capture is the source, the parse is ours.

Licences matter if we ever borrow code. MIT, Apache-2.0 and zlib code can go into this
GPL-3.0 firmware with attribution. Repos with no LICENSE file are all-rights-reserved:
we use them as facts only and never copy their text, code or captures. CC BY-NC code
conflicts with GPL-3.0 and with selling a dongle, so we do not copy it. (Licence
compatibility here is our reading, not legal advice.)

## Primary protocol research

| Key | Source | What it gives | Licence |
|---|---|---|---|
| ND-BT | [ndeadly/switch2_controller_research `bluetooth_interface.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/bluetooth_interface.md) | Adverts, GATT table, pairing, console init sequences | none (facts only) |
| ND-CMD | [`commands.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/commands.md) | Command IDs, header, feature bits, flash read/write | none |
| ND-HID | [`hid_reports.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/hid_reports.md) | Input reports 0x05/0x07/0x08/0x09/0x0A, output reports | none |
| ND-MEM | [`memory_layout.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/memory_layout.md) | Flash map (factory data, calibration, pairing store) | none |
| ND-USB | [`descriptors.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/descriptors.md) | USB descriptors and PIDs | none |
| ND-SAFE | [`safe_mode.md`](https://github.com/ndeadly/switch2_controller_research/blob/master/safe_mode.md) | Safe Mode PIDs and procedure | none |
| ND-CAP | [`captures/nrf52840/`](https://github.com/ndeadly/switch2_controller_research/tree/master/captures/nrf52840) | Decrypted console-to-controller sniffs: `btle_joycon2_{pairing,reconnect,wake_console,mouse_mode,ota_update}_decrypted.pcapng`, `btle_procon2_*`, `joycon2.ltk` | none |
| ND-DS | [`datasheets/`](https://github.com/ndeadly/switch2_controller_research/tree/master/datasheets) | IMU, magnetometer, NFC, charger datasheets; 13-pin connector drawing | none |

## Host-side implementations (PC/Android/ESP32 talking to real controllers)

| Key | Source | Licence |
|---|---|---|
| SDL | [libsdl-org/SDL `SDL_hidapi_switch2.c`](https://github.com/libsdl-org/SDL/blob/main/src/joystick/hidapi/SDL_hidapi_switch2.c), [`usb_ids.h`](https://github.com/libsdl-org/SDL/blob/main/src/joystick/usb_ids.h). USB only; BLE explicitly unsupported. | zlib |
| SDLF | [hifihedgehog/SDL `feat/hidmaestro-filter`](https://github.com/hifihedgehog/SDL/tree/feat/hidmaestro-filter/src/joystick/windows) WinRT BLE driver and design doc | zlib |
| SDL38 | [hifihedgehog/SDL issue #38](https://github.com/hifihedgehog/SDL/issues/38) clone analysis, console init order | issue text |
| LNX | Linux "[PATCH v13 0/3] HID: nintendo: Add preliminary Switch 2 controller" (Vicki Pfau, 2026-08-12), read via [ratatoskr.run mirror](https://ratatoskr.run/linux-input/2026/08/17399481/t) (lore.kernel.org was unreachable) | GPL-2.0+ |
| BR | [darthcloud/BlueRetro `main/bluetooth/hidp/sw2.c`, `sw2.h`](https://github.com/darthcloud/BlueRetro/blob/master/main/bluetooth/hidp/sw2.c) | Apache-2.0 |
| J2C | [Misaka10571/joycon2-connector](https://github.com/Misaka10571/joycon2-connector) (fork of [TheFrano/joycon2cpp](https://github.com/TheFrano/joycon2cpp)) | MIT |
| JC2MAC | [OZORDI/JoyCon2Mac](https://github.com/OZORDI/JoyCon2Mac) | MIT |
| NADE | [Nadeflore/switch2-controllers](https://github.com/Nadeflore/switch2-controllers) | none |
| JOY2WIN | [Logan-Gaillard/Joy2Win](https://github.com/Logan-Gaillard/Joy2Win) | MIT |
| J2A | [SWeav02/joycon2android `docs/protocol.md`](https://github.com/SWeav02/joycon2android/blob/main/docs/protocol.md) | GPL-3.0 |
| G77 | [german77/JoyconDriver `switch2/command_handler.lua`](https://github.com/german77/JoyconDriver/blob/master/switch2/command_handler.lua) (Wireshark dissector) | none found |
| PADF | [hifihedgehog/PadForge issue #493](https://github.com/hifihedgehog/PadForge/issues/493) (Joy-Con 2 clones) | CC BY-NC-SA 4.0 |

## Device-side emulators (pretending to be a controller to a real Switch 2)

| Key | Source | Status | Licence |
|---|---|---|---|
| ESPP | [esp-cpp/espp `feat/switch2-pro` / `components/switch2_pro`](https://github.com/esp-cpp/espp/tree/feat/switch2-pro/components/switch2_pro) (README, DESIGN.md) | Pro Controller 2 over BLE; self-reports full operation on ESP32-C6 and S3 | MIT |
| ZHANTSS | [zhantss/ESP32-BLE5-NSController-Emulator](https://github.com/zhantss/ESP32-BLE5-NSController-Emulator) | Pro Controller 2 on ESP32-C61/C6/S3; Joy-Con stubbed | MIT |
| JPOS | [joypad-ai/joypad-os `src/bt/switch2_ble/`](https://github.com/joypad-ai/joypad-os) | nRF52840/BTstack Pro 2 output, unreleased, "a completed connection has not yet been observed" | Apache-2.0 |
| PAIRR | [Inriconus/pair-raught](https://github.com/Inriconus/pair-raught) | nRF52840 Pro Controller over USB, BLE only for wake | AGPL-3.0 |
| GP2040 | [OpenStickCommunity/GP2040-CE](https://github.com/OpenStickCommunity/GP2040-CE) | Switch 1 Pro (057E:2009) and HORIPAD (0F0D:0092) over USB; no Switch 2 native code | MIT |
| HOJA | [HandHeldLegend/HOJA-LIB-RP2040](https://github.com/HandHeldLegend/HOJA-LIB-RP2040), [NS-LIB-HID](https://github.com/HandHeldLegend/NS-LIB-HID) | Switch 1 Pro protocol only | CC BY-NC-SA 3.0 / CC BY-NC 4.0 |

## XD2Joy itself

| Key | Source |
|---|---|
| XD | This repo, `firmware/src/*.c,h` at `bd877eb` (cited as `file:line`). The source has no comments, so purpose is often inferred. |
| XD-PATCH | `ZEPHYR_PATCHES.md`, `zephyr_patches/*.patch` |
| XD-I1 | [desktopsetup/XD2Joy issue #1](https://github.com/desktopsetup/XD2Joy/issues/1) (author's reply, 2026-10-10) |
| XD-REL | [desktopsetup/XD2Joy releases](https://github.com/desktopsetup/XD2Joy/releases) (v1.0, v1.1) |
| XD-STORE | [xd2joy.store](https://xd2joy.store/) FAQ |
| XD-BRIDGE | Branch `claude/project-thread-vw0aaf` (PR #1, wired USB bridge) |

Upstream `desktopsetup/XD2Joy` main is `ab233c7` (2026-10-10). The only commit we lack adds one README bug line about mouse DPI. The webtool HEAD is still `e955f6e`. The author says they develop locally and push after testing (XD-I1), so unpublished changes may exist.

## Hardware, official and community

| Key | Source |
|---|---|
| IFX-CHIP | [iFixit, Nintendo Switch 2 Chip ID](https://www.ifixit.com/Guide/Nintendo+Switch+2+Chip+ID/188725) |
| IFX-TD | [iFixit Switch 2 teardown](https://www.ifixit.com/News/110926/switch-2-teardown), [Right Joy-Con teardown](https://www.ifixit.com/Teardown/Right+Joy-Con+Teardown/181748) |
| SWB | [switchbrew Joy-Con 2](https://switchbrew.org/wiki/Joy-Con_2) |
| PAT | [Nintendo patent WO2025027803A1](https://patents.google.com/patent/WO2025027803A1/en) (mouse sensor) |
| NIN-MOUSE | [Nintendo support: Joy-Con 2 mouse](https://en-americas-support.nintendo.com/app/answers/detail/a_id/68334) |
| NIN-XFER | [Nintendo: Transferring to Switch 2](https://www.nintendo.com/en-gb/Hardware/Nintendo-Switch-2/Transferring-from-Nintendo-Switch/Transferring-to-Nintendo-Switch-2-2785638.html) |
| NIN-GRIP | [Nintendo: Joy-Con 2 Charging Grip](https://support.nintendo.com/sg/switch2/accessory/controller/charge-girp/index.html) |
| NIN-WIRED | [Nintendo: Pro Controller Wired Communication](https://en-americas-support.nintendo.com/app/answers/detail/a_id/26315/) |
| NL-MOUSE | [Nintendo Life: all mouse-mode games](https://www.nintendolife.com/guides/nintendo-switch-2-all-mouse-mode-compatible-games) (Jan 2026) |
| NL-TRIG | [Nintendo Life: GameCube analogue trigger games](https://nintendolife.com/guides/all-nintendo-switch-1-and-2-games-with-analogue-trigger-compatibility-that-work-with-the-gamecube-nso-controller) |
| VGC-USBM | [VGC: Switch 2 supports USB mouse](https://www.videogameschronicle.com/news/switch-2-supports-usb-mouse-controls-developer-reveals/), [Gigazine](https://gigazine.net/gsc_news/en/20250525-nintendo-switch-2-support-usb-mouse) |
| LON | [lon.tv: 8BitDo on Switch 2](https://blog.lon.tv/2025/06/22/8bitdo-controller-compatibility-with-the-switch-2/), [wake](https://blog.lon.tv/2025/10/25/how-to-wake-a-switch-2-with-8bitdo-pro-3-or-ultimate-2-controllers/) |
| GPLA | [Gamepadla: Pro Controller 2 latency](https://gamepadla.com/nintendo-switch-2-pro-controller.html) |
| IFX-TOPIC | [iFixit Joy-Con 2 topic page](https://www.ifixit.com/Topic/Nintendo_Switch_2_Joy-Con) (battery, models; its "Bluetooth 3.0" line is wrong) |
| TECHR | [TechRadar: Joy-Con 2 mouse on the HOME menu](https://www.techradar.com/gaming/the-nintendo-switch-2-joy-con-2-mouse-controls-can-be-used-to-navigate-the-home-menu) |
| GULI | [GoNintendo: GuliKit Switch 2 wake](https://gonintendo.com/contents/49738-gulikit-says-they-re-the-first-third-party-to-offer-controller-wake-up-functionality) |
| GP-TWEET | [TheTrain24 on X](https://x.com/TheTrain24/status/1930708458514554980) (search snippet only; page unreachable) |
| HORI-WHEEL | [HORI Mario Kart Racing Wheel Pro Mini listing](https://www.abt.com/Hori-Mario-Kart-Racing-Wheel-Pro-Mini-for-Nintendo-Switch-2-NSX122U/p/243497.html), [Nintendo Life](https://nintendolife.com/news/2026/03/mario-kart-world-hori-racing-wheels-releasing-for-switch-2-this-month) |
| NREP | [nintendoreporters.com mouse-mode article](https://www.nintendoreporters.com/en/news/nintendo-switch-2/nintendo-switch2-joycon2-mouse-mode-setup-tricks-firsthand-insights/) (unreliable, cited only to reject it) |

## What we could not reach

Recorded so nobody reads silence as "nothing exists":

- Reddit (including r/XD2Joy, which the store FAQ names as the update channel): blocked by the session's network proxy for both WebFetch and curl. No Reddit thread was read.
- lore.kernel.org (used a mirror), famiboards (a Joy-Con 2 DPI thread exists), ResetEra, the Nintendo Life "1,000 Joy-Con" feature, X/Twitter posts.
- GitHub API search (session is scoped to this repo); repos were found by web search and shallow-cloned instead.
- The joycon2android ConsoleSession code (only relayed via SDLF), HandHeldLegend procon2tool.
- No YouTube transcript was retrievable; only chapter titles of the iFixit teardown video.
