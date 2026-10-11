# XD2Joy compared with a genuine Joy-Con 2

How the firmware in this repo (`bd877eb`) implements each part of
[joycon2-protocol.md](joycon2-protocol.md), where it differs from a real controller, and
what those differences suggest we try. Tags and source keys as in
[sources.md](sources.md). The firmware has no comments, so any "why" below is **[I]**
unless the author said it somewhere.

## Does it already pass a mouse through as a Joy-Con 2 in mouse mode?

**Yes.** The author documents "Mouse mode: A Bluetooth mouse emulates the Joy-Cons mouse
state" as a working feature [V: README]. In code, the right board is a Joy-Con 2 (R)
identity (PID 0x2066, R-side GATT UUIDs) and writes the mouse into the same native-report
fields a genuine Joy-Con 2 uses for its optical sensor [V: code vs ND-HID, ND-CAP]. We have
not tested it on hardware ourselves.

| Report byte | XD2Joy writes | Genuine JC2 R | Code |
|---|---|---|---|
| 0x09-0x0A | s16 ΔX for this report | s16 ΔX per 5 ms report | `main.c:4387-4413` |
| 0x0B-0x0C | s16 ΔY | s16 ΔY | same |
| 0x0D | `0xFF` until the mouse first moves on this console connection, and whenever stick mode is on; otherwise the replayed captured value (0x39-0x3F in the right board's rows 25-37) | `0xFF` before tracking, then 5-233 | `main.c:4257`, `4394-4396` |
| 0x02-0x03 | mapped mouse buttons; default left → R, right → ZR | R / ZR are the clicks | `main.c:274-276`, `4312-4331` |

Mouse pipeline [V: `main.c:1011-1185`, `2944-2996`]:

1. Each BLE HID report is decoded (report-map parse, or by length) to x/y/wheel/buttons.
2. `rs_push()` stores timestamped cumulative positions in a 32-entry ring, merging inputs
   under 2 ms apart, and tracks an EMA of the mouse's report interval (`rs_span`, starts at
   11.25 ms).
3. Each console report, `rs_emit()` interpolates the path at `now − rs_span − 2 ms` and
   emits the difference since the last emit. This smooths the beat between the mouse rate
   and the console rate at the cost of roughly one mouse interval + 2 ms of delay [I].
4. Gain: `out = (v·g + remainder)/100` per axis, remainder carried. Code default 250; the
   shipped right-board config sets 420 (4.2×); valid 10-1000. Optional acceleration ramps
   from 100 at low speed to the set gain at ≥ 3000 counts/s.
5. No DPI normalisation, so sensitivity depends on the mouse's DPI (upstream README bug
   added in `ab233c7`).

**Wired and dongle mice:** upstream cannot read them because the nRF52840 is USB-device only.
On PR #1's branch, the STM32 USB-host bridge sends mouse frames over UART into
`jc_bridge_mouse()` → `mouse_feed()`, the same function the BLE path uses, and
`hid_ready()` counts the wired device so the board advertises without a BLE device
[V: XD-BRIDGE `main.c:2932-2967`, `2163-2166`]. A wired mouse therefore gets the same
resampling, gain and report fields.

**What it cannot do today:** gyro (motion is replayed; see row 7 of
[Where it differs](#where-it-differs) and [protocol §7.3](joycon2-protocol.md#73-motion-imu)), and mouse on the
left Joy-Con (the left report has the same mouse field per ND-HID, but XD2Joy never fills
it, so dual-mouse games like Drag x Drive only get one pointer) [V: code; I: that the
console would honour left-side mouse data].

## What matches a genuine controller

Checked against ND-BT/ND-CAP where a genuine counterpart exists; items about XD2Joy's own
storage and behaviour are checked against the code only:

- Advertising: byte-identical layout; wake flag at 11, host address at 12; pairable,
  bonded and wake modes (`main.c:98-114`, `2127-2216`). Wake advert interval 20 ms for
  30 s after holding HOME 1 s, right board only (`main.c:4548-4601`); genuine measures
  ~21 ms.
- GATT: every UUID and every handle 0x0001-0x002A. The services are named `_0a_svc_cfg` /
  `_0b_svc_main` so Zephyr's iterable section sorts them ahead of its built-in GATT/GAP
  services, which puts the Nintendo handles at 0x0001 [I: checked against Zephyr main, not
  the pinned commit]. GATT then comes before GAP, unlike the genuine table; the console
  doesn't use those handles.
- Command framing: 17-byte prefix on 0x0016, response header `cmd 01 01 sub 10 78 00 00`
  on 0x001A (`main.c:1821-1857`).
- Pairing: 0x15 with the fixed B1, LTK = A1 ⊕ B1, AES confirm with reversed inputs; LTK
  installed into Zephyr's key store as an authenticated P-256 key so the controller answers
  the console's EDIV=0/Rand=0 encryption request; persisted under `jc_con/rec`
  (`main.c:1919-1974`, `console_keep.c`).
- Every command in the captured console init sequences gets a reply
  (`main.c:1874-2033`): 07/01, 02/04 for 0x13000/0x13040/0x13060/0x13080/0x13100/0x1FC040
  (unknown addresses return zeros), 15/*, 0C/02 and 0C/04, 09/07, 10/01, 08/01, 08/02,
  16/01, 11/01, 11/03, 01/0C, 0A/02, 0A/08. Anything else gets a 1-byte `00`.
- Colours: when the console reads 0x13000, the user's four colours are patched over
  0x13019-0x13024 (`main.c:1900-1914`).
- Report layout: counter, power, buttons, grip byte, 12-bit stick, mouse, NFC/motion
  bytes all at the genuine offsets (`main.c:4201-4438`).
- Interval: accepts the console's 5 ms via `5ms.patch` and `ull_vendor.h`. The author's
  reason: without it "the board seems to pair normally in Change Grip/Order then you back
  out and it just disconnects" [V: XD-PATCH].

## Where it differs

| # | XD2Joy | Genuine | Effect / note |
|---|---|---|---|
| 1 | **One report every 15 ms** (`REPORT_MS`), 18 ms if the link is ≥ 7.5 ms, 25 ms if ≥ 12 ms (`main.c:892-915`) | one report **every 5 ms** connection event | ~66 Hz vs 200 Hz to the console. The author says "the console only takes a report every 5ms to 15ms and speed isn't the bottleneck" [V: XD-I1]. Not measured by anyone. |
| 2 | **Asks the console for a 10 ms interval** 6 s after streaming (`asked_slower`, `main.c:4245-4255`) | never sends a parameter request; console sets 5 ms | If granted, reports drop to every 18 ms. Whether the console grants it is unknown. Candidate cause for latency and the README's "Bad Delay for some situations" bug [I]. |
| 3 | Reports skipped when 2 notifications are in flight or ≥ 3 ACL TX buffers are used (`main.c:1700-1727`, `4210-4219`) | n/a | Probably meant to keep queueing latency low; skips reports under radio stress [I]. |
| 4 | Ignores the 15/01 host list; stores only the connecting address | console sends two addresses (second = low byte −1), both stored at 0x1FA000 | If the console ever reconnects from the second address, XD2Joy refuses it [I]. Purpose of the second address unknown. |
| 5 | 03/07 and 03/09 get the default 1-byte `00` payload | bare 8-byte header | Console tolerates it [I: XD2Joy pairs per its README]. |
| 6 | Feature mask (0x0C) ignored; always sends mouse and motion fields; streaming starts on 0C/02 | console enables 0x37 and subscribes CCC 0x000F later | Notifications before the CCC write are dropped by Zephyr, so harmless [I]. |
| 7 | **Motion replayed from captures**: right replays rows 25-37 and zeroes rate fields; left freezes row 51; fake timestamp `uptime_ms·602/1000` at block+5 (`main.c:4108-4239`, `4415-4420`) | live IMU | No gyro (matches the store FAQ "Everything a real Joy-Con has (besides Gyro)" [V: XD-STORE]). Frozen accel may matter to games that read orientation [I]. |
| 8 | Left board never writes mouse bytes (the `#if JC_LEFT` branch of `send_report`, `main.c:4260-4307`, has no mouse code) | left Joy-Con 2 has a mouse sensor | No dual-mouse. |
| 9 | Every unit reports the same serials (`HBW51085343086` L, `HCW51085906458` R, grip `HDL50008895827`) | unique per device | Two XD2Joy units on one console might collide if the console tracks serials [I: untested]. |
| 10 | 10/01 reports firmware **2.1.4** (bytes `02 01 04`, `XData_*.h:70-72`, read with ND-CMD's layout) | capture shows 1.0.14 | Probably chosen to avoid the console's update prompt [I]. |
| 11 | GAP name "Joy-Con 2 (R)" on both boards (`prj.conf:3`, not overridden in `left.conf`) | per side | Console identifies by PID, so harmless [I]. |
| 12 | Writes to 0x0012, 0x0014, 0x0018 and 0x002A are dropped (`wr_stub`); `85 00` to 0x0010 is stored in a 2-byte buffer nothing reads (`main.c:2043-2049`); rumble and LED commands are ACKed and ignored | acted on | No rumble or player number anywhere. A firmware-update attempt should fail harmlessly [I: untested]. PC tools that send commands on 0x0014 (BR, NADE, per their code) would get no answer [I]. |
| 13 | Battery level 0-8, from the BLE device's Battery Service; 01/0C reply built as `[pct, 0x12, mV LE]` with mV = 3300 + pct·9 | level 0-9; `61 12 50 0d` | Console shows a plausible battery [V: works; I: field meaning]. |
| 14 | Grip byte 0x04 = `0x01` six reports after 01/0C when grip mode is on; GL bit 0x0400 | 0x07 in all free captures; no public BLE capture of a real grip | The 0x01 value and GL bit come only from XD2Joy. |
| 15 | Pairable and bonded advertising only after a keyboard/mouse is connected and has sent a report; drops the console if that device disconnects (`main.c:2122-2150`, `4802-4882`). The 30 s HOME-hold wake advert does not need this. | always available | By design (XD-I1). PR #1 makes a wired device count. |
| 16 | Responses on 0x001A | 0x001A in captures, 0x001E per ndeadly prose | Captures agree with XD2Joy [C resolved in practice]. |

## Things worth trying, ranked

Ranked by expected payoff for Jim's goals (low-latency KB/M, wired peripherals). Each is a
hypothesis until measured on hardware.

1. **Match the genuine cadence.** Make the 10 ms request optional (off by default for a
   test build) and send a report every connection event (5 ms) instead of every 15 ms.
   Expected: lower input delay and less "snapping" on slow mouse moves, since the
   resampler would emit 3× more often. Risks: more radio load on the board that is also a
   central to the mouse; the author found speed "isn't the bottleneck". Needs an A/B
   latency measurement (a photodiode or a high-speed camera on a HOME-cursor move).
2. **Use `0x0D` like the real sensor.** Write a plausible surface value (e.g. ~20) while the
   mouse is in use and `0xFF` after an idle timeout, instead of replaying captured bytes.
   This might make the HOME cursor and Metroid Prime 4's auto stick/mouse switch behave
   like a real Joy-Con being put down and picked up [I].
3. **Mouse DPI normalisation.** A per-device CPI setting so `msens` means the same thing on
   every mouse (fixes the upstream "Mouse DPI" bug).
4. **Left-board mouse** for dual-mouse games, fed from a second mouse (BLE or wired via the
   bridge) [I: needs a test that the console honours left-side mouse data].
5. **Gyro from mouse** (aim with the mouse in gyro-aiming games). Blocked on decoding the
   packed motion block; ND-CAP has paired 0x05/0x000E Pro 2 motion captures that could be
   used to decode it, and XD2Joy's own constants are a head start.
6. **Store both host addresses** from 15/01 and accept either on reconnect.
7. **Per-chip serials** derived from FICR like the BLE address, to avoid collisions between
   units.
8. **Honour the feature mask** and the player-LED command (show the player number on the
   board LED or in the config tool).
