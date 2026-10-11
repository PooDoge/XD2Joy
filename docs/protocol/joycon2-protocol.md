# How Joy-Con 2 controllers talk to the Switch 2

Compiled 2026-10-11 from public third-party research (keys in [sources.md](sources.md)) and
from what the XD2Joy firmware already does. Nintendo publishes none of this.

**Tags.** **[V]** verified: a source we read states or implements it, or we decoded it
from a capture. **[I]** inferred by us. **[U]** unverified: seen only second-hand.
**[C]** sources conflict; both sides are given. Byte offsets are hex. Multi-byte values are
little-endian unless stated.

The core handshake, GATT layout, report layouts and pairing agree across ndeadly's
captures, the host-side tools, the device-side emulators and XD2Joy; the remaining
disagreements are tagged [C] where they occur. Where XD2Joy differs from a
genuine controller it is called out in [xd2joy-vs-genuine.md](xd2joy-vs-genuine.md).

---

## 1. Transport

- Joy-Con 2 (L/R), Pro Controller 2 and the NSO GameCube controller are **Bluetooth LE
  only**: no BR/EDR, no HID-over-GATT, no SMP [V: ND-BT; advert flag 0x06 = BR/EDR not
  supported]. iFixit's topic page says "Bluetooth 3.0"; that is wrong [C: iFixit Joy-Con 2 topic page vs ND-BT/ND-CAP].
- Everything runs over a proprietary GATT service with **fixed handles**. Generic OS HID
  drivers never see these controllers [V: ND-BT, SDLF].
- Link-layer setup after connect, identical in every console capture [V: ND-CAP, our decode]:
  - `LL_LENGTH_REQ/RSP` 251 bytes / 2120 µs.
  - `LL_PHY_REQ 02 02` from the console, then `LL_PHY_UPDATE_IND` to **LE 2M PHY**
    (Joy-Con answers `02 02`, Pro 2 answers `03 03`).
  - ATT MTU 512 both ways.
  - Channel map `1f ff 00 00 00`: data channels 0-12 only (2404-2430 MHz). The reason is
    undocumented; Wi-Fi coexistence is a guess [I].
- **Connection interval** [V: ND-CAP; ESPP]:
  - First pairing (Change Grip/Order): 15 ms (interval 12). ESPP reports the console then
    moves to 5 ms about 1.5 s after input subscription; the 4 s Joy-Con pairing capture
    shows no update [C: minor; may differ by controller type or capture length].
  - Reconnect and wake: **5 ms (interval 4) from the first packet**, latency 0, supervision
    timeout 2 s.
  - 5 ms is **below the BLE spec minimum of 7.5 ms**. The console sets it with a vendor HCI
    command; the controller never sends a parameter-update request [V: ND-BT].
- Radio stacks that refuse 5 ms cannot hold a link: Nordic's SoftDevice Controller refuses
  it [V: JPOS notes]; Zephyr's open link layer needs XD2Joy's `5ms.patch` [V: XD-PATCH];
  ESP32-C6/C61 need a binary patch of `libble_app.a` [V: ESPP, ZHANTSS]; for ESP32-S3,
  ESPP says ESP-IDF ≥ 6.1 has an official option while ZHANTSS says it needs Espressif's
  patch [C]. joypad-os reports the Pico W's CYW43 lacks LE 2M PHY and could not hold a
  Switch 2 link [V: JPOS CHANGELOG; I: that 2M PHY is the cause is their diagnosis]. Zephyr's `CONFIG_BT_CTLR_CONN_INTERVAL_LOW_LATENCY` is
  not a fix: it reinterprets small values as 500 µs units [V: JPOS].

## 2. Advertising

Controller advert: `ADV_IND`, exactly two AD entries: Flags `0x06` and one 26-byte
Manufacturer Specific Data entry. No name, no service UUID. With the 3-byte flags AD it is
exactly 31 bytes [V: ND-BT, ND-CAP, XD main.c:98-114].

| Offset (after AD type 0xFF) | Value | Meaning |
|---|---|---|
| 0x00-0x01 | `53 05` | Company ID 0x0553 (Nintendo) |
| 0x02-0x04 | `01 00 03` | unknown, constant |
| 0x05-0x06 | `7E 05` | VID 0x057E |
| 0x07-0x08 | PID LE | 0x2066 Joy-Con 2 R, 0x2067 L, 0x2069 Pro 2, 0x2073 GC |
| 0x09-0x0A | `00 01` | unknown, constant |
| 0x0B | `00` / `81` | **0x81 = wake the console** |
| 0x0C-0x11 | 6 bytes | Bonded console BD_ADDR, reversed; **all zero = pairable** |
| 0x12 | `0F` | unknown, constant |
| 0x13-0x19 | `00`×7 | reserved |

- Pairing advert example (genuine JC2 R): `1b ff 53 05 01 00 03 7e 05 66 20 00 01 00 00 00 00 00 00 00 0f 00…` [V: ND-CAP].
- Reconnect advert: same, with the console address at 0x0C [V: ND-CAP].
- **Wake**: same plus 0x0B = 0x81, sent on all three primary channels at a measured
  median ~21.2 ms interval [V: ND-CAP, our decode of 485 adverts]. A real Pro 2 measures
  21.25 ms [V: JPOS comment].
- The console scans passively (no `SCAN_REQ`) and sends `CONNECT_IND` ~0.53 ms after an
  `ADV_IND` [V: JPOS, from ND-CAP]. A real Pro 2 repeats one static payload during pairing;
  rotating identity, or setting the wake flag while unbonded, must not be done [V: JPOS].
- The console's own "Search for controllers" advert is `ADV_NONCONN_IND`, flags `0x18`,
  mfg data `53 05 03 00 01 00 80 <controller addr reversed> 01` [V: ND-BT, ND-CAP].
  ndeadly's table puts the trailing `01` at 0x12, which does not fit the 14-byte length
  [V: our decode; minor doc error].
- The controller address must be **public and stable**, since the console bonds to it.
  The OUI does not seem to matter: genuine `48:F1:EB`, joypad-os `94:8E:6D`, XD2Joy
  `B8:68:70` all work [V: ND-CAP, JPOS, XD chip_addr.c; I: OUI not checked].
  joypad-os found "the console ignores anything that isn't public" and that a console that
  failed to pair with an address will not retry it [U: JPOS only].
- Advertisement filtering on the console requires the 0xFF Nintendo entry [U: ND-BT,
  sentence truncated].

## 3. GATT table

The console **does no service discovery**. It exchanges MTU, does two Read-By-Type
requests (for `…bd281` and `…bd283`), then writes to hard-coded handles. A clone must
reproduce the handle numbers exactly [V: ND-CAP].

Value handles (identical on every controller type; per-type UUIDs share handles) [V: ND-BT,
cross-checked against BR, J2C, JC2MAC, NADE and XD main.c:41-90, 2056-2120]:

| Handle | UUID | Props | Use |
|---|---|---|---|
| svc 0x0001 | `00c5af5d-1964-4e30-8f51-1956f96bd280` | | Config service |
| 0x0003 | `…bd281` | R | `04 00 05 00 01 01 00` (meaning unknown) |
| 0x0005 | `…bd282` | W | console writes `01 00` first |
| 0x0007 | `…bd283` | R | 8 bytes, per device (R `29 31 95 87 06 4c 0a 9f` in ND-CAP); meaning unknown |
| svc 0x0008 | `ab7de9be-89fe-49ad-828f-118f09df7fd0` | | Main service |
| 0x000A | `…7fd2` | R/N | **Unified report 0x05** (PC tools use this); CCC 0x000B; vendor desc `679d5510-5a24-4dee-9557-95df80486ecb` 0x000C |
| 0x000E | per type | R/N | **Native report** (console uses this); CCC 0x000F; vendor desc 0x0010 |
| 0x0012 | per type | WNR | Vibration output |
| 0x0014 | `649d4ac9-8eb7-4e6c-af44-1ea54fe5f005` | WNR | Command (no rumble prefix) |
| 0x0016 | per type | WNR | **Vibration + command** (what the console uses) |
| 0x0018 | `4147423d-fdae-4df7-a4f7-d23e5df59f8d` | WNR | Large command / firmware update |
| 0x001A | `c765a961-d9d8-4d36-a20a-5315b111836a` | N | **Command response**; CCC 0x001B; desc `b746df8c-f358-495b-9cd2-e3bbeda4f979` |
| 0x001E | per type | N | Extended response |
| 0x0022 | `d3bd69d2-841c-4241-ab15-f86f406d2a80` | N | unknown |
| 0x0026 | `…7fde` | R/N | unknown |
| 0x002A | `…7fdf` | WNR | unknown (Joy2Win: writing it crashes the controller) |

Per-type UUIDs [V: ND-BT]:

| | Joy-Con 2 L | Joy-Con 2 R | Pro 2 | NSO GC |
|---|---|---|---|---|
| 0x000E input | `cc1bbbb5-7354-4d32-a716-a81cb241a32a` (0x07) | `d5a9e01e-2ffc-4cca-b20c-8b67142bf442` (0x08) | `7492866c-ec3e-4619-8258-32755ffcc0f8` (0x09) | `8261cba1-9435-420c-84d6-f0c75a2c8e4d` (0x0A) |
| 0x0012 vibration | `289326cb-a471-485d-a8f4-240c14f18241` | `fa19b0fb-cd1f-46a7-84a1-bbb09e00c149` | `cc483f51-9258-427d-a939-630c31f72b05` | `3f8fb670-ab25-45bf-b540-38c72834d064` |
| 0x0016 vib+cmd | `ce49a830-dced-48ae-931e-c8cf88aadbea` | `65a724b3-f1e7-4a61-8078-a342376b27ff` | `3dacbc7e-6955-40b5-8eaf-6f9809e8b379` | `af95885e-44b3-4a24-9cf0-483cc129469a` |
| 0x001E ext. resp. | `63a3810f-aec7-474b-9010-3d52403cb996` | `640ca58e-0e88-410c-a7f3-426faf2b690b` | `506d9f7d-4278-4e95-a549-326ba77657e0` | `46f6ad29-cdaf-4569-a2fe-339020b94604` |

- After 0x002A come GAP and an **empty** Generic Attribute service [V: ND-BT].
- Pro 2 on firmware ≥ 2.0.0 adds headset-audio characteristics (value handles 0x002C,
  0x002E, 0x0032), shifting GAP/GATT up by 8 handles [V: ND-BT]. Whether an emulated Pro 2 needs them is disputed: ESPP says a pad
  without them looks like factory firmware; JPOS says including them reproduces a
  "discovers table then rejects" symptom [C].
- Console writes on connect: `0x0005 ← 01 00`, CCCs `0x001B/0x001F/0x0023 ← 01 00`,
  commands to 0x0016, `0x0010 ← 85 00`, `0x000F ← 01 00` (starts native reports),
  rumble to 0x0012 [V: ND-CAP]. `85 00` is labelled "Set Report Rate?" by ndeadly; its
  meaning is unknown [U].
- Attributes must not require encryption: on reconnect the console reads and writes
  them before it starts LL encryption [V: ND-CAP; I: implication].

## 4. Pairing and security

No SMP. If a host starts SMP the controller drops the link [V: ND-BT, ESPP, J2A].
Pairing is application-level over command **0x15** [V: ND-BT, ND-CAP; implemented
identically by XD main.c:1919-1974, ESPP, JPOS, ZHANTSS].

Order on the wire is 01, 04, 02, 03 [V: ND-CAP]:

1. **15/01** console → `00 02 <console addr> <console addr with low byte −1>`
   (both reversed). Controller → `01 04 01 <own addr>`.
2. **15/04** console → `00 <A1, 16 bytes>`. Controller → `01 <B1>` where
   **B1 = `5C F6 EE 79 2C DF 05 E1 BA 2B 63 25 C4 1A 5F 10`, a fixed constant**.
   **LTK = A1 XOR B1** (bytes as sent).
3. **15/02** console → `00 <A2 challenge>`. Controller →
   `01 <B2>`, **B2 = AES-128-ECB(key = reverse(LTK), plaintext = reverse(A2))**, sent
   without re-reversing.
4. **15/03** console → `00`. Controller → `01`, stores host + LTK.
5. Console then sends **03/07** (second host address + LTK, reversed) and **03/09**
   (store). The genuine controller answers both with a bare header [V: ND-CAP].
6. Console starts **LL encryption** with `LL_ENC_REQ` EDIV = 0, Rand = 0 using the LTK.

- Check vector, recomputed from the capture: A1 `08065a60e902e4e102029e3fa39a78d1`,
  A2 `934e580f163aeecfb575fc9136b22fbb` → B2 `881ae4a3a04b4d75b1941445b178ef2f`; the wire
  LTK reversed equals ndeadly's `joycon2.ltk` [V: our computation]. ESPP and JPOS publish a
  second vector from a Pro 2 capture [V].
- Controller flash: count byte at 0x1FA000, then 0x28-byte entries from 0x1FA008, each with
  the host address at entry+0x00 and the LTK at entry+0x12 (first entry: 0x1FA008 /
  0x1FA01A); the two console addresses share one LTK [V: ND-MEM, ND-CAP].
- **No per-device attestation has been observed.** B1 is fixed and A1/A2 are arbitrary (BR
  and NADE hard-code their own) [V: BR, NADE]. Commercial clones such as the NYXI Hyperion 3
  copy the Joy-Con 2 R identity and are sold as Switch 2 controllers [U: PADF, SDL38].
  XD2Joy itself works with the same fixed key [V: README]. A future console update could
  add a check [I]. ESPP: "a possession check, not per-device attestation" [V].
- 03/07 + 03/09 alone can install a host and LTK, bypassing 0x15 [V: ND-CMD]. Rail
  pairing ("attach to pair") probably uses this path [I].
- Reconnect skips all of 0x15 and 03/07-09: an unencrypted prelude (MTU, two Read-By-Type,
  CCCDs, 07/01, read 0x13000, 10/01, 16/01), then LL encryption ~0.5-0.65 s after connect
  [V: ND-CAP].
- A synced Joy-Con 2 in wake/reconnect advertising refuses GATT connections from anyone but
  its bonded host [V: J2A].

## 5. Command layer

Commands are written to **0x0016** behind a vibration prefix, or to 0x0014 bare
[V: ND-BT, ND-CAP]. The prefix is 17 bytes for a Joy-Con (report ID `00` + 16 rumble
bytes), 0x21 for Pro 2, 0x05 for GC [V: ND-BT, ND-HID]. ESPP's "33-byte prefix" for Pro 2 is the same 0x21 [V].

8-byte header [V: ND-CMD, LNX, ND-CAP]:

| Byte | Request | Response |
|---|---|---|
| 0 | command ID | command ID |
| 1 | `0x91` | `0x01` (NACK flag bit 2 = `0x04`) |
| 2 | transport `0x01` BLE / `0x00` USB | same |
| 3 | subcommand | subcommand |
| 4 | `0x00` | `0x10` on BLE |
| 5 | data length | ack `0x78` BLE / `0xF8` USB |
| 6-7 | `00 00` | `00 00` |

**Response channel [C]:** ndeadly's prose says responses to 0x0016 writes arrive on 0x001E
with a zero prefix. Every decoded Joy-Con 2 capture (pairing, reconnect, wake, mouse,
OTA) shows them as notifications on **0x001A** (`c765a961`) with no prefix [V: ND-CAP].
XD2Joy replies on 0x001A and the console accepts it. SDLF reads replies from either.

### Command IDs [V: ND-CMD, G77; disagreements noted]

| ID | Area | Subcommands seen |
|---|---|---|
| 0x01 | NFC | 0x03 scan, 0x04 leave, 0x05 status, 0x06 read, 0x14/0x15 buffer read/write (ND and G77 swap the names [C]). 01/0C reply is `61 12 50 0d` on genuine JC2; ND labels it unknown, XD2Joy treats it as power info [C] |
| 0x02 | Flash | 01 read block, 02 write block, 03 erase, **04 read ≤ 0x4F bytes (BLE)**: request `[len] 7E 00 00 [addr u32]`, reply `[len] 00 00 00 [addr u32] + data`; 05 write, only at ≥ 0x1F5000 |
| 0x03 | Init (USB/rail) | 01 BT wake, 02 BT cancel, 03 enable USB reports, 07 send pairing info, 08 clear pairing, 09 store pairing, 0A select input report, 0D init USB |
| 0x06 | Power? | 02 "shutdown controller?", 03 "reboot controller?" (ND's question marks; G77 adds that 02 is sent over USB when a docked console sleeps [I]; 06/03 ends the OTA in ND-CAP) |
| 0x07 | unknown | 07/01 is the first command the console sends; 1-byte reply |
| 0x08 | Charging grip | 01 info (0x20 bytes, grip PID 0x2068), 02 enable GL/GR, 03 info (0x40) |
| 0x09 | Player LEDs | 01-06 presets, **07 bitmask** (bits 0-3 = LEDs 1-4), 08 flash |
| 0x0A | Vibration | 02 play built-in sample, 08 raw data |
| 0x0B | Battery | 03 voltage mV, 04 charge state |
| 0x0C | **Feature select** | 01 get, 02 set mask, 03 clear, 04 enable, 05 disable, 06 configure |
| 0x0D | Firmware update | 01-07 (can brick a real controller) |
| 0x10 | Firmware info | 10/01 |
| 0x11 | unknown | 01, 03 (exact blobs; JPOS: "must match a real pad exactly or the console keeps re-probing") |
| 0x13 | Joy-Con only, unknown | |
| 0x15 | Pairing | §4 |
| 0x16 | unknown | 16/01 → 0x18 zero bytes |
| 0x17, 0x18 | unknown | 18/01 → 8-byte blob (JPOS) |

**Feature bits (0x0C)** [V: ND-CMD, LNX]: `0x01` buttons, `0x02` sticks, `0x04` IMU,
`0x10` **mouse (Joy-Con only)**, `0x20` rumble (ND) / "current" (G77) [C on the name; on
Joy-Con it also enables battery current in report 0x05], `0x80` magnetometer. The console
uses **0x37 for Joy-Con 2**, 0x2F for Pro 2, 0x27 for GC [V: ND-BT, ND-CAP]. On reconnect
the console may send only 0C/02, so emulators treat set-mask as enable [V: JPOS, ESPP].

**10/01 firmware info** [V: ND-CMD, ND-CAP]: 3 bytes version (major.minor.micro), 1 byte
type (`00` JC L, `01` JC R, `02` Pro, `03` GC), BT patch version, DSP version (Pro only).
Genuine JC2 R in the capture: `01 00 0e 01 0c 00 00 00 ff ff ff ff` (1.0.14). ESPP says the
console cross-checks this type byte against the PID and GATT and rejects a mismatch
[U: ESPP observation].

### Console init sequence for a Joy-Con 2 [V: ND-BT, ND-CAP, SDL38]

```
07/01
02/04 read 0x40 @ 0x13000          (factory data; JPOS: console validates serial+VID/PID here)
10/01
16/01
[first pairing only] 15/01 15/04 15/02 15/03 03/07 03/09  → LL encryption
0A/02 sample 03                     (connect "click")
09/07 LEDs 01                       (player 1)
0C/02 mask 0x37
02/04 0x13080 (0x40), 0x1FC040 (0x40), 0x13040 (0x10), 0x13100 (0x18)
11/03
02/04 0x13060 (0x20)
0A/08 01 59 09 00 00 ff ff ff ff 35 00 46 …
11/01
0C/04 0x37
write 0x0010 ← 85 00 ; CCC 0x000F ← 01 00     → native reports start
rumble on 0x0012 ; 10/01 ; 01/0C
```

Pro 2 uses mask 0x2F and also reads 0x130C0; GC uses 0x27 and reads 0x13140 (2 bytes)
[V: ND-BT]. The genuine JC2 took ~240 ms to answer 0C/02 [V: ND-CAP]. The `59 09` in 0A/08
matches the value just read from 0x13060 [I: our observation, likely LRA calibration].

## 6. Flash (SPI) memory map

Served to the console through 02/04 [V: ND-MEM, SDL, BR, XD XData_*.h]:

| Address | Size | Contents |
|---|---|---|
| 0x13000 | 0x40 | +0x02 serial (ASCII, 16), +0x12 VID, +0x14 PID, +0x16 3 unknown, **+0x19 body RGB, +0x1C buttons RGB, +0x1F 3rd colour, +0x22 4th colour** |
| 0x13040 | 0x10 | gyro bias floats (SDL reads +4/+8/+12) [V: SDL; I: meaning] |
| 0x13060 | 0x20 | starts `59 09`/`46 09`/`42 09`… (vibration/LRA data?) [I] |
| 0x13080 | 0x40 | stick calibration at **0x130A8**, 9 bytes [V] |
| 0x130C0 | 0x40 | second stick (Pro/GC) at 0x130E8 |
| 0x13100 | 0x18 | accel bias floats (SDL +12/+16/+20) |
| 0x13140 | 2 | GC trigger zero points |
| 0x1FA000 | | pairing store (§4) |
| 0x1FC000 | | user motion calibration |
| 0x1FC040 | 0x40 | user stick calibration, magic `B2 A1`; all 0xFF = none |
| 0x1FC060 / 0x1FC080 | | user secondary stick: ND and BR say 0x1FC060, LNX and SDL say 0x1FC080 [C] |

- Stick calibration: 3 packed 12-bit pairs: centre, max (positive range), min (negative
  range). Nine 0xFF bytes = uncalibrated; clones ship blank calibration [V: SDL, LNX, PADF].
- Colour names for +0x1F/+0x22: ndeadly says "highlight"/"grip", XD2Joy and its webtool say
  "rail"/"stick" [C]. Stock JC2 values are body `32 32 32`, buttons `aa aa aa`, 3rd
  `9b e1 e6` (L) / `ff 8c 5f` (R), 4th `32 32 32` [V: ND-CAP, XD].
- Flash is 2 MB; firmware header magic `0xAA640001 'SYS '` [V: ND-MEM].

## 7. Input reports

Two families [V: ND-HID]:

- **0x05 "unified"** on 0x000A (`…7fd2`). This is what every PC tool reads.
- **Native** reports on 0x000E: **0x07** JC L, **0x08** JC R, **0x09** Pro 2, **0x0A** GC.
  **The console uses these.** Over BLE there is no report-ID byte (USB offset = BLE + 1).

Genuine JC2 R in mouse mode sends one 63-byte notification **every 5 ms (~200 Hz)**: median
gap exactly 5000 µs; two independent decodes counted 4192 and 4212 of 4221 gaps at 5.0 ms,
with a few 10 ms gaps (missed connection events) among the rest [V: ND-CAP, our decode].

### 7.1 Native report 0x08 (Joy-Con 2 R) and 0x07 (L), 63 bytes

| Offset | Size | Field |
|---|---|---|
| 0x00 | 1 | counter, +1 per report |
| 0x01 | 1 | power: bit0 external power, bit1 charging, bits 2-5 battery level 0-9 |
| 0x02 | 2 | buttons (below) |
| 0x04 | 1 | `0x07` free; XD2Joy uses `0x01` = in charging grip [V: ND-CAP shows 0x07 constant; I: 0x01 from XD only] |
| 0x05 | 3 | stick, two packed 12-bit values: `x = b0 \| (b1&0x0F)<<8`, `y = b1>>4 \| b2<<4` |
| 0x08 | 1 | `0x38` (constant in captures) |
| 0x09 | 2 | **mouse ΔX, s16** |
| 0x0B | 2 | **mouse ΔY, s16** |
| 0x0D | 1 | **mouse surface/lift byte**; `0xFF` = no surface / not tracking |
| 0x0E | 1 | R: NFC state. L: motion length |
| 0x0F | 1 | R: motion length (0, 0x1E or 0x28). L: motion starts here |
| 0x10 | 0x28 | R: packed motion block (format undocumented) |

Button bits, bytes 0x02-0x03 as u16 [V: ND-HID; XD main.c:123-152 defines all except right
0x0080 R-stick and GR]:

| Bit | Left | Right |
|---|---|---|
| 0x0001 | Down | B |
| 0x0002 | Right | A |
| 0x0004 | Left | Y |
| 0x0008 | Up | X |
| 0x0010 | L | R |
| 0x0020 | ZL | ZR |
| 0x0040 | Minus | Plus |
| 0x0080 | L stick | R stick |
| 0x0100 | Capture | Home |
| 0x0400 | GL (grip) | GR (grip) [I] |
| 0x1000 | | C (GameChat) |
| 0x4000 | SR | SR |
| 0x8000 | SL | SL |

0x0400 for GL/GR comes only from XD2Joy; ndeadly leaves that bit blank [I for GR: XD defines
GL only (`main.c:152`), GR by symmetry].

### 7.2 Mouse mode

- Native reports carry **relative per-report deltas**, s16 at 0x09/0x0B [V: ND-HID, ND-CAP].
  In the mouse capture ΔX ranged −67..+82 and ΔY −61..+66 per 5 ms report; steady movement
  shows constant deltas (~+17), not an accumulating counter [V: our decode]. R and ZR are
  the mouse buttons [V: ND-CAP buttons 0x0010/0x0020].
- **Byte 0x0D**: 0xFF for the first ~1.5 s, before tracking starts; then 5-233 (mode ~18-25)
  and never 0xFF again in that capture. **No motion is reported while 0x0D = 0xFF**
  [V: our decode]. ndeadly calls it "lift-off distance?". It looks like a surface-quality or
  distance metric where 0xFF = invalid [I]. The capture contains no lift event, so "0xFF on
  lift" is not proven [U].
- Report **0x05** carries mouse differently: **absolute** 16-bit X/Y counters at 0x10/0x12
  that wrap, plus "surface quality?" at 0x14 and "lift-off distance?" at 0x16 [V: ND-HID].
  PC tools difference them with 16-bit wraparound [V: J2C, NADE]. NADE treats the Joy-Con
  as on a surface when distance ≠ 0 and < 1000 and roughness < 4000 [V].
- Mouse data exists only in Joy-Con reports. Pro 2 (0x09) and GC (0x0A) have no mouse
  field, and feature 0x10 is "Joy-Con only" [V: ND-HID, ND-CMD]. So we infer **only a
  Joy-Con 2 identity can do native mouse mode** [I].
- Console behaviour [V: NIN-MOUSE, TECHR]: the HOME cursor appears when a detached
  Joy-Con 2 is laid on its rail side on a surface and moved, and disappears when lifted.
  Settings has Mouse Sensitivity High/Medium/Low and an on/off toggle. One or two Joy-Con 2
  can be mice at once. How the console decides "on a surface" is undocumented; 0x0D and/or
  IMU orientation are the likely inputs [I].
- Around 100 games support mouse mode; Drag x Drive, Mouse Work and parts of Welcome Tour
  and Super Mario Party Jamboree NS2 Edition require it [V: NL-MOUSE].
- Sensor: no source we could read identifies the optical sensor IC, its CPI or lift-off
  distance (searched iFixit, switchbrew, ND-DS and PixArt part numbers; see the unreachable
  list in sources.md). Nintendo's
  patent describes a light source + receiver + lens behind an opening on the rail side, with
  detection failing beyond 1 cm / 5 mm / 1 mm in various embodiments [V: PAT]. A
  nintendoreporters.com piece claiming "Motion Tuning" menus and a 1 kHz rate contradicts
  Nintendo's docs and cites nothing; ignore it [C: NREP vs NIN-MOUSE].

### 7.3 Motion (IMU)

- Native reports: a length byte (0x1E or 0x28) then a **packed block whose format is not
  publicly documented** [V: ND-HID]. The genuine JC2 R used 0x1E in 4217 of 4222 mouse-mode
  reports [V: our decode].
- XD2Joy's constants imply a partial decode: 3 sample slots at bit 56 + 88·s, a u16
  timestamp at block+5 incrementing at ~602 per second, and six 16-bit fields it zeroes at
  bit 154 + 89·sm + 16·k (sm 0-1, k 0-2). Note the 89-bit stride of those fields differs
  from the 88-bit sample stride [V: XD main.c:4110-4199 constants; I: their meaning].
- Report 0x05 has a documented block at 0x2A: u32 timestamp µs, s16 temperature, s16 accel
  X/Y/Z (±8 g, 4096 LSB/g) at 0x30/0x32/0x34, s16 gyro X/Y/Z (±2000 dps) at 0x36/0x38/0x3A
  [V: ND-HID, SDL]. A conflicting scale "48000 raw = 360 dps" appears in joycon2cpp and
  JoyCon2Mac [C]. ndeadly and SDL agree with each other (~0.061 dps/LSB, ±2000 dps full
  scale); "48000 = 360 dps" would imply ~245 dps full scale.
- A clone's console report carries accel at block+0x12 (int16 in the high half of three
  4-byte words, 4096 LSB/g) and no gyro [U: SDLF quoting joycon2android].
- An all-zero motion block is accepted by the console for Pro 2 [V: ESPP].

### 7.4 Pro 2 (0x09) and GameCube (0x0A)

- 0x09: counter, power, 3-byte buttons at 0x02 (B A Y X R ZR Plus RS / Down Right Left Up L
  ZL Minus LS / Home Capture GR GL C), left stick 0x05, right stick 0x08, 0x0B = 0x38 when
  feature bit 5 is set else 0x30 (JPOS: the console drops mismatching reports), 0x0C NFC,
  0x0D headset, 0x0E motion length, 0x0F motion [V: ND-HID, JPOS].
- 0x0A: same skeleton, **analog L/R triggers at 0x0C/0x0D** (8-bit), buttons relabelled.
  The only native report with analog triggers [V: ND-HID].

### 7.5 Unified report 0x05 (for reference)

0x00 u32 counter; 0x04 u32 buttons (different bit order from native; see ND-HID);
0x0A/0x0D sticks; 0x10 mouse (8 bytes); 0x19 magnetometer 3×s16 (0.15 µT/LSB);
0x1F u16 battery mV; 0x21 charge state; 0x22 u16 current (0.01 mA); 0x2A motion;
0x3C/0x3D GC triggers [V: ND-HID]. joycon2cpp's README offset table is misaligned with its
own (correct) code [V: our decode of its example].

## 8. Output: rumble, LEDs, sounds

- Vibration on 0x0012: `00`, then `0x50 | seq`, then 5-byte LRA samples; Joy-Con one group,
  Pro two (left at 0x01, right at 0x11), GC 4 bytes [V: ND-HID, BR, J2C]. The bit packing
  of the 5-byte sample differs across implementations and ndeadly says it is not fully
  understood [C]. BR's version: `lf_freq:9, lf_en:1, lf_amp:10, hf_freq:9, hf_en:1, tbd:1,
  enable:1` + `hf_amp:8`.
- 0A/02 built-in samples: 0 stop, 1 buzz ~1 s, 2 "find controller", 3 connect click (sent on
  every connect), 4 pairing (G77) / low battery (ND) [C], 5 stronger click, 6-7 beeps
  [V: ND-CMD, G77].
- Player LEDs 09/07, first data byte bitmask [V: ND-CMD]. SDL and JPOS map players
  `1,3,7,F,9,5,D,6`; BR uses `1,2,4,8,3,6,C,F` [V].

## 9. Battery and power

- Native report byte 0x01: bits 2-5 level 0-9 [V: ND-HID].
- 0B/03 voltage mV (e.g. `a5 0e` = 3749 mV), 0B/04 charge state [V: ND-CMD].
- 01/0C reply on genuine JC2: `61 12 50 0d` [V: ND-CAP]. Reading it as [%, ?, mV LE] is
  XD2Joy's interpretation [I].
- Each Joy-Con 2 has a 500 mAh cell; models BEE-012 (L) and BEE-014 (R) [V: IFX-TOPIC].

## 10. Wired: USB, rail and charging grip

- USB IDs (VID 0x057E): JC2 R 0x2066, L 0x2067, pair/grip 0x2068, Pro 2 0x2069, GC 0x2073;
  Safe Mode 0x2070 (R), 0x2071 (L), 0x2072 (Pro), 0x2074 (GC) [V: ND-USB, ND-SAFE, SDL].
  BlueRetro's `sw2.h` swaps the L/R defines; don't copy them [V: BR vs all others].
- USB: interface 0 HID (EP 0x81/0x01, 64 bytes, bInterval 4), interface 1 vendor bulk
  (0x02/0x82) carrying the same command protocol with transport `0x00` and ack `0xF8`.
  Silent until 03/0D "Initialise USB" [V: ND-USB, SDL, LNX].
- Joy-Con 2 have no USB port, yet present USB descriptors and enumerate as "Nintendo Safe
  Mode Device" through the Charging Grip; ndeadly tags 0x03 as "USB/rail". The rail very
  likely carries **USB 2.0**, not the Switch 1 Joy-Con UART [I: SWB, ND-SAFE, ND-CMD].
  No pinout is public. ND-DS includes a drawing of a 13-pin, 1.00 mm pitch connector
  (pin count and pitch only) [V].
- Charging grip attached to console or dock "automatically changes the connection to a
  wired connection" [V: NIN-GRIP].
- Safe Mode: hold ZR+Plus+Sync (R) or ZL+Minus+Sync (L) [V: ND-SAFE].

## 11. Firmware updates (OTA)

User-initiated "Update Controllers": 0A/02 calls, 0D/01, 0D/02 (target bank 0x75000),
0D/03 (image size), then 0D/04 4108-byte blocks over **0x0018** in chunked writes each
acknowledged on 0x001A, then 0D/05, 0D/06 (size + CRC-32), 0D/07, 06/03 reboot; ~15 s.
The image starts `0xAA640001 'SYS '` and is encrypted [V: ND-CAP OTA, our decode; C on block
size: ndeadly's prose says ≤ 0xB5C bytes in 30 chunks, the capture shows 42 chunks per
0x1000 block].

## 12. Hardware

| Part | Source |
|---|---|
| MediaTek MT3689BCA BLE SoC | IFX-CHIP, SWB |
| IMU: ICM-42607-P (iFixit) vs ICM-42670-P (switchbrew); ND-DS ships both datasheets | [C] |
| AKM AK09919C magnetometer | IFX-CHIP, SWB |
| TI BQ25618 charger, MAX98388 amp, TPS2553 USB switch | IFX-CHIP |
| NXP PN7160/PN71602 NFC (right only) | SWB, ND-DS |
| Optical mouse sensor | **unidentified** |
| Sticks: potentiometer, not Hall (magnets would interfere) | IFX-TD |

## 13. Other Switch 2 input paths

- Switch 1 Joy-Con and Pro Controllers work on Switch 2 but **cannot wake it** and cannot
  be used where a game needs Joy-Con 2 features like the mouse [V: NIN-XFER]; presumably
  over Bluetooth Classic as on Switch 1 [I]. Third-party pads in Switch 1 Pro mode (8BitDo) work with motion and
  standard rumble [V: LON]. 8BitDo firmware 2.0 [V: LON] and GuliKit [V: GULI] added
  Switch 2 wake later; the mechanism isn't stated, most likely the native 0x81 advert [I].
- "Pro Controller Wired Communication" exists on Switch 2 [V: NIN-WIRED]. GP2040-CE's
  HORIPAD-style 0F0D:0092 USB mode reportedly works on Switch 2 [U: GP-TWEET, search snippet only].
- **USB mice** work per game (Nobunaga's Ambition), and a USB mouse **takes priority over
  Joy-Con 2 mouse** [V: VGC-USBM]. USB keyboards work for text entry (XD2Joy's keyboard mode
  relies on this) [V: XD usb_dev.c, README].
- ZL/ZR are digital bits in the Joy-Con 2 and Pro 2 reports; only the NSO GC report has
  analog triggers [V: ND-HID]. Only a few titles read them (F-Zero GX, Luigi's Mansion,
  Sunshine, Wind Waker, GRID Autosport/Legends, Trials Rising) [V: NL-TRIG].
- Licensed HORI Mario Kart wheels for Switch 2 are wired USB with a C button; their protocol
  and whether pedals are analog are unknown [V: HORI-WHEEL; I: C button implies a
  Switch 2-aware protocol].
- We found no latency measurement for any Switch 2 controller on the console (Reddit and
  other forums were unreachable). The only figure found is Gamepadla's wired-to-PC
  Pro Controller 2: 7.97 ms average buttons [V: GPLA].
