//! What the window shows, as plain data, and how host events and clicks change it. No Freya types
//! here, so every rule is a unit test.
//!
//! Files: `mod.rs` (events in and out, the model), `board.rs` (one board's state from its lines),
//! `editor.rs` (the mapping being edited, undo, the mapper's pick-then-bind flow, conflicts).

mod board;
mod editor;

use std::{collections::VecDeque, path::PathBuf};

pub use board::{BoardView, Firmware, Link, PairState, pairing_tip};
pub use editor::{BindMode, Capture, Editor, NamedHotkey, changes, conflicts, target_name};
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
    /// Store a preset's configs in a slot without changing what the boards run (unless the slot
    /// is the active one). One config per connected board.
    ProfileFromPreset {
        slot: Slot,
        name: String,
        blobs: Vec<ConfigBlob>,
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

impl Effect {
    /// One line for the log. A config is shown as the hex `cfg` takes, so a log can be replayed.
    pub fn summary(&self) -> String {
        match self {
            Effect::ProfileFromPreset { slot, name, blobs } => format!(
                "ProfileFromPreset slot={} name={name:?} blobs={}",
                slot.number(),
                blobs.iter().map(|b| b.to_hex().unwrap_or_else(|e| format!("<{e}>"))).collect::<Vec<_>>().join(",")
            ),
            Effect::Apply { side, blob, save_slot, .. } => format!(
                "Apply {side:?} save_slot={:?} blob={}",
                save_slot.map(Slot::number),
                blob.to_hex().unwrap_or_else(|e| format!("<{e}>"))
            ),
            other => format!("{other:?}"),
        }
    }
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
    /// The window was asked to close while something would be lost; it stays open once.
    CloseRefused(String),
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
                "This firmware can't restart itself. Double-press this board's reset button (on a Pro Micro, short RST \
                 to GND twice), quickly."
                    .to_owned()
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
    /// When each board's remembered config was read, for a board edited while unplugged.
    pub cached: [Option<u64>; 2],
    /// A modifier pressed while a Joy-Con button waits; it binds on release unless a key follows.
    pub pending_modifier: Option<PendingModifier>,
}

/// A modifier waiting for its release, with what it would bind to and where it was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingModifier {
    pub bits: u8,
    /// The capture it was pressed in; if that ended or moved to another button, it binds nothing.
    pub capture: Capture,
    /// Pressed on this computer's keyboard rather than the board's; only that keyboard releases it.
    pub from_pc: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Drafts {
    /// Profile names, by slot.
    pub profile_names: [String; 4],
    /// The Advanced page's blob field.
    pub blob_hex: String,
    /// Typed values on the Stick & mouse page: pointer speed, sensitivity, anti-deadzone, speed.
    pub tuning: [String; 4],
    /// Hex colours typed on the Colours page, by half and part (body, buttons, rail, stick).
    pub colours: [[String; 4]; 2],
    /// The values `tuning` and `colours` were typed over. When the values change some other way
    /// (a slider, undo, a board read), the typed text is stale and dropped.
    basis: Option<(xd2joy_proto::mapping::Tuning, [xd2joy_proto::blob::Colours; 2])>,
}

impl Drafts {
    /// Typed values that are still about the current values, for showing in their fields.
    pub fn fresh(&self, map: &xd2joy_proto::Mapping) -> bool {
        self.basis == Some((map.tuning, map.colours))
    }
}

impl Model {
    /// For writing a typed value: drops stale text first, so typing starts from an empty field.
    pub fn value_drafts(&mut self) -> &mut Drafts {
        let now = (self.editor.edit.tuning, self.editor.edit.colours);
        if self.drafts.basis != Some(now) {
            self.drafts.tuning = Default::default();
            self.drafts.colours = Default::default();
            self.drafts.basis = Some(now);
        }
        &mut self.drafts
    }
}

impl Model {
    /// Start from the configs remembered from the last time each board was connected.
    pub fn load_cached(&mut self, cached: [Option<crate::cache::Cached>; 2]) {
        for c in cached.into_iter().flatten() {
            let side = c.blob.side;
            tracing::info!(?side, read_at = c.read_at, "using the remembered config until the board connects");
            self.editor.board_config(&c.blob);
            self.cached[side.index()] = Some(c.read_at);
        }
    }

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

    /// Show a notice. Every notice is logged, so a log shows what the person was told.
    pub fn info(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::info!(notice = %text, "shown");
        self.notice = Some(Notice::Info(text));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!(notice = %text, "shown");
        self.notice = Some(Notice::Error(text));
    }

    /// The effects that write every board with unapplied changes.
    pub fn apply_effects(&self) -> Result<Vec<Effect>, String> {
        let mut out = Vec::new();
        for side in Side::BOTH {
            if !self.editor.dirty(side) || !self.board(side).writable() {
                continue;
            }
            let b = self.board(side);
            let blob = self.editor.edit.to_blob(side).map_err(|e| e.to_string())?;
            let save_slot = b.profiles.active_saved().and_then(Slot::new);
            out.push(Effect::Apply { side, blob, expected: b.config.clone(), save_slot });
        }
        match self.waiting_for_board().as_slice() {
            [] => Ok(out),
            [side, ..] if out.is_empty() => Err(self.why_not_writable(*side).unwrap_or_default()),
            _ => Ok(out),
        }
    }

    /// Why the tool can't write a board's settings now, as a sentence; `None` when it can.
    pub fn why_not_writable(&self, side: Side) -> Option<String> {
        let b = self.board(side);
        let word = side_word(side);
        if b.writable() {
            None
        } else if !b.connected() {
            Some(format!("The {word} board isn't connected. Your changes to it are kept; apply them once it is."))
        } else if b.firmware.newer_than_tool() {
            Some(format!(
                "The {word} board runs firmware newer than this app, so the app only reads it. Update the app to \
                 change its settings."
            ))
        } else {
            Some(format!(
                "The {word} board's config hasn't been read yet. Your changes are kept; Apply again in a moment, or \
                 replug the board if this stays."
            ))
        }
    }

    /// A preset file's configs for each connected board, built on what the boards hold now (not on
    /// unapplied edits), for filling a profile slot.
    pub fn preset_blobs(&self, json: &str) -> Result<Vec<ConfigBlob>, String> {
        let mut map = self.editor.baseline.clone();
        xd2joy_proto::preset::apply(json, &mut map).map_err(|e| format!("That preset can't be used: {e}"))?;
        let blobs: Vec<ConfigBlob> = Side::BOTH
            .into_iter()
            .filter(|&s| self.board(s).writable())
            .map(|s| map.to_blob(s).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        if blobs.is_empty() {
            return Err("Connect a board first.".into());
        }
        Ok(blobs)
    }

    /// What closing the window now would lose, if anything.
    pub fn close_warning(&self) -> Option<String> {
        if let Some(stage) = self.flash.as_ref().filter(|s| !s.is_finished()) {
            return Some(format!(
                "The {} board's firmware is being updated. Stopping now leaves it in its bootloader until it is \
                 updated again.",
                side_word(stage.side())
            ));
        }
        if let Some(job) = &self.working {
            return Some(format!("{job} is still running."));
        }
        let dirty: Vec<&str> = Side::BOTH.into_iter().filter(|&s| self.editor.dirty(s)).map(side_word).collect();
        match dirty.as_slice() {
            [] => None,
            [one] => Some(format!("The {one} board has unapplied changes.")),
            _ => Some("Both boards have unapplied changes.".to_owned()),
        }
    }

    /// Boards with unapplied changes that can't be written now (unplugged, or still being read).
    pub fn waiting_for_board(&self) -> Vec<Side> {
        Side::BOTH.into_iter().filter(|&s| self.editor.dirty(s) && !self.board(s).writable()).collect()
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
            HostEvent::CloseRefused(reason) => self.error(format!("{reason} Close the window again to quit anyway.")),
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
                {
                    // read live now; the remembered copy no longer matters
                    self.cached[side.index()] = None;
                    if self.editor.board_config(&blob) {
                        self.info(format!(
                            "The {} board's mapping changed on the board (a profile switch?). Your unapplied \
                             changes were kept; settings you did not touch now show what the board holds.",
                            side_word(side)
                        ));
                    }
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
            self.capture_press(source, false);
        }
        if side == Side::Left {
            let held = self.live.held_mods();
            self.capture_release(held, false);
        }
    }

    /// A key or button pressed on a device while a Joy-Con button waits for one. A modifier waits
    /// for its release: Shift then K binds K (bindings ignore modifiers), Shift alone binds Shift.
    pub fn capture_press(&mut self, source: Source, from_pc: bool) {
        if !matches!(self.editor.capture, Capture::Source(_)) {
            self.pending_modifier = None;
            return;
        }
        if let Source::Modifiers(bits) = source {
            self.pending_modifier = Some(PendingModifier { bits, capture: self.editor.capture, from_pc });
            return;
        }
        self.pending_modifier = None;
        let mode = self.bind_mode();
        match self.editor.pick_source(source, mode) {
            Ok(Some(text)) => self.info(text),
            Ok(None) => {}
            Err(text) => self.error(text),
        }
    }

    /// Modifiers still held after a report or a key release: a waiting modifier let go without
    /// another key binds now.
    pub fn capture_release(&mut self, still_held: u8, from_pc: bool) {
        let Some(pending) = self.pending_modifier else { return };
        if pending.from_pc != from_pc || still_held & pending.bits != 0 {
            return;
        }
        self.pending_modifier = None;
        let bits = pending.bits;
        if self.editor.capture == pending.capture && matches!(self.editor.capture, Capture::Source(_)) {
            let mode = self.bind_mode();
            match self.editor.pick_source(Source::Modifiers(bits), mode) {
                Ok(Some(text)) => self.info(text),
                Ok(None) => {}
                Err(text) => self.error(text),
            }
        }
    }
}

impl Model {
    /// A shortcut was recorded: store it where it belongs. Mode shortcuts are part of the mapping
    /// (applied with the rest); profile and unpair shortcuts are sent at once.
    pub fn finish_hotkey(&mut self, key: Hotkey) {
        let Some(target) = self.hotkey_capture else { return };
        // A refused key keeps the recording going, so another key can be pressed straight away.
        if let Err(why) = self.hotkey_allowed(target, key) {
            self.error(format!("{why} Press another key, or Esc."));
            return;
        }
        self.hotkey_capture = None;
        match target {
            HotkeyFor::Mode(mode) => {
                self.editor.set_mode_key(mode, Some(key));
                self.info(format!("{} toggles {} mode once applied.", key.label(), mode.label()));
            }
            HotkeyFor::Profile(slot) => self.outbox.push(Effect::ProfileKey { slot, key: Some(key) }),
            HotkeyFor::Unpair => {
                let bare = Hotkey { key: key.key, mods: 0 };
                if key.mods != 0 {
                    self.info(format!("The unpair key takes no modifiers, so it is {}.", bare.label()));
                }
                self.outbox.push(Effect::SyncKey(Some(bare)));
            }
        }
    }

    /// Why `key` can't be the shortcut for `target`, if it can't.
    fn hotkey_allowed(&self, target: HotkeyFor, key: Hotkey) -> Result<(), String> {
        let own = |h: &NamedHotkey| match target {
            HotkeyFor::Mode(m) => h.what == format!("{} mode", m.label()),
            HotkeyFor::Profile(s) => h.what == format!("Profile {}", s.number()),
            HotkeyFor::Unpair => false,
        };
        let others: Vec<NamedHotkey> = self.hotkeys().into_iter().filter(|h| !own(h)).collect();
        match target {
            // Held 2 s, the unpair key fires whatever else is held, so only the key itself counts.
            HotkeyFor::Unpair => {
                if let Some(h) = others.iter().find(|h| h.key.key == key.key) {
                    return Err(format!("{} is part of the {} shortcut.", h.key.label(), h.what));
                }
                let presses: Vec<String> = self.editor.edit.targets_of(Source::Key(key.key)).map(target_name).collect();
                if !presses.is_empty() {
                    return Err(format!(
                        "{} presses {}; holding it in a game would unpair the console.",
                        Hotkey { key: key.key, mods: 0 }.label(),
                        presses.join(" and ")
                    ));
                }
            }
            _ => {
                if let Some(h) = others.iter().find(|h| h.key.clashes(key)) {
                    return Err(format!("{} is already the {} shortcut.", key.label(), h.what));
                }
                if self.sync_key().is_some_and(|s| s.key == key.key) {
                    return Err(format!(
                        "{} is the console unpair key; holding it 2 seconds unpairs.",
                        Hotkey { key: key.key, mods: 0 }.label()
                    ));
                }
            }
        }
        Ok(())
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
    fn a_connected_board_applies_while_the_other_waits() {
        let mut m = with_configs();
        let _ = m.editor.bind(Source::Wheel(WheelDir::Up), Target::Button(Button::X), BindMode::Add);
        let _ = m.editor.bind(Source::Key(0x59), Target::Button(Button::Zl), BindMode::Add);
        m.apply(HostEvent::Disconnected { side: Side::Right });
        let fx = m.apply_effects().expect("left applies");
        assert!(matches!(fx.as_slice(), [Effect::Apply { side: Side::Left, .. }]));
        assert_eq!(m.waiting_for_board(), vec![Side::Right]);
    }

    #[test]
    fn a_remembered_config_can_be_edited_before_the_board_connects() {
        let mut m = Model::default();
        let mut right = ConfigBlob::firmware_default(Side::Right);
        right.stick_gain = right.stick_gain.wrapping_add(7);
        m.load_cached([None, Some(crate::cache::Cached { blob: right.clone(), read_at: 1, firmware: None })]);
        assert_eq!(m.cached, [None, Some(1)]);
        let r = m.editor.bind(Source::Wheel(WheelDir::Up), Target::Button(Button::X), BindMode::Add);
        assert!(r.is_ok(), "{r:?}");
        assert!(m.editor.dirty(Side::Right));
        // the board connects holding the same config: the edit stays and can be applied
        m.apply(HostEvent::Connected { side: Side::Right, port: "/dev/ttyR".into() });
        m.apply(line_on(Side::Right, &format!("[CFG] blob {}", right.to_hex().expect("hex"))));
        assert!(m.editor.dirty(Side::Right));
        assert_eq!(m.apply_effects().map(|fx| fx.len()), Ok(1));
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
    fn a_modifier_waits_for_release_unless_a_key_follows() {
        let mut m = with_configs();
        let t = Target::Button(Button::Capture);
        // Shift then K on the board: K is bound
        let _ = m.editor.pick_target(t, BindMode::Move);
        m.apply(line("[RAW] h=0x0012 len=8: 02 00 00 00 00 00 00 00"));
        assert_eq!(m.editor.capture, Capture::Source(t));
        m.apply(line("[RAW] h=0x0012 len=8: 02 00 0E 00 00 00 00 00"));
        assert_eq!(m.editor.edit.sources_of(t).collect::<Vec<_>>(), vec![Source::Key(0x0E)]);
        // Shift alone, released: Shift is bound
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 00 00 00 00 00 00"));
        let _ = m.editor.pick_target(t, BindMode::Move);
        m.apply(line("[RAW] h=0x0012 len=8: 02 00 00 00 00 00 00 00"));
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 00 00 00 00 00 00"));
        assert_eq!(m.editor.edit.sources_of(t).collect::<Vec<_>>(), vec![Source::Modifiers(0x02)]);
        assert_eq!(m.editor.capture, Capture::Idle);
        // Shift held, then another button picked: releasing Shift binds nothing to either
        let other = Target::Button(Button::Plus);
        let before: Vec<Source> = m.editor.edit.sources_of(other).collect();
        let _ = m.editor.pick_target(t, BindMode::Move);
        m.apply(line("[RAW] h=0x0012 len=8: 02 00 00 00 00 00 00 00"));
        let _ = m.editor.pick_target(other, BindMode::Move);
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 00 00 00 00 00 00"));
        assert_eq!(m.editor.capture, Capture::Source(other));
        assert_eq!(m.editor.edit.sources_of(other).collect::<Vec<_>>(), before);
        // a computer-keyboard Shift is not released by a board report
        m.capture_press(Source::Modifiers(0x02), true);
        m.apply(line("[RAW] h=0x0012 len=8: 00 00 00 00 00 00 00 00"));
        assert!(m.pending_modifier.is_some());
        m.capture_release(0, true);
        assert_eq!(m.editor.edit.sources_of(other).collect::<Vec<_>>(), vec![Source::Modifiers(0x02)]);
    }

    #[test]
    fn the_unpair_key_refuses_shortcut_keys_and_bound_keys() {
        let mut m = with_configs();
        m.hotkey_capture = Some(HotkeyFor::Unpair);
        // K is in the Ctrl+K keyboard mode shortcut
        m.finish_hotkey(Hotkey { key: 0x0E, mods: 0 });
        assert!(matches!(&m.notice, Some(Notice::Error(t)) if t.contains("Keyboard mode")));
        assert_eq!(m.hotkey_capture, Some(HotkeyFor::Unpair), "still recording");
        // Space presses B by default
        m.finish_hotkey(Hotkey { key: 0x2C, mods: 0 });
        assert!(matches!(&m.notice, Some(Notice::Error(t)) if t.contains("unpair the console")));
        // F11 is free; modifiers are dropped
        m.finish_hotkey(Hotkey { key: 0x44, mods: 0x01 });
        assert_eq!(m.outbox, vec![Effect::SyncKey(Some(Hotkey { key: 0x44, mods: 0 }))]);
        assert_eq!(m.hotkey_capture, None);
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
