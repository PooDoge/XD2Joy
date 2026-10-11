//! Every page but the mapper: Bluetooth devices, profiles, shortcuts, stick and mouse tuning,
//! colours, the boards themselves, and the advanced tools.

use std::fmt::Write as _;

use freya::prelude::*;
use xd2joy_proto::{
    ConfigBlob, DeviceKind, HidState, InputSource, Mode, PROFILE_NAME_MAX, Rgb, ScanDevice, Side, Slot,
    blob::Colours,
    mapping::{MOUSE_GAIN, STICK_DEADZONE, STICK_SENS, STICK_SPEED, Tuning},
    preset,
};

use super::mapper::{self, Looks};
use super::{danger_button, muted, panel, row};
use crate::{
    host::HostHandle,
    model::{
        BoardView, Effect, Firmware, FlashStage, HotkeyFor, Link, Model, PairState, conflicts, pairing_tip, side_word,
    },
    theme::*,
};

fn side_title(side: Side) -> impl IntoElement {
    row()
        .child(rect().width(Size::px(10.)).height(Size::px(10.)).corner_radius(5.).background(match side {
            Side::Left => LEFT,
            Side::Right => RIGHT,
        }))
        .child(label().text(side.label()).font_size(HEADING).font_weight(FontWeight::BOLD))
}

/// A card per board, side by side.
fn two_cards(left: impl IntoElement, right: impl IntoElement) -> Rect {
    rect().horizontal().width(Size::fill()).content(Content::flex()).spacing(GAP).child(left).child(right)
}

fn card() -> Rect {
    rect()
        .width(Size::flex(1.))
        .background(PANEL)
        .corner_radius(RADIUS)
        .border(Border::new().fill(BORDER).width(1.))
        .padding(Gaps::new_all(PAD))
        .spacing(10.)
}

fn not_connected(b: &BoardView) -> Option<Label> {
    (!b.connected()).then(|| muted("Not connected. Plug the board in with a USB data cable."))
}

// ---------------------------------------------------------------------------------------------
// Bluetooth

pub fn devices(model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(panel("Bluetooth keyboard and mouse").child(muted(
            "The left board takes a keyboard and the right board a mouse. Search, then pick yours; the board \
             remembers it and reconnects by itself. A keyboard may ask you to type a code shown here.",
        )))
        .child(two_cards(device_card(model, m, Side::Left, host), device_card(model, m, Side::Right, host)))
}

fn pair_line(p: &PairState) -> Option<(String, Color)> {
    Some(match p {
        PairState::Unknown | PairState::Idle => return None,
        PairState::Connecting => ("Connecting…".into(), WARN),
        PairState::Passkey(code) => (format!("Type {code:06} on the keyboard, then press Enter."), ACCENT),
        PairState::Connected => ("Connected, setting up…".into(), WARN),
        PairState::Ready => ("Ready".into(), OK),
        PairState::Failed(why) => (format!("Couldn't connect: {why}. {}", pairing_tip(why)), DANGER),
    })
}

fn kind_word(k: DeviceKind) -> &'static str {
    match k {
        DeviceKind::Keyboard => "keyboard",
        DeviceKind::Mouse => "mouse",
        DeviceKind::OtherInput => "input device",
        DeviceKind::Other => "other",
    }
}

fn device_card(model: State<Model>, m: &Model, side: Side, host: &HostHandle) -> impl IntoElement {
    let b = m.board(side);
    let busy = m.busy();
    let ok = b.writable() && !busy;
    let scan_host = host.clone();
    let (pinned, attached) = (b.pin.clone().flatten(), b.attached.clone().flatten());
    let pin_text = match (&b.pin, &pinned) {
        (None, _) => None,
        (Some(_), None) => Some("No device chosen yet.".to_owned()),
        (Some(_), Some((addr, name))) => {
            Some(format!("Chosen: {} ({})", if name.is_empty() { "unnamed" } else { name }, addr.hex()))
        }
    };
    let link_text = match (&attached, b.hid) {
        (Some((_, name, _)), Some(HidState::Subscribed)) => {
            Some((format!("{name} is connected and sending input."), OK))
        }
        (Some((_, name, _)), _) => Some((format!("{name} is connected."), OK)),
        (None, _) if pinned.is_some() => {
            Some(("Waiting for it. Wake it up or put it in pairing mode.".to_owned(), INK_MUTED))
        }
        _ => None,
    };
    let mut heard: Vec<ScanDevice> = b.heard.clone();
    // the right kind first, then the strongest signal
    let want = if side == Side::Left { DeviceKind::Keyboard } else { DeviceKind::Mouse };
    heard.sort_by_key(|d| (d.kind() != want, !d.hid, -d.rssi));
    card()
        .child(side_title(side))
        .maybe_child(not_connected(b))
        .maybe_child(pin_text.map(label_of))
        .maybe_child(link_text.map(|(t, c)| label().text(t).color(c)))
        .maybe_child(pair_line(&b.pairing).map(|(t, c)| label().text(t).color(c)))
        .child(
            row()
                .child(
                    Button::new()
                        .enabled(ok && !b.scanning)
                        .on_press(move |_| scan_host.send(Effect::Scan(side)))
                        .child(if b.scanning { "Searching…" } else { "Search" }),
                )
                .child(danger_button(
                    model,
                    m,
                    format!("unpin {}", side.letter()),
                    "Forget device",
                    ok && pinned.is_some(),
                    Effect::Unpin(side),
                    host,
                )),
        )
        .maybe_child(b.scanning.then(|| muted("Put the device in pairing mode now. Searching for 20 seconds.")))
        .children(
            heard.into_iter().enumerate().map(|(i, d)| heard_row(m, side, d, want, ok, host).key(i).into_element()),
        )
}

fn label_of(t: String) -> Label {
    label().text(t)
}

fn heard_row(m: &Model, side: Side, d: ScanDevice, want: DeviceKind, ok: bool, host: &HostHandle) -> Rect {
    let host = host.clone();
    let kind = d.kind();
    let wrong = matches!((kind, side), (DeviceKind::Mouse, Side::Left) | (DeviceKind::Keyboard, Side::Right));
    let is_pinned = m.board(side).pin.as_ref().and_then(|p| p.as_ref()).is_some_and(|(a, _)| a.bytes == d.addr.bytes);
    let name = if d.name.is_empty() { "Unnamed device".to_owned() } else { d.name.clone() };
    let (addr, pin_name) = (d.addr, d.name.clone());
    row()
        .width(Size::fill())
        .padding(Gaps::new(4., 8., 4., 8.))
        .corner_radius(6.)
        .background(if kind == want { SELECT.with_a(90) } else { Color::TRANSPARENT })
        .child(rect().width(Size::flex(1.)).child(label().text(name)).child(muted(format!(
            "{} · {} dBm · {}{}",
            kind_word(kind),
            d.rssi,
            d.addr.hex(),
            if wrong { " · belongs on the other board" } else { "" }
        ))))
        .child(
            Button::new()
                .enabled(ok && !is_pinned)
                .on_press(move |_| host.send(Effect::Pin { side, addr, name: pin_name.clone() }))
                .child(if is_pinned { "Chosen" } else { "Use this" }),
        )
}

// ---------------------------------------------------------------------------------------------
// Profiles

pub fn profiles(model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    // the boards keep their profile tables in step over the link; the left one is shown
    let side = if m.board(Side::Left).profiles.active.is_some() { Side::Left } else { Side::Right };
    let profiles = &m.board(side).profiles;
    let any_board = Side::BOTH.iter().any(|&s| m.board(s).writable());
    let dirty = m.editor.any_dirty();
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(
            panel("Profiles")
                .child(muted(
                    "Each profile holds both boards' mappings, settings and colours. Switch with its shortcut on \
                     the keyboard, or here. The active profile is the one the boards start with.",
                ))
                .maybe_child(
                    dirty
                        .then(|| label().text("Apply or discard your changes before saving to a profile.").color(WARN)),
                )
                .maybe_child((!any_board).then(|| muted("Connect a board to see its profiles."))),
        )
        .children(Slot::ALL.map(|slot| {
            let info = profiles.slot(slot.number()).cloned();
            let active = profiles.active == Some(slot.number());
            profile_row(model, m, slot, info, active, any_board, dirty, host).key(slot.number()).into_element()
        }))
}

#[allow(clippy::too_many_arguments)]
fn profile_row(
    mut model: State<Model>,
    m: &Model,
    slot: Slot,
    info: Option<xd2joy_proto::ProfileSlot>,
    active: bool,
    any_board: bool,
    dirty: bool,
    host: &HostHandle,
) -> Rect {
    let n = slot.number();
    let i = usize::from(n - 1);
    let saved = info.as_ref().is_some_and(|s| s.saved);
    let name =
        info.as_ref().map(|s| s.name.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Profile {n}"));
    let draft = m.drafts.profile_names[i].trim().to_owned();
    let draft: String = draft.chars().take(PROFILE_NAME_MAX).collect();
    let ok = any_board && !m.busy();
    let key = info.as_ref().and_then(|s| s.key);
    let recording = m.hotkey_capture == Some(HotkeyFor::Profile(slot));
    let name_field =
        Writable::from_state(model).map(move |m| &m.drafts.profile_names[i], move |m| &mut m.drafts.profile_names[i]);
    let (switch_host, save_host, rename_host, key_host) = (host.clone(), host.clone(), host.clone(), host.clone());
    let save_name = if draft.is_empty() { name.clone() } else { draft.clone() };
    let mut panel = panel(&format!("{n}. {name}{}", if active { "  (active)" } else { "" }));
    panel = panel
        .child(muted(match (saved, key) {
            (false, _) => "Empty".to_owned(),
            (true, Some(k)) => format!("Saved · shortcut {}", k.label()),
            (true, None) => "Saved · no shortcut".to_owned(),
        }))
        .child(
            row()
                .child(
                    Button::new()
                        .enabled(ok && saved && !active)
                        .on_press(move |_| switch_host.send(Effect::ProfileSwitch(slot)))
                        .child("Switch to it"),
                )
                .child(
                    Button::new()
                        .enabled(ok && !dirty)
                        .on_press(move |_| save_host.send(Effect::ProfileSave { slot, name: save_name.clone() }))
                        .child(if saved { "Overwrite with current" } else { "Save current here" }),
                )
                .child(Input::new(name_field).placeholder("New name").width(Size::px(180.)))
                .child(
                    Button::new()
                        .enabled(ok && saved && !draft.is_empty())
                        .on_press(move |_| {
                            rename_host.send(Effect::ProfileRename { slot, name: draft.clone() });
                            model.write().drafts.profile_names[i].clear();
                        })
                        .child("Rename"),
                ),
        )
        .child(
            row()
                .child(
                    Button::new()
                        .enabled(ok && saved)
                        .on_press(move |_| {
                            let mut m = model.write();
                            m.hotkey_capture = if recording { None } else { Some(HotkeyFor::Profile(slot)) };
                        })
                        .child(if recording { "Press the shortcut… (Esc cancels)" } else { "Record shortcut" }),
                )
                .child(
                    Button::new()
                        .enabled(ok && key.is_some())
                        .on_press(move |_| key_host.send(Effect::ProfileKey { slot, key: None }))
                        .child("Remove shortcut"),
                )
                .child(danger_button(
                    model,
                    m,
                    format!("clear profile {n}"),
                    "Clear",
                    ok && saved,
                    Effect::ProfileClear(slot),
                    host,
                )),
        );
    panel
}

// ---------------------------------------------------------------------------------------------
// Shortcuts

pub fn hotkeys(mut model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    let named = m.hotkeys();
    let sync = m.sync_key();
    let problems = conflicts(&m.editor.edit, &named, sync);
    let ok = Side::BOTH.iter().any(|&s| m.board(s).writable()) && !m.busy();
    let sync_known = Side::BOTH.iter().any(|&s| m.board(s).sync.is_some());
    let recording_unpair = m.hotkey_capture == Some(HotkeyFor::Unpair);
    let (off_host,) = (host.clone(),);
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .maybe_child(m.hotkey_capture.is_some().then(|| {
            panel("Recording").child(
                label()
                    .text("Press the shortcut on the Bluetooth keyboard or on this computer's keyboard. Esc cancels.")
                    .color(ACCENT),
            )
        }))
        .child(
            panel("Mode shortcuts (left board)")
                .child(muted(
                    "Pressing a mode shortcut switches to that mode; pressing it again goes back to Joy-Con. They \
                     are saved with Apply.",
                ))
                .children(Mode::ALL.into_iter().filter(|&mode| mode != Mode::Joycon).map(|mode| {
                    let key = m.editor.edit.mode_key(mode);
                    let recording = m.hotkey_capture == Some(HotkeyFor::Mode(mode));
                    row()
                        .key(mode.label())
                        .child(label().text(format!("{} mode", mode.label())).width(Size::px(140.)))
                        .child(label().text(key.map_or("none".to_owned(), |k| k.label())).width(Size::px(160.)))
                        .child(
                            Button::new()
                                .on_press(move |_| {
                                    let mut m = model.write();
                                    m.hotkey_capture = if recording { None } else { Some(HotkeyFor::Mode(mode)) };
                                })
                                .child(if recording { "Press it…" } else { "Record" }),
                        )
                        .child(
                            Button::new()
                                .enabled(key.is_some())
                                .on_press(move |_| model.write().editor.set_mode_key(mode, None))
                                .child("Remove"),
                        )
                        .into_element()
                })),
        )
        .child(
            panel("Console unpair key")
                .child(muted(
                    "Holding this key for 2 seconds makes the Joy-Con forget the console, so it can pair with \
                     another one. One key, no modifiers. It is saved on the boards at once.",
                ))
                .child(
                    row()
                        .child(
                            label()
                                .text(match (sync_known, sync) {
                                    (false, _) => "unknown".to_owned(),
                                    (true, None) => "off".to_owned(),
                                    (true, Some(k)) => k.label(),
                                })
                                .width(Size::px(160.)),
                        )
                        .child(
                            Button::new()
                                .enabled(ok)
                                .on_press(move |_| {
                                    let mut m = model.write();
                                    m.hotkey_capture = if recording_unpair { None } else { Some(HotkeyFor::Unpair) };
                                })
                                .child(if recording_unpair { "Press it…" } else { "Record" }),
                        )
                        .child(
                            Button::new()
                                .enabled(ok && sync.is_some())
                                .on_press(move |_| off_host.send(Effect::SyncKey(None)))
                                .child("Turn off"),
                        )
                        .child(danger_button(
                            model,
                            m,
                            "sync now".into(),
                            "Unpair from console now",
                            ok,
                            Effect::SyncNow,
                            host,
                        )),
                ),
        )
        .child(
            panel("Conflicts")
                .maybe_child(problems.is_empty().then(|| label().text("None found.").color(OK)))
                .children(
                    problems.into_iter().enumerate().map(|(i, p)| label().key(i).text(p).color(WARN).into_element()),
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// Stick and mouse

fn percent(v: u16, r: &std::ops::RangeInclusive<u16>) -> f64 {
    f64::from(v - r.start()) * 100. / f64::from(r.end() - r.start())
}

fn from_percent(p: f64, r: &std::ops::RangeInclusive<u16>) -> u16 {
    let span = f64::from(r.end() - r.start());
    // the slider reports 0..=100; anything else is clamped to the range
    (f64::from(*r.start()) + (p.clamp(0., 100.) / 100. * span).round())
        .clamp(f64::from(*r.start()), f64::from(*r.end())) as u16
}

fn slider_row(
    mut model: State<Model>,
    name: &'static str,
    help: &'static str,
    value: u16,
    shown: String,
    range: std::ops::RangeInclusive<u16>,
    set: fn(&mut Tuning, u16),
) -> impl IntoElement {
    let step = 100. / f64::from(range.end() - range.start());
    let r = range.clone();
    rect()
        .width(Size::fill())
        .spacing(4.)
        .child(row().child(label().text(name).width(Size::px(200.))).child(label().text(shown).color(ACCENT)))
        .child(
            Slider::new(move |p: f64| {
                let v = from_percent(p, &r);
                model.write().editor.tweak(name, |map| set(&mut map.tuning, v));
            })
            .value(percent(value, &range))
            .step(step)
            .size(Size::px(420.)),
        )
        .child(muted(help))
}

pub fn tuning(mut model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    let t = m.editor.edit.tuning;
    let right = m.board(Side::Right);
    let grip = Side::BOTH.iter().find_map(|&s| m.board(s).grip);
    let ok = Side::BOTH.iter().any(|&s| m.board(s).writable()) && !m.busy();
    let grip_host = host.clone();
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(
            panel("Mouse (Joy-Con mouse mode)")
                .child(slider_row(
                    model,
                    "Pointer speed",
                    "How far the Switch pointer moves for a mouse movement.",
                    t.mouse_gain,
                    format!("{}%", t.mouse_gain),
                    MOUSE_GAIN,
                    |t, v| t.mouse_gain = v,
                ))
                .child(
                    row()
                        .child(
                            Switch::new()
                                .toggled(t.accel)
                                .on_toggle(move |_| model.write().editor.tweak("accel", |map| map.tuning.accel = !map.tuning.accel)),
                        )
                        .child(label().text("Acceleration: fast movements go further")),
                ),
        )
        .child(
            panel("Stick mode (the mouse moves the right stick)")
                .child(slider_row(
                    model,
                    "Sensitivity",
                    "How much stick a mouse movement gives. Stops getting stronger above 10.",
                    t.stick_sens,
                    t.stick_sens.to_string(),
                    STICK_SENS,
                    |t, v| t.stick_sens = v,
                ))
                .child(slider_row(
                    model,
                    "Anti-deadzone",
                    "Skips the stick's dead centre so small movements register in games. The web tool shows a tenth of this.",
                    t.stick_deadzone,
                    format!("{} ({:.1})", t.stick_deadzone, f64::from(t.stick_deadzone) / 10.),
                    STICK_DEADZONE,
                    |t, v| t.stick_deadzone = v,
                ))
                .child(slider_row(
                    model,
                    "Return speed",
                    "How fast the stick follows the mouse and settles back. The web tool stops at 20.",
                    t.stick_speed,
                    t.stick_speed.to_string(),
                    STICK_SPEED,
                    |t, v| t.stick_speed = v,
                ))
                .child(
                    Button::new()
                        .on_press(move |_| {
                            let _ = model.write().editor.change(|map| {
                                map.tuning = Tuning::DEFAULT;
                                Ok::<(), ()>(())
                            });
                        })
                        .child("Back to defaults"),
                )
                .maybe_child(right.live_tuning.map(|l| {
                    muted(format!(
                        "Running on the right board now: sensitivity {}, pointer {}%, anti-deadzone {}, speed {}.",
                        l.stick_sens, l.mouse_gain, l.stick_deadzone, l.stick_speed
                    ))
                })),
        )
        .child(
            panel("Charging grip")
                .child(muted(
                    "Tell the console the Joy-Con are in a grip, so it pairs them as one controller. Saved on the \
                     boards at once.",
                ))
                .child(
                    row()
                        .child(
                            Switch::new()
                                .toggled(grip.unwrap_or(false))
                                .enabled(ok && grip.is_some())
                                .on_toggle(move |_| grip_host.send(Effect::Grip(!grip.unwrap_or(false)))),
                        )
                        .child(label().text(match grip {
                            None => "unknown",
                            Some(true) => "In a grip",
                            Some(false) => "Separate",
                        })),
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// Colours

fn to_color(c: Rgb) -> Color {
    Color::from_rgb(c.0[0], c.0[1], c.0[2])
}

fn from_color(c: Color) -> Rgb {
    Rgb([c.r(), c.g(), c.b()])
}

/// Colour sets people ask for, as (name, body, buttons).
const PRESETS: [(&str, [u8; 3], [u8; 3]); 6] = [
    ("Neon blue", [0x00, 0xAE, 0xE3], [0x1E, 0x1E, 0x1E]),
    ("Neon red", [0xFF, 0x3E, 0x46], [0x1E, 0x1E, 0x1E]),
    ("Grey", [0x82, 0x82, 0x82], [0x0F, 0x0F, 0x0F]),
    ("Black", [0x31, 0x31, 0x31], [0x0F, 0x0F, 0x0F]),
    ("Neon yellow", [0xE6, 0xFF, 0x00], [0x14, 0x28, 0x00]),
    ("Neon green", [0x1E, 0xDC, 0x00], [0x00, 0x28, 0x00]),
];

pub fn colours(model: State<Model>, m: &Model) -> impl IntoElement {
    let looks = Looks::of(m);
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(panel("Joy-Con colours").child(muted(
            "The console shows these colours for each Joy-Con, in menus and in games. They are saved with Apply. \
             The console reads them when it pairs, so re-pair to see a change.",
        )))
        .child(two_cards(colour_card(model, m, Side::Left, &looks), colour_card(model, m, Side::Right, &looks)))
}

fn colour_card(mut model: State<Model>, m: &Model, half: Side, looks: &Looks) -> impl IntoElement {
    let c = m.editor.edit.colours[half.index()];
    let i = half.index();
    let part = move |name: &'static str, value: Rgb, set: fn(&mut Colours, Rgb)| {
        let what: &'static str = match (half, name) {
            (Side::Left, "Body") => "colour L body",
            (Side::Left, "Buttons") => "colour L buttons",
            (Side::Left, "Rail") => "colour L rail",
            (Side::Left, _) => "colour L stick",
            (Side::Right, "Body") => "colour R body",
            (Side::Right, "Buttons") => "colour R buttons",
            (Side::Right, "Rail") => "colour R rail",
            (Side::Right, _) => "colour R stick",
        };
        row()
            .key(name)
            .child(label().text(name).width(Size::px(70.)))
            .child(label().text(value.to_string()).width(Size::px(80.)).font_family("monospace"))
            .child(
                ColorPicker::new(move |col: Color| {
                    model.write().editor.tweak(what, |map| set(&mut map.colours[i], from_color(col)));
                })
                .value(to_color(value)),
            )
            .into_element()
    };
    let set_all = move |colours: Colours| {
        move |_: Event<PressEventData>| {
            let _ = model.write().editor.change(|map| {
                map.colours[i] = colours;
                Ok::<(), ()>(())
            });
        }
    };
    card()
        .child(side_title(half))
        .child(
            row().child(mapper::joycon(model, m, half, looks)).child(
                rect()
                    .spacing(8.)
                    .child(part("Body", c.body, |c, v| c.body = v))
                    .child(part("Buttons", c.buttons, |c, v| c.buttons = v))
                    .child(part("Rail", c.rail, |c, v| c.rail = v))
                    .child(part("Stick", c.stick, |c, v| c.stick = v)),
            ),
        )
        .child(row().child(Button::new().on_press(set_all(Colours::stock(half))).child("Stock")).children(
            PRESETS.iter().map(|&(name, body, buttons)| {
                let mut colours = c;
                colours.body = Rgb(body);
                colours.buttons = Rgb(buttons);
                colours.rail = Rgb(body);
                Button::new().key(name).on_press(set_all(colours)).child(name).into_element()
            }),
        ))
}

// ---------------------------------------------------------------------------------------------
// Boards

pub fn boards(model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    let versions: Vec<&str> = Side::BOTH
        .iter()
        .filter_map(|&s| match &m.board(s).firmware {
            Firmware::Known(info) => Some(info.version.as_str()),
            _ => None,
        })
        .collect();
    let mismatch = versions.len() == 2 && versions[0] != versions[1];
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .maybe_child(m.flash.as_ref().map(flash_line))
        .maybe_child(mismatch.then(|| {
            panel("Firmware differs").child(
                label()
                    .text(format!(
                        "The left board runs {} and the right board {}. Update both from the same release; the boards \
                         talk to each other and may not agree otherwise.",
                        versions[0], versions[1]
                    ))
                    .color(WARN),
            )
        }))
        .child(two_cards(board_card(model, m, Side::Left, host), board_card(model, m, Side::Right, host)))
}

fn flash_line(stage: &FlashStage) -> impl IntoElement {
    let color = match stage {
        FlashStage::Done { .. } => OK,
        FlashStage::Failed { .. } => DANGER,
        FlashStage::NeedsManualReset { .. } => WARN,
        _ => INK,
    };
    panel("Firmware update").child(label().text(format!("{}: {}", stage.side().label(), stage.describe())).color(color))
}

fn board_card(model: State<Model>, m: &Model, side: Side, host: &HostHandle) -> impl IntoElement {
    let v = m.board(side);
    let connected = v.connected();
    let busy = m.busy();
    let writable = v.writable() && !busy;
    let (status, status_color) = match &v.link {
        Link::Absent => ("Not connected".to_owned(), INK_MUTED),
        Link::Connected { port } => (format!("Connected on {port}"), OK),
        Link::Failed { port, error } => (format!("Could not open {port}: {error}"), DANGER),
    };
    let firmware = match &v.firmware {
        _ if !connected => None,
        Firmware::Unknown => Some(("Checking firmware…".to_owned(), INK_MUTED)),
        Firmware::Old => Some((
            "Older firmware. Update it once by hand (RST to GND twice); after that this app can do it.".to_owned(),
            WARN,
        )),
        Firmware::Known(info) if v.firmware.newer_than_tool() => Some((
            format!(
                "Firmware {} is newer than this app. Settings are read-only until the app is updated.",
                info.version
            ),
            WARN,
        )),
        Firmware::Known(info) => Some((format!("Firmware {}", info.version), INK_MUTED)),
    };
    let modes = SegmentedButton::new().children(Mode::ALL.map(|mode| {
        let host = host.clone();
        ButtonSegment::new()
            .key(mode.label())
            .selected(v.mode == Some(mode))
            .enabled(writable)
            .on_press(move |_| host.send(Effect::SetMode(side, mode)))
            .child(mode.label())
            .into_element()
    }));
    let inputs = v.input.map(|status| {
        rect()
            .spacing(6.)
            .child(label().text("Input").color(INK_MUTED))
            .child(SegmentedButton::new().children(InputSource::ALL.map(|source| {
                let host = host.clone();
                ButtonSegment::new()
                    .key(source.label())
                    .selected(status.source == source)
                    .enabled(writable)
                    .on_press(move |_| host.send(Effect::SetInput(side, source)))
                    .child(source.label())
                    .into_element()
            })))
            .child(muted(if status.wired_active {
                format!("A wired device is in use ({} on the USB bridge).", status.bridge_devices)
            } else if status.frames > 0 {
                format!("USB bridge seen: {} frames, {} bad. No wired device in use.", status.frames, status.bad_frames)
            } else {
                "No wired device in use.".to_owned()
            }))
    });
    let update_host = host.clone();
    let reload_host = host.clone();
    let update = Button::new()
        .enabled(!busy)
        .on_press(move |_| {
            let host = update_host.clone();
            spawn_forever(async move {
                let picked = rfd::AsyncFileDialog::new()
                    .set_title(format!("Firmware for the {} board", side_word(side)))
                    .add_filter("UF2 firmware", &["uf2", "UF2"])
                    .pick_file()
                    .await;
                if let Some(file) = picked {
                    host.send(Effect::Flash { side, file: file.path().to_path_buf() });
                }
            });
        })
        .child(if v.firmware.can_restart_itself() || !connected {
            "Update firmware…"
        } else {
            "Update firmware (manual reset)…"
        });
    card()
        .child(side_title(side))
        .child(label().text(status).color(status_color))
        .maybe_child(firmware.map(|(text, color)| label().text(text).color(color)))
        .maybe_child(v.warning.clone().map(|w| label().text(w).color(WARN)))
        .maybe_child(connected.then(|| rect().spacing(6.).child(label().text("Mode").color(INK_MUTED)).child(modes)))
        .maybe_child(inputs)
        .child(
            row()
                .child(update)
                .child(
                    Button::new()
                        .enabled(connected && !busy)
                        .on_press(move |_| reload_host.send(Effect::Reload(side)))
                        .child("Read again"),
                )
                .child(danger_button(
                    model,
                    m,
                    format!("restore {}", side.letter()),
                    "Restore defaults",
                    writable,
                    Effect::RestoreDefaults(side),
                    host,
                )),
        )
        .child(muted(
            "Restore defaults puts back the built-in mapping and forgets the console pairing and the chosen device.",
        ))
}

// ---------------------------------------------------------------------------------------------
// Advanced

pub fn advanced(model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    let json = preset::to_json(&m.editor.edit, m.sync_key());
    let sync_host = host.clone();
    let hex_field = Writable::from_state(model).map(|m| &m.drafts.blob_hex, |m| &mut m.drafts.blob_hex);
    let blobs: Vec<(Side, Result<String, String>)> = Side::BOTH
        .map(|s| {
            (s, m.editor.edit.to_blob(s).map_err(|e| e.to_string()).and_then(|b| b.to_hex().map_err(|e| e.to_string())))
        })
        .into();
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(
            panel("Preset file")
                .child(muted(format!(
                    "The same {} file the web tool saves and opens. Opening one replaces the mapping being edited; \
                     Apply sends it to the boards.",
                    preset::FILE_NAME
                )))
                .child(
                    row()
                        .child(Button::new().on_press(move |_| save_preset(model, json.clone())).child("Save preset…"))
                        .child(Button::new().on_press(move |_| open_preset(model, sync_host.clone())).child("Open preset…")),
                ),
        )
        .child(
            panel("Config blobs")
                .child(muted("What Apply would write, as the hex `cfg set` takes. Paste a blob below to load it into the editor."))
                .children(blobs.into_iter().map(|(s, hex)| {
                    rect()
                        .key(s.letter().to_string())
                        .spacing(2.)
                        .child(muted(s.label()))
                        .child(match hex {
                            Ok(h) => label().text(h).font_family("monospace").font_size(TEXT_SMALL),
                            Err(e) => label().text(e).color(DANGER),
                        })
                        .into_element()
                }))
                .child(
                    row()
                        .child(Input::new(hex_field).placeholder("Blob hex").width(Size::px(420.)))
                        .child(Button::new().on_press(move |_| load_blob(model)).child("Load into editor")),
                ),
        )
        .child(raw_panel(m))
        .child(
            panel("Console")
                .child(
                    row().child(Button::new().on_press(move |_| save_diagnostics(model)).child("Save diagnostics…")).child(muted(
                        "Saves this log and what the app knows about both boards, for a bug report.",
                    )),
                )
                .child(
                    rect().width(Size::fill()).spacing(2.).children(
                        m.log
                            .iter()
                            .rev()
                            .take(120)
                            .enumerate()
                            .map(|(i, line)| label().key(i).text(line.clone()).font_size(TEXT_SMALL).font_family("monospace").into_element()),
                    ),
                ),
        )
}

fn raw_panel(m: &Model) -> impl IntoElement {
    panel("Live reports")
        .child(muted(
            "The last report of each kind from the Bluetooth devices, as the boards receive them. Bytes that ever \
             changed are highlighted. Useful when a key or button does nothing.",
        ))
        .maybe_child(m.raw.is_empty().then(|| muted("Nothing yet. Press something on the keyboard or mouse.")))
        .children(m.raw.iter().enumerate().map(|(i, r)| {
            row()
                .key(i)
                .child(
                    label()
                        .text(format!("{} h=0x{:04X} len={}", r.side.letter(), r.handle, r.len))
                        .width(Size::px(150.))
                        .font_family("monospace"),
                )
                .children(r.bytes.iter().zip(&r.changed).enumerate().map(|(j, (b, c))| {
                    label()
                        .key(j)
                        .text(format!("{b:02X}"))
                        .font_family("monospace")
                        .color(if *c { GLOW } else { INK_MUTED })
                        .into_element()
                }))
                .into_element()
        }))
}

fn save_preset(mut model: State<Model>, json: String) {
    spawn_forever(async move {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Save preset")
            .set_file_name(preset::FILE_NAME)
            .add_filter("Preset", &["json"])
            .save_file()
            .await
        else {
            return;
        };
        match std::fs::write(file.path(), json) {
            Ok(()) => model.write().info(format!("Saved {}", file.path().display())),
            Err(e) => model.write().error(format!("Could not save {}: {e}", file.path().display())),
        }
    });
}

fn open_preset(mut model: State<Model>, host: HostHandle) {
    spawn_forever(async move {
        let Some(file) =
            rfd::AsyncFileDialog::new().set_title("Open preset").add_filter("Preset", &["json"]).pick_file().await
        else {
            return;
        };
        let text = match std::fs::read_to_string(file.path()) {
            Ok(t) => t,
            Err(e) => {
                model.write().error(format!("Could not read {}: {e}", file.path().display()));
                return;
            }
        };
        let mut m = model.write();
        match m.editor.change(|map| preset::apply(&text, map)) {
            Ok(extras) => {
                let mut note = "Preset loaded. Apply to send it to the boards.".to_owned();
                if let Some(key) = extras.sync_key {
                    host.send(Effect::SyncKey(key));
                    note.push_str(" Its unpair key was saved on the boards.");
                }
                m.info(note);
            }
            Err(e) => m.error(format!("That preset can't be used: {e}")),
        }
    });
}

fn load_blob(mut model: State<Model>) {
    let mut m = model.write();
    let hex: String = m.drafts.blob_hex.chars().filter(|c| !c.is_whitespace()).collect();
    match ConfigBlob::from_hex(&hex) {
        Ok(blob) => {
            let side = blob.side;
            let _ = m.editor.change(|map| {
                map.read_board(&blob);
                Ok::<(), ()>(())
            });
            m.drafts.blob_hex.clear();
            m.info(format!("Loaded into the {} board's mapping. Apply to write it.", side_word(side)));
        }
        Err(e) => m.error(format!("That isn't a config blob: {e}")),
    }
}

/// Everything useful for a bug report, as text.
fn diagnostics(m: &Model) -> String {
    let mut out = format!("XD2Joy app {}\n\n", env!("CARGO_PKG_VERSION"));
    for side in Side::BOTH {
        let b = m.board(side);
        let _ = writeln!(out, "== {} ==", side.label());
        let _ =
            writeln!(out, "link: {:?}\nfirmware: {:?}\nmode: {:?}\ninput: {:?}", b.link, b.firmware, b.mode, b.input);
        let _ = writeln!(out, "profiles: {:?}\ngrip: {:?}\nsync: {:?}", b.profiles, b.grip, b.sync);
        let _ =
            writeln!(out, "pin: {:?}\nattached: {:?}\nhid: {:?}\npairing: {:?}", b.pin, b.attached, b.hid, b.pairing);
        let hex = b.config.as_ref().and_then(|c| c.to_hex().ok()).unwrap_or_else(|| "not read".into());
        let _ = writeln!(out, "config: {hex}\n");
    }
    out.push_str("== Console ==\n");
    for line in &m.log {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn save_diagnostics(mut model: State<Model>) {
    let text = diagnostics(&model.read());
    spawn_forever(async move {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Save diagnostics")
            .set_file_name("xd2joy-diagnostics.txt")
            .save_file()
            .await
        else {
            return;
        };
        match std::fs::write(file.path(), text) {
            Ok(()) => model.write().info(format!("Saved {}", file.path().display())),
            Err(e) => model.write().error(format!("Could not save {}: {e}", file.path().display())),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_positions_round_trip_every_value() {
        for r in [STICK_SENS, MOUSE_GAIN, STICK_DEADZONE, STICK_SPEED] {
            for v in r.clone() {
                assert_eq!(from_percent(percent(v, &r), &r), v);
            }
            assert_eq!(from_percent(-5., &r), *r.start());
            assert_eq!(from_percent(140., &r), *r.end());
        }
    }
}
