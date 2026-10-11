//! What the window shows, as plain data, and how host events and clicks change it. No Freya types
//! here, so every rule is a unit test.
//!
//! Files: `mod.rs` (events in and out, the model), `board.rs` (one board's state from its lines),
//! `editor.rs` (the mapping being edited, undo, the mapper's pick-then-bind flow, conflicts).

mod board;
mod editor;

use std::{collections::VecDeque, path::PathBuf};

pub use board::{BoardView, Firmware, Link, PairState, pairing_tip};
pub use editor::{BindMode, Capture, Editor, NamedHotkey, conflicts, target_name};
use xd2joy_device::ConnectionEvent;
use xd2joy_proto::raw::{LiveInput, Pressed};
use xd2joy_proto::{
    BtAddr, ConfigBlob, Hotkey, InputSource, Mode, RawReport, Reply, Side, Slot, Source, hid::WheelDir,
};

/// Lines kept in the console pane.
pub const LOG_LINES: usize = 300;
/// Distinct reports kept for the live reports table.
const RAW_KEEP: usize = 24;

/// Something the window asks the host to do. Components only produce these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    SetMode(Side, Mode),
    SetInput(Side, InputSource),
    Flash {
        side: Side,
        file: PathBuf,
    },
    /// Read everything from the board again.
    Reload(Side),
    /// Write a config. `expected` is the config the edit was based on: if the board holds
    /// something else now (a profile switched), nothing is written. `save_slot` is the active
    /// profile, which the board loads at boot and so has to be saved as well.
    Apply {
        side: Side,
        blob: ConfigBlob,
        expected: Option<ConfigBlob>,
        save_slot: Option<Slot>,
    },
    /// `cfg default`: the built-in mapping and pairing key.
    RestoreDefaults(Side),
    ProfileSwitch(Slot),
    /// Copy each board's current mapping into the slot and name it.
    ProfileSave {
        slot: Slot,
        name: String,
    },
    ProfileRename {
        slot: Slot,
        name: String,
    },
    ProfileKey {
        slot: Slot,
        key: Option<Hotkey>,
    },
    ProfileClear(Slot),
    Grip(bool),
    /// The console unpair key (hold 2 s); `None` turns it off.
    SyncKey(Option<Hotkey>),
    /// Unpair from the console now.
    SyncNow,
    /// Listen for Bluetooth keyboards and mice for 20 s.
    Scan(Side),
    Pin {
        side: Side,
        addr: BtAddr,
        name: String,
    },
    Unpin(Side),
}

/// Everything the host tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Connected {
        side: Side,
        port: String,
    },
    ConnectFailed {
        side: Side,
        port: String,
        error: String,
    },
    Disconnected {
        side: Side,
    },
    Board {
        side: Side,
        event: ConnectionEvent,
    },
    /// The board answered nothing to `ver`: firmware from before this tool.
    NoVersion {
        side: Side,
    },
    /// More than one board claims the same side; the tool uses the first and says so.
    Duplicate {
        side: Side,
        ignored: String,
    },
    Flash(FlashStage),
    /// The host started (`Some`) or finished (`None`) a multi-step job.
    Working(Option<String>),
    /// The outcome of a job, as a sentence.
    Done(String),
    Failed(String),
    Scanning {
        side: Side,
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashStage {
    Checking {
        side: Side,
        file: PathBuf,
    },
    /// The board was told to restart into its bootloader.
    Restarting {
        side: Side,
    },
    /// The board's firmware cannot restart itself; the person has to double-tap reset.
    NeedsManualReset {
        side: Side,
    },
    Copying {
        side: Side,
        drive: PathBuf,
    },
    WaitingForBoard {
        side: Side,
    },
    Done {
        side: Side,
    },
    Failed {
        side: Side,
        error: String,
    },
}

impl FlashStage {
    pub fn side(&self) -> Side {
        match self {
            FlashStage::Checking { side, .. }
            | FlashStage::Restarting { side }
            | FlashStage::NeedsManualReset { side }
            | FlashStage::Copying { side, .. }
            | FlashStage::WaitingForBoard { side }
            | FlashStage::Done { side }
            | FlashStage::Failed { side, .. } => *side,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, FlashStage::Done { .. } | FlashStage::Failed { .. })
    }

    /// One sentence for the progress line.
    pub fn describe(&self) -> String {
        match self {
            FlashStage::Checking { file, .. } => format!("Checking {}", file.display()),
            FlashStage::Restarting { .. } => "Restarting the board into its bootloader".to_owned(),
            FlashStage::NeedsManualReset { .. } => {
                "This firmware can't restart itself. Short RST to GND twice quickly on this board.".to_owned()
            }
            FlashStage::Copying { drive, .. } => format!("Copying the firmware to {}", drive.display()),
            FlashStage::WaitingForBoard { .. } => "Waiting for the board to come back".to_owned(),
            FlashStage::Done { .. } => "Updated".to_owned(),
            FlashStage::Failed { error, .. } => format!("Update failed: {error}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Mapper,
    Devices,
    Profiles,
    Hotkeys,
    Tuning,
    Colours,
    Firmware,
    Advanced,
}

/// What the pointer is over on the mapper, so related things light up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    Target(xd2joy_proto::Target),
    Source(Source),
}

/// A shortcut being recorded from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyFor {
    Mode(Mode),
    Profile(Slot),
    /// The console unpair key: one key, no modifiers.
    Unpair,
}

impl Page {
    pub const ALL: [Page; 8] = [
        Page::Mapper,
        Page::Devices,
        Page::Profiles,
        Page::Hotkeys,
        Page::Tuning,
        Page::Colours,
        Page::Firmware,
        Page::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Mapper => "Mapper",
            Page::Devices => "Bluetooth",
            Page::Profiles => "Profiles",
            Page::Hotkeys => "Shortcuts",
            Page::Tuning => "Stick & mouse",
            Page::Colours => "Colours",
            Page::Firmware => "Boards",
            Page::Advanced => "Advanced",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Info(String),
    Error(String),
}

/// One `[RAW]` report kept for the Advanced page: the latest bytes per board and handle, and which
/// bytes ever changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRow {
    pub side: Side,
    pub handle: u16,
    pub len: usize,
    pub bytes: Vec<u8>,
    pub changed: Vec<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub boards: [BoardView; 2],
    pub flash: Option<FlashStage>,
    pub log: VecDeque<String>,
    pub editor: Editor,
    pub live: LiveInput,
    pub raw: Vec<RawRow>,
    pub page: Page,
    pub notice: Option<Notice>,
    /// The host's current job.
    pub working: Option<String>,
    /// Add instead of move when binding.
    pub add_mode: bool,
    pub hover: Option<Hover>,
    pub hotkey_capture: Option<HotkeyFor>,
    /// Modifier bits held on this computer's keyboard (left and right told apart by key code).
    pub pc_mods: u8,
    /// A dangerous button pressed once; pressing it again does it.
    pub armed: Option<String>,
    /// Requests made by the model itself (a shortcut recorded from the board's keyboard); the
    /// window sends them to the host.
    pub outbox: Vec<Effect>,
    /// Text being typed into fields, kept here so it survives page changes.
    pub drafts: Drafts,
}

#[derive(Debug, Clone, Default)]
pub struct Drafts {
    /// Profile names, by slot.
    pub profile_names: [String; 4],
    /// The Advanced page's blob field.
    pub blob_hex: String,
}

impl Model {
    pub fn board(&self, side: Side) -> &BoardView {
        &self.boards[side.index()]
    }

    fn board_mut(&mut self, side: Side) -> &mut BoardView {
        &mut self.boards[side.index()]
    }

    /// A flash or another job is running: actions on boards wait.
    pub fn busy(&self) -> bool {
        self.flash.as_ref().is_some_and(|s| !s.is_finished()) || self.working.is_some()
    }

    pub fn bind_mode(&self) -> BindMode {
        if self.add_mode { BindMode::Add } else { BindMode::Move }
    }

    fn log(&mut self, side: Option<Side>, text: impl AsRef<str>) {
        let prefix = side.map_or_else(String::new, |s| format!("{} ", s.letter()));
        self.log.push_back(format!("{prefix}{}", text.as_ref()));
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice::Info(text.into()));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice::Error(text.into()));
    }

    /// The effects that write every board with unapplied changes.
    pub fn apply_effects(&self) -> Result<Vec<Effect>, String> {
        let mut out = Vec::new();
        for side in Side::BOTH {
            if !self.editor.dirty(side) {
                continue;
            }
            let b = self.board(side);
            if !b.writable() {
                return Err(format!("The {} board isn't connected, so its changes can't be applied.", side_word(side)));
            }
            let blob = self.editor.edit.to_blob(side).map_err(|e| e.to_string())?;
            let save_slot = b.profiles.active_saved().and_then(Slot::new);
            out.push(Effect::Apply { side, blob, expected: b.config.clone(), save_slot });
        }
        Ok(out)
    }

    /// The shortcuts the boards know, with names, for conflict checks.
    pub fn hotkeys(&self) -> Vec<NamedHotkey> {
        let mut out: Vec<NamedHotkey> = self
            .editor
            .edit
            .mode_keys
            .iter()
            .map(|h| NamedHotkey { what: format!("{} mode", h.mode.label()), key: h.key })
            .collect();
        let profiles = &self.board(Side::Left).profiles;
        for s in profiles.slots.iter().flatten() {
            if let Some(key) = s.key {
                out.push(NamedHotkey { what: format!("Profile {}", s.slot), key });
            }
        }
        out
    }

    pub fn sync_key(&self) -> Option<Hotkey> {
        Side::BOTH.iter().find_map(|&s| self.board(s).sync).filter(|s| s.on && s.key.key != 0).map(|s| s.key)
    }

    pub fn apply(&mut self, event: HostEvent) {
        match event {
            HostEvent::Connected { side, port } => {
                self.log(Some(side), format!("connected on {port}"));
                *self.board_mut(side) = BoardView { link: Link::Connected { port }, ..BoardView::default() };
            }
            HostEvent::ConnectFailed { side, port, error } => {
                self.log(Some(side), format!("could not open {port}: {error}"));
                self.board_mut(side).link = Link::Failed { port, error };
            }
            HostEvent::Disconnected { side } => {
                self.log(Some(side), "disconnected");
                *self.board_mut(side) = BoardView::default();
                self.live.clear(side);
            }
            HostEvent::Board { side, event } => self.board_event(side, event),
            HostEvent::NoVersion { side } => {
                let board = self.board_mut(side);
                if board.firmware == Firmware::Unknown {
                    board.firmware = Firmware::Old;
                }
            }
            HostEvent::Duplicate { side, ignored } => {
                self.board_mut(side).warning =
                    Some(format!("Another {} board is plugged in on {ignored}; it is ignored.", side_word(side)));
            }
            HostEvent::Flash(stage) => {
                self.log(Some(stage.side()), stage.describe());
                self.flash = Some(stage);
            }
            HostEvent::Working(job) => self.working = job,
            HostEvent::Done(text) => {
                self.log(None, &text);
                self.info(text);
            }
            HostEvent::Failed(text) => {
                self.log(None, &text);
                self.error(text);
            }
            HostEvent::Scanning { side, on } => {
                let b = self.board_mut(side);
                b.scanning = on;
                if on {
                    b.heard.clear();
                }
            }
        }
    }

    fn board_event(&mut self, side: Side, event: ConnectionEvent) {
        match event {
            ConnectionEvent::Line { raw, reply } => {
                if !board::is_chatter(&reply) {
                    self.log(Some(side), &raw);
                }
                match &reply {
                    Reply::Version(info) => {
                        let board = self.board_mut(side);
                        board.warning = (info.side != side).then(|| {
                            format!(
                                "This board's USB id says {}, but its firmware says {}. Flash the right file.",
                                side_word(side),
                                side_word(info.side)
                            )
                        });
                        board.firmware = Firmware::Known(info.clone());
                    }
                    Reply::Raw(r) => self.raw_report(side, r),
                    Reply::Mode { mode: Mode::Keyboard, .. } => {
                        // reports stop in keyboard mode; the last one would look held forever
                        self.live.clear(side);
                    }
                    _ => {}
                }
                if let Some(blob) = self.board_mut(side).take(&reply)
                    && blob.side == side
                    && self.editor.board_config(&blob)
                {
                    self.info(format!(
                        "The {} board's mapping changed on the board (a profile switch?). Your unapplied changes were kept.",
                        side_word(side)
                    ));
                }
            }
            ConnectionEvent::Unreadable { raw, error } => self.log(Some(side), format!("{raw}  ({error})")),
            ConnectionEvent::Lost(why) => self.log(Some(side), format!("connection lost: {why}")),
        }
    }

    fn raw_report(&mut self, side: Side, r: &RawReport) {
        match self.raw.iter_mut().find(|row| row.side == side && row.handle == r.handle && row.len == r.len) {
            Some(row) => {
                for (i, b) in r.bytes.iter().enumerate() {
                    if row.bytes.get(i) != Some(b)
                        && let Some(c) = row.changed.get_mut(i)
                    {
                        *c = true;
                    }
                }
                row.bytes.clone_from(&r.bytes);
            }
            None => {
                if self.raw.len() >= RAW_KEEP {
                    self.raw.remove(0);
                }
                self.raw.push(RawRow {
                    side,
                    handle: r.handle,
                    len: r.len,
                    bytes: r.bytes.clone(),
                    changed: vec![false; r.bytes.len()],
                });
            }
        }
        let mode = self.bind_mode();
        let presses = self.live.feed(side, r);
        if self.hotkey_capture.is_some() && side == Side::Left {
            let mods = self.live.held_mods();
            if let Some(key) = presses.iter().find_map(|p| match p {
                Pressed::Key(k) => Some(*k),
                _ => None,
            }) {
                self.finish_hotkey(Hotkey { key, mods });
            }
            return;
        }
        for p in presses {
            let source = match p {
                Pressed::Key(k) => Source::Key(k),
                Pressed::Modifiers(m) => Source::Modifiers(m),
                Pressed::Mouse(b) => Source::Mouse(b),
                Pressed::Wheel(WheelDir::Up) => Source::Wheel(WheelDir::Up),
                Pressed::Wheel(WheelDir::Down) => Source::Wheel(WheelDir::Down),
            };
            match self.editor.pressed(source, mode) {
                Some(Ok(text)) => self.info(text),
                Some(Err(text)) => self.error(text),
                None => {}
            }
        }
    }
}

impl Model {
    /// A shortcut was recorded: store it where it belongs. Mode shortcuts are part of the mapping
    /// (applied with the rest); profile and unpair shortcuts are sent at once.
    pub fn finish_hotkey(&mut self, key: Hotkey) {
        let Some(target) = self.hotkey_capture.take() else { return };
        let all: Vec<NamedHotkey> = self.hotkeys();
        let clash = all.iter().find(|h| {
            let own = match target {
                HotkeyFor::Mode(m) => h.what == format!("{} mode", m.label()),
                HotkeyFor::Profile(s) => h.what == format!("Profile {}", s.number()),
                HotkeyFor::Unpair => false,
            };
            !own && h.key.clashes(key)
        });
        if let Some(h) = clash {
            self.error(format!("{} is already the {} shortcut.", key.label(), h.what));
            return;
        }
        match target {
            HotkeyFor::Mode(mode) => {
                self.editor.set_mode_key(mode, Some(key));
                self.info(format!("{} toggles {} mode once applied.", key.label(), mode.label()));
            }
            HotkeyFor::Profile(slot) => self.outbox.push(Effect::ProfileKey { slot, key: Some(key) }),
            HotkeyFor::Unpair => self.outbox.push(Effect::SyncKey(Some(Hotkey { key: key.key, mods: 0 }))),
        }
    }

    /// The second press of a dangerous button: true when it should go ahead.
    pub fn confirm(&mut self, what: &str) -> bool {
        if self.armed.as_deref() == Some(what) {
            self.armed = None;
            true
        } else {
            self.armed = Some(what.to_owned());
            false
        }
    }
}

pub fn side_word(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xd2joy_proto::{Button, PROTO_VERSION, Target};

    fn line_on(side: Side, raw: &str) -> HostEvent {
        let reply = xd2joy_proto::parse_line(raw).expect("test lines parse");
        HostEvent::Board { side, event: ConnectionEvent::Line { raw: raw.into(), reply } }
    }

    fn line(raw: &str) -> HostEvent {
        line_on(Side::Left, raw)
    }

    fn connected() -> Model {
        let mut m = Model::default();
        m.apply(HostEvent::Connected { side: Side::Left, port: "/dev/ttyACM0".into() });
        m
    }

    fn with_configs() -> Model {
        let mut m = Model::default();
        for side in Side::BOTH {
            m.apply(HostEvent::Connected { side, port: format!("/dev/tty{}", side.letter()) });
            m.apply(line_on(side, &format!("[VER] fw v2 proto 1 side {}", side.letter())));
            let hex = ConfigBlob::firmware_default(side).to_hex().expect("encodes");
            m.apply(line_on(side, &format!("[CFG] blob {hex}")));
        }
        m
    }

    #[test]
    fn replies_fill_the_board_card() {
        let mut m = connected();
        m.apply(line("[VER] fw v2 proto 1 side L"));
        m.apply(line("[MODE] now STICK, joy-con input on"));
        m.apply(line("[INPUT] src=auto wired=none bridge=0x00 frames=0 bad=0"));
        let b = m.board(Side::Left);
        assert!(b.connected());
        assert!(b.firmware.can_restart_itself());
        assert_eq!(b.mode, Some(Mode::Stick));
        assert_eq!(b.input.map(|i| i.source), Some(InputSource::Auto));
        assert_eq!(b.warning, None);
        assert_eq!(m.log.len(), 4);
    }

    #[test]
    fn status_block_and_raw_lines_stay_out_of_the_console() {
        let mut m = connected();
        m.apply(line("[MAP] grip 1"));
        m.apply(line("[HID] kbd=none"));
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 1A 00 00 00 00 00"));
        assert_eq!(m.log.len(), 1);
        assert_eq!(m.raw.len(), 1);
        assert_eq!(m.live.held_keys().collect::<Vec<_>>(), vec![0x1A]);
    }

    #[test]
    fn firmware_without_ver_is_marked_old_and_cannot_restart_itself() {
        let mut m = connected();
        m.apply(HostEvent::NoVersion { side: Side::Left });
        assert_eq!(m.board(Side::Left).firmware, Firmware::Old);
        assert!(!m.board(Side::Left).firmware.can_restart_itself());
        m.apply(line("[VER] fw v2 proto 1 side L"));
        assert!(m.board(Side::Left).firmware.can_restart_itself());
        m.apply(HostEvent::NoVersion { side: Side::Left });
        assert!(m.board(Side::Left).firmware.can_restart_itself());
    }

    #[test]
    fn side_mismatch_is_flagged() {
        let mut m = connected();
        m.apply(line("[VER] fw v2 proto 1 side R"));
        assert!(m.board(Side::Left).warning.as_deref().is_some_and(|w| w.contains("firmware says right")));
    }

    #[test]
    fn newer_protocol_is_read_only() {
        let mut m = with_configs();
        m.apply(line(&format!("[VER] fw v9 proto {} side L", PROTO_VERSION + 1)));
        assert!(m.board(Side::Left).firmware.newer_than_tool());
        assert!(!m.board(Side::Left).writable());
    }

    #[test]
    fn disconnect_forgets_the_board() {
        let mut m = connected();
        m.apply(line("[MODE] KEYBOARD"));
        m.apply(HostEvent::Disconnected { side: Side::Left });
        assert_eq!(*m.board(Side::Left), BoardView::default());
    }

    #[test]
    fn jobs_and_flashes_make_the_model_busy() {
        let mut m = Model::default();
        assert!(!m.busy());
        m.apply(HostEvent::Flash(FlashStage::Restarting { side: Side::Right }));
        assert!(m.busy());
        m.apply(HostEvent::Flash(FlashStage::Done { side: Side::Right }));
        assert!(!m.busy());
        m.apply(HostEvent::Working(Some("Applying".into())));
        assert!(m.busy());
        m.apply(HostEvent::Working(None));
        assert!(!m.busy());
    }

    #[test]
    fn the_log_is_capped() {
        let mut m = connected();
        for _ in 0..LOG_LINES + 50 {
            m.apply(line("[MODE] KEYBOARD"));
        }
        assert_eq!(m.log.len(), LOG_LINES);
    }

    #[test]
    fn apply_writes_dirty_boards_and_the_active_profile() {
        let mut m = with_configs();
        assert_eq!(m.apply_effects(), Ok(vec![]));
        for l in [
            "[PROF] active 2 of 4",
            "[PROF] 1 empty  key 0x00 mods 0x00 ",
            "[PROF] 2 saved  key 0x00 mods 0x00 ",
            "[PROF] 3 empty  key 0x00 mods 0x00 ",
            "[PROF] 4 empty  key 0x00 mods 0x00 ",
        ] {
            m.apply(line(l));
        }
        let r = m.editor.bind(Source::Key(0x04), Target::Button(Button::Zl), BindMode::Add);
        assert!(r.is_ok());
        let fx = m.apply_effects().expect("applies");
        assert_eq!(fx.len(), 1);
        let Effect::Apply { side, blob, expected, save_slot } = &fx[0] else { panic!("apply effect") };
        assert_eq!(*side, Side::Left);
        assert_eq!(save_slot.map(Slot::number), Some(2));
        assert_eq!(expected.as_ref(), Some(&ConfigBlob::firmware_default(Side::Left)));
        assert_eq!(blob.entries.len(), 28);

        // the read-back after a write makes it clean again
        let hex = blob.to_hex().expect("encodes");
        m.apply(line(&format!("[CFG] blob {hex}")));
        assert!(!m.editor.any_dirty());
    }

    #[test]
    fn edits_for_an_absent_board_cannot_be_applied() {
        let mut m = with_configs();
        let _ = m.editor.bind(Source::Wheel(WheelDir::Up), Target::Button(Button::X), BindMode::Add);
        m.apply(HostEvent::Disconnected { side: Side::Right });
        assert!(m.apply_effects().is_err_and(|e| e.contains("right board")));
    }

    #[test]
    fn a_key_on_the_board_binds_while_a_target_waits() {
        let mut m = with_configs();
        let t = Target::Button(Button::Capture);
        let _ = m.editor.pick_target(t, BindMode::Move);
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 46 00 00 00 00 00"));
        assert!(m.editor.edit.sources_of(t).any(|s| s == Source::Key(0x46)));
        assert!(matches!(m.notice, Some(Notice::Info(_))));
        // the mouse binds through the right board
        let _ = m.editor.pick_target(Target::Button(Button::Home), BindMode::Move);
        m.apply(line_on(Side::Right, "[RAW] h=0x0020 len=4: 04 00 00 00"));
        assert!(m.editor.edit.has_home());
    }

    #[test]
    fn shortcuts_record_and_refuse_clashes() {
        let mut m = with_configs();
        m.hotkey_capture = Some(HotkeyFor::Profile(Slot::new(1).expect("slot")));
        // Ctrl+K is the keyboard mode shortcut
        m.finish_hotkey(Hotkey { key: 0x0E, mods: 0x01 });
        assert!(matches!(&m.notice, Some(Notice::Error(t)) if t.contains("Keyboard mode")));
        assert!(m.outbox.is_empty());
        m.hotkey_capture = Some(HotkeyFor::Profile(Slot::new(1).expect("slot")));
        m.apply(line("[RAW] h=0x0012 len=8: 04 00 1E 00 00 00 00 00"));
        assert_eq!(
            m.outbox,
            vec![Effect::ProfileKey { slot: Slot::new(1).expect("slot"), key: Some(Hotkey { key: 0x1E, mods: 0x04 }) }]
        );
        // a mode shortcut goes into the mapping instead
        m.hotkey_capture = Some(HotkeyFor::Mode(Mode::Stick));
        m.finish_hotkey(Hotkey { key: 0x0A, mods: 0x01 });
        assert_eq!(m.editor.edit.mode_key(Mode::Stick), Some(Hotkey { key: 0x0A, mods: 0x01 }));
        assert!(m.editor.dirty(Side::Left));
    }

    #[test]
    fn dangerous_buttons_need_two_presses() {
        let mut m = Model::default();
        assert!(!m.confirm("restore L"));
        assert!(m.confirm("restore L"));
        assert!(!m.confirm("restore L"));
    }

    #[test]
    fn hotkeys_collect_modes_and_profiles() {
        let mut m = with_configs();
        m.apply(line("[PROF] 3 saved  key 0x20 mods 0x01 x"));
        let names: Vec<String> = m.hotkeys().into_iter().map(|h| h.what).collect();
        assert_eq!(names, vec!["Keyboard mode", "Stick mode", "Profile 3"]);
        m.apply(line("[SYNC] shortcut key 0x45 mods 0x00 on"));
        assert_eq!(m.sync_key(), Some(Hotkey { key: 0x45, mods: 0 }));
    }
}
