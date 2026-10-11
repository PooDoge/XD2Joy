# Switch 2 controller protocol notes

What is publicly known about how Joy-Con 2 (and the other Switch 2 controllers) talk to the
Switch 2 console, and how XD2Joy implements it. Compiled 2026-10-11 by a parallel research
pass over this firmware, the original author's repos and store, ndeadly's protocol research
and decrypted captures, SDL/Linux/BlueRetro and other host drivers, three device-side
emulators, teardowns and Nintendo's support pages.

This is interoperability documentation for owners modifying their own hardware. It contains
no Nintendo code or firmware.

## Short answer: mouse passthrough

The right board already presents a connected mouse to the Switch 2 as a Joy-Con 2 (R) in
mouse mode: the author documents it as a working feature, and the code matches the genuine
report layout (we have not hardware-tested it ourselves). It writes per-report s16 ΔX/ΔY
into report bytes 0x09-0x0C and the surface byte 0x0D, which are the same fields a genuine Joy-Con 2's optical sensor fills
(`firmware/src/main.c:4257`, `4387-4413`). With the wired USB bridge (PR #1), a USB or
dongle mouse uses the same path. Details and limits:
[xd2joy-vs-genuine.md](xd2joy-vs-genuine.md#does-it-already-pass-a-mouse-through-as-a-joy-con-2-in-mouse-mode).

## Files

| File | What it covers |
|---|---|
| [joycon2-protocol.md](joycon2-protocol.md) | The protocol: transport, advertising, GATT, pairing, commands, init sequence, flash map, input/output reports, mouse, IMU, USB/rail, OTA, hardware |
| [xd2joy-vs-genuine.md](xd2joy-vs-genuine.md) | How this firmware implements each part, every known difference, and ranked things to try |
| [multi-peripheral.md](multi-peripheral.md) | What the protocol allows for joysticks, wheels and other peripherals |
| [open-questions.md](open-questions.md) | What nobody has published yet, and how to find out |
| [sources.md](sources.md) | Every source with its key and licence, plus what we couldn't reach |
| [evidence/claims.md](evidence/claims.md) | All 335 raw research claims with sources, for tracing any statement |

## How to read the tags

- **[V]** verified: a source we read states or implements it, or we decoded it from a capture.
- **[I]** inferred by us.
- **[U]** unverified: only second-hand.
- **[C]** sources conflict; both sides are given.

## The main source

Almost everything traces back to
[ndeadly/switch2_controller_research](https://github.com/ndeadly/switch2_controller_research).
It has no licence file, so we cite its facts and never copy its text, code or captures.
Where its prose disagrees with its own captures (for example, which handle carries command
responses), these notes follow the captures and say so.
