# XD2Joy desktop tool

A desktop app for XD2Joy boards, written in Rust with [Freya](https://freyaui.dev). This first
version connects both boards over USB, shows their firmware, switches mode and input source, and
updates firmware without the RST-to-GND double tap. It doesn't replace the web tool yet: mapping,
profiles, colours and pairing are still done there.

## Run it

```sh
cd tool
cargo run -p xd2joy                 # with real boards
XD2JOY_DEMO=1 cargo run -p xd2joy   # with pretend boards, for working on the window
XD2JOY_LOG=debug cargo run -p xd2joy
```

On Linux you need `libudev-dev` and the GL/EGL/Wayland development packages (see
`.github/workflows/tool.yml`). To use the boards without joining `dialout` (or `uucp` on Arch), and
to keep ModemManager away from them, install `packaging/70-xd2joy.rules` (instructions are in the
file).

## Updating firmware

1. Click **Update firmware…** on a board's card and pick its `.uf2` (`xd2joy_LEFT.uf2` for the
   left board, `xd2joy_RIGHT.uf2` for the right).
2. The app checks the file is a whole UF2 image for the nRF52840. Then it sends the board the
   `bootloader` command, waits for the new `NICENANO` drive, copies the file onto it, and waits for
   the board to come back.

Boards running firmware from before the `bootloader` command can't restart themselves. The card
says so, and the app waits up to 90 seconds while you short RST to GND twice on that board. You only
need to do this once. If the *other* board restarts instead, the app notices and copies nothing.

Your system has to mount the `NICENANO` drive by itself. Desktop Linux, macOS and Windows all do
this; a headless Linux box may not.

## Layout

| Crate | Job | Freya |
|---|---|---|
| `xd2joy-proto` | Board ids, commands, parsing `[TAG] ...` reply lines | no |
| `xd2joy-device` | Finding boards on USB serial, a reader thread per board | no |
| `xd2joy-flash` | UF2 checks, finding bootloader drives, copying | no |
| `xd2joy` (`xd2joy-app/`) | The host thread, the model, the window | yes |

Rules borrowed from reclaw: decisions live in plain data (`model.rs`) with unit tests, components
only draw and produce `Effect`s, and the host thread is the only code that touches the OS.

Freya is pinned to `0.5.0-rc.8`, the same release candidate reclaw uses. Cargo would otherwise
move the inner Freya crates to any newer rc, so `tests/freya_pinned.rs` fails if `Cargo.lock`
drifts. Don't run a bare `cargo update`. To move to a new rc, update every Freya crate together
and re-check the window.
