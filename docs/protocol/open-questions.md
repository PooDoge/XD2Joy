# Open questions

Things no source we could reach answers. Each needs a sniff, a probe or an A/B test on
hardware. Keys as in [sources.md](sources.md).

## Affects XD2Joy now

1. **Does the console grant XD2Joy's 10 ms request**, and would streaming every 5 ms (like
   genuine hardware) reduce latency and mouse "snapping"? Needs a BLE sniff plus a latency
   measurement.
2. **What exactly is mouse byte 0x0D?** Surface quality vs lift distance, the threshold the
   console uses for "on a surface", and whether 0xFF is right for "not a mouse". Needs a
   capture that includes a lift event.
3. **Mouse units.** How the console's High/Medium/Low sensitivity scales deltas (console
   side, or pushed to the controller via 0C/06?), and the sensor's native CPI.
4. **The second host address** in 15/01 and 03/07 (console address, low byte −1): what it is
   for and whether the console ever connects from it.
5. **`85 00` to 0x0010**: report rate or something else?
6. **Does the console honour left-Joy-Con mouse data** (dual mouse)?
7. **Serial collisions**: does the console track controllers by the 0x13002 serial, so two
   XD2Joy units on one console clash?
8. **01/0C**: power query (XD2Joy) or NFC (ndeadly's command group)?
9. **"Only 1 of 2 boards pairs" bug**: the reconnect capture shows the console connecting twice
   within 2.6 s, and ZHANTSS reports stuck HID reports if an emulator is reconnected through
   Change Grip/Order after initial pairing. Either may be related.

## Needed for gyro

10. **Packed motion block format** in native reports (sample count, bit widths, axis order,
    timestamp unit). XD2Joy's constants (88-bit stride, rate fields at bit 154 + 89·n,
    timestamp ×602/1000 per ms) and ndeadly's paired Pro 2 motion captures are the starting
    points.
11. **Gyro scale**: ±2000 dps (ndeadly, SDL) vs "48000 = 360 dps" (joycon2cpp, JoyCon2Mac).

## Platform and hardware

12. Rail connector pinout and signalling (USB 2.0 is our inference).
13. The optical sensor IC.
14. IMU part: ICM-42607-P vs ICM-42670-P (may differ by board revision).
15. User secondary stick calibration at 0x1FC060 or 0x1FC080.
16. How soft removal and sleep are signalled over BLE (LL_TERMINATE reason?).
17. Does the console push firmware updates to controllers reporting an old version?

## For the multi-peripheral idea

18. Does the Switch 2 accept a Pro Controller 2 emulated over **USB** (057E:2069 with the
    bulk command interface)? No public emulator does this.
19. Protocol and PID of licensed wired Switch 2 accessories with a C button (HORI wheels,
    Horipad); are HORI pedals analog?
20. Do GP2040-CE's 0F0D:0092 and 057E:2009 USB modes work on Switch 2 (one unverified tweet)?
21. Can a Joy-Con 2 R identity and a Pro 2 identity from the same adapter coexist in a game?
22. Are Pro 2 headset-audio attributes required or harmful for an emulated Pro 2 (ESPP and
    JPOS disagree)?

## Sources we could not read

Reddit (including r/XD2Joy) was blocked by this session's network proxy, as were
famiboards (a Joy-Con 2 DPI thread), ResetEra and X. A pass from a normal browser over
those would be worthwhile, especially r/XD2Joy.
