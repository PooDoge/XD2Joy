# XD2Joy desktop tool

A desktop app for XD2Joy boards, written in Rust with [Freya](https://freyaui.dev). It does
everything the web tool does, over USB, without a browser:

- **Mapper**: both Joy-Con halves, a full keyboard and an eight-button mouse, drawn. Click a Joy-Con
  button and press a key (on the Bluetooth keyboard, on this computer, or on the drawing), or the
  other way round. Bound keys take their half's colour, held keys glow, hovering lights up what is
  connected. Right-click clears. Choose whether binding an input again moves it or adds a second
  button. Undo and redo; nothing reaches the boards until **Apply**.
- **Bluetooth**: search, pick the keyboard or mouse, see the passkey to type, forget it.
- **Profiles**: switch, save the current mapping, rename, record a shortcut, clear.
- **Shortcuts**: the mode shortcuts and the console unpair key, with conflict checks (a shortcut
  whose key also presses a Joy-Con button, shortcuts that fire each other).
- **Stick & mouse**: pointer speed, acceleration, stick-mode sensitivity, anti-deadzone and speed;
  the charging grip.
- **Colours**: body, buttons, rail and stick per half, with presets and a live preview.
- **Boards**: firmware version, mode, input source, firmware update, restore defaults.
- **Advanced**: open and save the web tool's `joycon2_preset.json`, the raw config blobs, live HID
  reports, the console, and a diagnostics file for bug reports.

## Run it

```sh
cd tool
cargo run -p xd2joy                 # with real boards
XD2JOY_DEMO=1 cargo run -p xd2joy   # with simulated boards, for working on the window
XD2JOY_DEMO=1 XD2JOY_PAGE=Colours cargo run -p xd2joy   # open on a page
XD2JOY_LOG=trace cargo run -p xd2joy   # also log every raw keyboard and mouse report
```

On Linux you need `libudev-dev` and the GL/EGL/Wayland development packages (see
`.github/workflows/tool.yml`). To use the boards without joining `dialout` (or `uucp` on Arch), and
to keep ModemManager away from them, install `packaging/70-xd2joy.rules` (instructions are in the
file).

## Logs and remembered configs

Every run writes a log file, one per day, kept for 14 days. It records every command sent to a
board and every line it printed back, every job (Apply, profile switches, updates) and how it
ended, every notice the window showed, every edit to the mapping, and panics with a backtrace.
Pairing codes are masked, in the log and in the diagnostics file; Bluetooth addresses and device
names are not.
The **Advanced** page shows the folder and opens it.

| | Linux | macOS | Windows |
|---|---|---|---|
| Logs | `~/.local/share/xd2joy/logs` | `~/Library/Application Support/xd2joy/logs` | `%LOCALAPPDATA%\xd2joy\logs` |
| Board configs | `~/.config/xd2joy/boards.json` | `~/Library/Application Support/xd2joy/boards.json` | `%APPDATA%\xd2joy\boards.json` |

The board configs file holds the last config read from each board, so a board's half of the
mapping can be edited while it is unplugged; the edits are applied when it is plugged in again.
Deleting the file only loses that.

| Variable | Effect |
|---|---|
| `XD2JOY_LOG` | Filter for both the terminal and the file, in `tracing` syntax (`debug`, `trace`, `xd2joy::host=trace`) |
| `XD2JOY_LOG_DIR` | Folder for log files |
| `XD2JOY_CACHE` | Path of the board configs file |
| `XD2JOY_DEMO` | Run with simulated boards (the board configs file is not read or written) |
| `XD2JOY_PAGE` | Open on a page by its name |

For a first run on real boards, follow [TESTING.md](TESTING.md).

## Updating firmware

1. On the **Boards** page, click **Update firmware…** on a board's card and pick its `.uf2` (`xd2joy_LEFT.uf2` for the
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

Rules borrowed from reclaw: decisions live in plain data (`model/`) with unit tests, components
only draw and produce `Effect`s, and the host thread is the only code that touches the OS.

The host sends one command at a time per board and waits for its answer, because the firmware drops
bytes that arrive while it is busy. `fake.rs` simulates both boards' firmware; the demo and the host
tests run against it.

Freya is pinned to `0.5.0-rc.8`, the same release candidate reclaw uses. Cargo would otherwise
move the inner Freya crates to any newer rc, so `tests/freya_pinned.rs` fails if `Cargo.lock`
drifts. Don't run a bare `cargo update`. To move to a new rc, update every Freya crate together
and re-check the window.
