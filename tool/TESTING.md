# Testing the desktop tool on real boards

The app has only run against the simulated boards in `xd2joy-app/src/fake.rs` so far. This
walks through a first run on the real thing, roughly in the order things depend on each other.
Each step says what you should see; anything else is worth a note and the log file (see
[Reporting a problem](#reporting-a-problem)).

Plan on about an hour for everything. Sections 1 to 4 are the minimum; the later ones can be done
in any order.

## 0. What you need

- Both nRF52840 boards, wired as in the main README, each with its own USB cable to the computer.
- The Bluetooth keyboard and mouse you normally use with them.
- The Switch 2, for the last checks.
- Rust 1.88 or newer (`rustup update stable`).
- On Linux: `libudev-dev` and the GL/EGL/Wayland/xkbcommon/fontconfig development packages:

  ```sh
  sudo apt-get install -y libudev-dev libegl1-mesa-dev libgl1-mesa-dev libgles2-mesa-dev \
      libwayland-dev libxkbcommon-dev libfontconfig-dev
  ```

  Then either join the `dialout` group (`uucp` on Arch; log out and in again) or install the udev
  rule, which also keeps ModemManager from poking the boards:

  ```sh
  sudo cp tool/packaging/70-xd2joy.rules /etc/udev/rules.d/
  sudo udevadm control --reload && sudo udevadm trigger
  ```

- On macOS and Windows nothing extra; the boards are standard USB serial devices.

Get the code:

```sh
git clone https://github.com/PooDoge/XD2Joy.git
cd XD2Joy
git checkout claude/project-thread-5idf6y
```

## 1. Back up what the boards hold now

The tool writes to the boards, and a bug could leave a mapping you don't want. Before anything
else, open the [web tool](https://desktopsetup.github.io/XD2Joy-Webtool/) in Chrome, press
**Connect**, and **Save preset** to a file. Also write down which profile is active and which
slots are saved. If something goes wrong, open that preset in the web tool and Apply it.

Close the web tool's tab afterwards: only one program can have a board's serial port open.

## 2. Build and try the demo

```sh
cd tool
cargo test --workspace     # should end with every "test result: ok"
XD2JOY_DEMO=1 cargo run --release -p xd2joy
```

The first build takes a few minutes. The window opens on two simulated boards. Click through every
page to check that it draws properly on your system. Nothing you do here reaches the real boards,
and demo mode does not touch the remembered-config file.

**Expected:** two green pills at the top ("Left: Joy-Con", "Right: Joy-Con"). The mapper shows two
Joy-Con halves, a keyboard and a mouse, and every page opens.

**If the window is blank or crashes on Linux:** try the other display server
(`WAYLAND_DISPLAY= cargo run …` forces X11). Note which one worked.

## 3. Flash firmware with `ver` and `bootloader`

The tool needs two console commands that the firmware on this branch adds: `ver` and
`bootloader`. Build it as in the main README ("Building"):

```sh
cd ..    # back to the repo root
ZEPHYR_WORKSPACE=/path/to/zephyrproject bash firmware/build.sh
```

Flash `firmware/out/xd2joy_LEFT.uf2` and `xd2joy_RIGHT.uf2` the usual way (RST to GND twice, then
copy the file onto the `NICENANO` drive). Do one board at a time.

To test the app's own update path for old firmware instead, skip this step and see section 9.

**Check the new commands work.** In any serial terminal (115200 baud; the baud rate does not
matter over USB), send `ver` to each board:

```
[VER] fw <version> proto 1 side L
```

The right board should say `side R`.

## 4. First connection

```sh
cd tool
cargo run --release -p xd2joy
```

Plug both boards in.

| Check | Expected |
|---|---|
| Top-bar pills | "reading…" (amber) briefly, then green with the mode ("Joy-Con", "Keyboard", …) |
| Boards page | Both cards show the firmware version from `ver` and the mode (plus the input setting, on firmware that has it) |
| Mapper | Your current bindings, not the defaults: keys you bound are coloured in their half's colour |
| Profiles page | The same active profile and saved slots you wrote down in section 1 |
| Top bar | "No unapplied changes" |

Unplug one board and plug it back in. Its pill should go grey ("not connected"), then come back.
The other board must not be affected.

**If a pill says "can't open"**, another program has the port open, or (on Linux) you lack
permission; see section 0. The log says which.

## 5. Mapper

Do these on the **Mapper** page. After each one, check the top bar says there are unapplied
changes, and that **Advanced → Config blobs** lists the change in words.

1. **Click, then press.** Click **ZL** on the left Joy-Con, then press **Q** on the Bluetooth
   keyboard. Q should take the left half's colour, and ZL should show Q.
2. **Press with this computer's keyboard.** Click **ZR**, then press **E** on the computer's own
   keyboard (with the app window focused). Same result.
3. **Click a key on the drawing.** Click **L**, then click **R** on the keyboard drawing.
4. **Modifiers.** Click **Y**, hold **Shift**, press **K**, release both: Y gets **K**, not
   Shift. Then click **X**, press and release **Shift** alone: X gets **Shift**.
5. **Mouse.** Click **ZR** on the right half, then click a side button (button 4 or 5) on the
   Bluetooth mouse. Scroll the wheel on another binding.
6. **Replace vs add.** Switch the toggle between "replaces what was there" and "adds to it", and
   bind a second key to **A**. In replace mode the old key goes, in add mode both stay.
7. **Undo/redo.** Undo every step back to the start: the top bar should return to "No unapplied
   changes". Redo one step.
8. **Live glow.** Hold keys on the Bluetooth keyboard and buttons on the mouse: they glow on the
   drawing, and bound Joy-Con buttons light up.
9. **Apply.** Press **Apply**. Expected: "Applied to the left board…" (and the right) with no error, and "No unapplied changes".
   Restart the app: your bindings are still there (they were read back from the boards).
10. **On the Switch.** With the boards paired to the console, check the new bindings press what
    they should (Controllers → Test Input Devices → Test Controller Buttons is easiest).

## 6. The other pages

**Bluetooth**
- Press **Search** on a board. Put a keyboard or mouse in pairing mode: it should appear in the
  list with its name and address.
- Choose it. If it asks for a passkey, the page shows the six digits to type on the keyboard.
- **Forget device**, then pair it again.

**Profiles**
- Save the current mapping into an empty slot, rename it, switch to another profile and back.
- **Fill from preset file…** with the file from section 1 into an empty slot. Expected: the slot fills,
  the board keeps running what it ran before (the mapper does not change).
- **Switch** is disabled while there are unapplied changes; that is intended.
- Set a profile shortcut, then press it on the Bluetooth keyboard: the active profile changes and
  the page follows.

**Shortcuts**
- Record a mode shortcut and press it on the keyboard: the mode pill changes.
- Record an unpair key. Try a key that is already bound on the mapper: it should be refused with a
  reason, and recording should continue.

**Stick & mouse**
- Drag each slider, then type a value in the box next to it and press **Enter**. Type a value out of
  range (for example 5000): an error, and nothing changes.
- Apply, then check pointer speed and stick mode on the console.

**Colours**
- Change a colour with the picker, then type a hex value (`#FF0000`) in a box and press **Enter**.
- **As on the board** puts back what the board holds; **Random** makes a new scheme.
- Apply, disconnect the Joy-Con from the Switch and reconnect: the console shows the new colours.

**Advanced**
- **Save preset…** then **Open preset…** the same file: no unapplied changes after Apply.
- **Copy** a config blob and paste it into the box below; **Load into editor** should change
  nothing.
- **Open log folder** opens your file manager on the log folder.

## 7. Editing a board while it is unplugged

1. With both boards connected and no unapplied changes, quit the app.
2. Unplug the right board and start the app again. The mapper's right half shows a banner: "isn't
   connected… as it was last read (date)".
3. Change pointer speed on **Stick & mouse** and bind a mouse button.
4. Press **Apply**: the left board is written (if it had changes) and the app says the right
   board's changes are kept.
5. Plug the right board in, then **Apply**. The changes reach it.

Bonus: in step 2, change something on the right board with the web tool first (another computer,
or quit this app). After step 5, the settings you did not touch should keep the web tool's value,
and the ones you changed should keep yours.

## 8. Closing the window

- Make a change without applying it and close the window. Expected: it stays open with "The …
  board has unapplied changes. Close the window again to quit anyway." A second close within 10
  seconds quits.
- Start a firmware update (section 9) and try to close: same warning, about the update.

## 9. Firmware update from the app

**Boards** page → **Update firmware…** on a board's card → pick its `.uf2`.

| Case | Expected |
|---|---|
| Board already runs firmware with `bootloader` | The board restarts, a `NICENANO` drive appears, the file is copied, the board comes back and the card shows the new version. No buttons to press. |
| Board runs older firmware | The button reads **Update firmware (manual reset)…**. The card asks you to short RST to GND twice on that board, and waits up to 90 seconds. |
| Wrong file (the RIGHT image on the left board) | Refused before anything happens. |
| You reset the *other* board by mistake | The app notices and copies nothing. |

Your system must mount `NICENANO` by itself. Desktop Linux, macOS and Windows do.

## 10. Restore defaults

Only once everything else is done, as it erases the mapping. **Boards** → **Restore defaults** on
one board. If a saved profile is active the app warns that the board goes back to it on restart;
that is how the firmware works. Put your mapping back with your preset from section 1 afterwards.

## Reporting a problem

Every run writes a log file, one per day. **Advanced → Log files → Open log folder** shows it.

| | Folder |
|---|---|
| Linux | `~/.local/share/xd2joy/logs` |
| macOS | `~/Library/Application Support/xd2joy/logs` |
| Windows | `%LOCALAPPDATA%\xd2joy\logs` |

For a bug, attach:
1. The newest `xd2joy.<date>.log`. It has every command sent to the boards and every line they
   printed back. Pairing codes are masked; Bluetooth addresses and device names are not.
2. **Advanced → Save diagnostics…**, saved right after the problem.
3. What you did, what you expected, and what happened instead.

For a problem with a key or mouse button that does nothing, run with raw reports logged too:

```sh
XD2JOY_LOG=trace cargo run --release -p xd2joy
```

To start from a clean slate (forget the remembered board configs), delete `boards.json` from
`~/.config/xd2joy` (Linux), `~/Library/Application Support/xd2joy` (macOS) or `%APPDATA%\xd2joy`
(Windows).

## Known unknowns

- The right half's **RS** (stick press) and **GR** masks are inferred, not confirmed. Check both
  on the console in section 5 step 10.
- The **input** setting (Auto / Bluetooth / USB) only appears on the Boards page for firmware that
  answers the `input` command.
- Whether AltGr and the Windows/Command key are reported the same way on every OS is unchecked. If
  a shortcut recorded with them loses the modifier, note your OS.
