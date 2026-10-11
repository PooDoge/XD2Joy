//! The mapping being edited: what the boards hold (`baseline`), the edits on top (`edit`), undo,
//! and the two-step "pick a Joy-Con button, then a key" interaction of the mapper.

use xd2joy_proto::mapping::MapError;
use xd2joy_proto::{Binding, ConfigBlob, Hotkey, Mapping, Mode, Side, Source, Target};

const UNDO_DEPTH: usize = 100;

/// What the mapper is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Capture {
    #[default]
    Idle,
    /// A Joy-Con target was picked; the next key or button pressed (on screen, on this computer's
    /// keyboard, or on the board's Bluetooth device) is bound to it.
    Source(Target),
    /// An input was picked on screen; the next Joy-Con target clicked gets it.
    Target(Source),
}

/// How a new binding treats the input's other targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindMode {
    /// The input stops driving whatever it drove before (the web tool's behaviour).
    Move,
    /// The input drives this target as well (the firmware ORs entries).
    Add,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    pub baseline: Mapping,
    pub edit: Mapping,
    /// The board's config has been read, so its part of `baseline` is real, not a default.
    pub loaded: [bool; 2],
    undo: Vec<Mapping>,
    redo: Vec<Mapping>,
    pub capture: Capture,
    /// The slider or picker being dragged: its steps share one undo entry.
    drag: Option<&'static str>,
}

impl Default for Editor {
    fn default() -> Self {
        let m = Mapping::firmware_default();
        Editor {
            baseline: m.clone(),
            edit: m,
            loaded: [false; 2],
            undo: Vec::new(),
            redo: Vec::new(),
            capture: Capture::Idle,
            drag: None,
        }
    }
}

impl Editor {
    /// A config was read from a board. Without unapplied edits to that board the edit follows the
    /// board. With them, the edits are replayed on what the board now holds (see [`rebase`]), so
    /// settings the person did not touch take the board's new values instead of writing stale ones
    /// back. Returns true when edits were kept although the board changed.
    pub fn board_config(&mut self, blob: &ConfigBlob) -> bool {
        let side = blob.side;
        let clean = !self.loaded[side.index()] || self.edit.same_on_board(&self.baseline, side);
        let mut new_baseline = self.baseline.clone();
        new_baseline.read_board(blob);
        let changed = !new_baseline.same_on_board(&self.baseline, side);
        let old_baseline = std::mem::replace(&mut self.baseline, new_baseline);
        self.loaded[side.index()] = true;
        tracing::debug!(?side, clean, changed, "config read from board");
        if clean {
            self.edit.read_board(blob);
            if changed {
                // the board moved on (a profile switch, a restore); undoing into the old state would
                // write it back over the new one
                self.undo.clear();
                self.redo.clear();
            }
            self.drag = None;
            false
        } else if changed {
            self.edit = rebase(&old_baseline, &self.edit, &self.baseline, side);
            // undo steps hold the board's old settings; stepping back into them would write them
            self.undo.clear();
            self.redo.clear();
            self.drag = None;
            // after an Apply the read-back is the edit itself: nothing to tell
            !self.edit.same_on_board(&self.baseline, side)
        } else {
            false
        }
    }

    pub fn dirty(&self, side: Side) -> bool {
        self.loaded[side.index()] && !self.edit.same_on_board(&self.baseline, side)
    }

    pub fn any_dirty(&self) -> bool {
        Side::BOTH.iter().any(|&s| self.dirty(s))
    }

    /// Drop unapplied edits.
    pub fn discard(&mut self) {
        tracing::debug!("discard edits");
        if self.edit != self.baseline {
            self.push_undo();
            self.edit = self.baseline.clone();
        }
        self.capture = Capture::Idle;
    }

    /// Make a change with undo. The closure's error leaves the mapping as it was.
    pub fn change<T, E>(&mut self, f: impl FnOnce(&mut Mapping) -> Result<T, E>) -> Result<T, E> {
        self.drag = None;
        let before = self.edit.clone();
        match f(&mut self.edit) {
            Ok(v) => {
                if self.edit != before {
                    self.undo.push(before);
                    if self.undo.len() > UNDO_DEPTH {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                }
                Ok(v)
            }
            Err(e) => {
                self.edit = before;
                Err(e)
            }
        }
    }

    /// A step of a drag (a slider, a colour picker). Only the first step since another change
    /// makes an undo entry, so one undo goes back to before the drag.
    pub fn tweak(&mut self, what: &'static str, f: impl FnOnce(&mut Mapping)) {
        let before = self.edit.clone();
        f(&mut self.edit);
        if self.edit != before && self.drag != Some(what) {
            tracing::debug!(what, "drag started");
            self.undo.push(before);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.drag = Some(what);
    }

    fn push_undo(&mut self) {
        self.drag = None;
        self.undo.push(self.edit.clone());
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        tracing::debug!(steps = self.undo.len(), "undo");
        self.drag = None;
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edit, prev));
        }
    }

    pub fn redo(&mut self) {
        tracing::debug!(steps = self.redo.len(), "redo");
        self.drag = None;
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.edit, next));
        }
    }

    /// A Joy-Con target was clicked.
    pub fn pick_target(&mut self, target: Target, mode: BindMode) -> Result<Option<String>, String> {
        match self.capture {
            Capture::Target(source) => {
                // a refused bind keeps the source waiting, so another target can be tried
                let r = self.bind(source, target, mode);
                if r.is_ok() {
                    self.capture = Capture::Idle;
                }
                r.map(Some)
            }
            Capture::Source(t) if t == target => {
                self.capture = Capture::Idle;
                Ok(None)
            }
            _ => {
                self.capture = Capture::Source(target);
                Ok(None)
            }
        }
    }

    /// A key or mouse button was clicked on screen, or pressed on a keyboard or mouse.
    pub fn pick_source(&mut self, source: Source, mode: BindMode) -> Result<Option<String>, String> {
        match self.capture {
            Capture::Source(target) => {
                let r = self.bind(source, target, mode);
                if r.is_ok() {
                    self.capture = Capture::Idle;
                }
                r.map(Some)
            }
            Capture::Target(s) if s == source => {
                self.capture = Capture::Idle;
                Ok(None)
            }
            _ => {
                self.capture = Capture::Target(source);
                Ok(None)
            }
        }
    }

    pub fn cancel(&mut self) {
        self.capture = Capture::Idle;
    }

    /// Bind, and say what happened in one sentence.
    pub fn bind(&mut self, source: Source, target: Target, mode: BindMode) -> Result<String, String> {
        let b = Binding::new(source, target).map_err(|e| sentence(&e.to_string()))?;
        let board = source.board();
        if !self.loaded[board.index()] {
            return Err(format!(
                "The {} board hasn't been read yet, so its bindings can't be changed. Connect it first.",
                if board == Side::Left { "left" } else { "right" }
            ));
        }
        let removed = self
            .change(|m| match mode {
                BindMode::Move => m.rebind(b),
                BindMode::Add => m.add(b).map(|_| Vec::new()),
            })
            .map_err(|e| match e {
                MapError::TooMany { side, .. } => format!(
                    "The {} board is full (32 entries). Remove a binding first.",
                    if side == Side::Left { "left" } else { "right" }
                ),
                other => sentence(&other.to_string()),
            })?;
        tracing::debug!(?source, ?target, ?mode, ?removed, "bind");
        let mut text = format!("{} now presses {}.", source.label(), target_name(target));
        let old: Vec<String> = removed.iter().filter(|r| r.source == source).map(|r| target_name(r.target)).collect();
        if !old.is_empty() {
            text.push_str(&format!(" It no longer presses {}.", old.join(" or ")));
        }
        let replaced: Vec<String> = removed.iter().filter(|r| r.source != source).map(|r| r.source.label()).collect();
        if !replaced.is_empty() {
            text.push_str(&format!(" {} no longer does.", replaced.join(" and ")));
        }
        Ok(text)
    }

    pub fn unbind(&mut self, b: Binding) {
        tracing::debug!(?b, "unbind");
        // removing cannot fail
        let _ = self.change(|m| Ok::<bool, ()>(m.remove(b)));
    }

    pub fn clear_target(&mut self, target: Target) {
        tracing::debug!(?target, "clear target");
        let _ = self.change(|m| {
            m.bindings.retain(|b| b.target != target);
            Ok::<(), ()>(())
        });
    }

    pub fn set_mode_key(&mut self, mode: Mode, key: Option<Hotkey>) {
        tracing::debug!(?mode, ?key, "mode shortcut");
        let _ = self.change(|m| {
            m.set_mode_key(mode, key);
            Ok::<(), ()>(())
        });
    }
}

/// "ZL (left)" style names: L/R halves share labels like SL, SR and "Stick press".
pub fn target_name(t: Target) -> String {
    let half = if t.half() == Side::Left { "left" } else { "right" };
    format!("{} ({half})", t.label())
}

/// `edit`'s changes from `old` to it on one board, replayed on `new`: bindings added or removed,
/// and each setting the edit changed. Settings it left alone take `new`'s value. The other board's
/// half of `edit` is untouched.
pub fn rebase(old: &Mapping, edit: &Mapping, new: &Mapping, side: Side) -> Mapping {
    fn pick<T: PartialEq + Clone>(old: &T, edit: &T, new: &T) -> T {
        if edit == old { new.clone() } else { edit.clone() }
    }
    let i = side.index();
    let on = |b: &&Binding| b.source.board() == side;
    let (was, now): (Vec<Binding>, Vec<Binding>) =
        (old.bindings.iter().filter(on).copied().collect(), edit.bindings.iter().filter(on).copied().collect());
    let mut out = edit.clone();
    out.bindings.retain(|b| b.source.board() != side);
    out.bindings.extend(new.bindings.iter().filter(on).filter(|b| !(was.contains(b) && !now.contains(b))).copied());
    for b in now.iter().filter(|b| !was.contains(b)) {
        if !out.bindings.contains(b) {
            out.bindings.push(*b);
        }
    }
    out.colours[i] = pick(&old.colours[i], &edit.colours[i], &new.colours[i]);
    out.kept[i] = pick(&old.kept[i], &edit.kept[i], &new.kept[i]);
    match side {
        Side::Left => {
            for mode in Mode::ALL {
                out.set_mode_key(mode, pick(&old.mode_key(mode), &edit.mode_key(mode), &new.mode_key(mode)));
            }
        }
        Side::Right => {
            let (o, e, n) = (old.tuning, edit.tuning, new.tuning);
            out.tuning.stick_sens = pick(&o.stick_sens, &e.stick_sens, &n.stick_sens);
            out.tuning.mouse_gain = pick(&o.mouse_gain, &e.mouse_gain, &n.mouse_gain);
            out.tuning.accel = pick(&o.accel, &e.accel, &n.accel);
            out.tuning.stick_deadzone = pick(&o.stick_deadzone, &e.stick_deadzone, &n.stick_deadzone);
            out.tuning.stick_speed = pick(&o.stick_speed, &e.stick_speed, &n.stick_speed);
            for a in 0..2 {
                out.right_stick[a] = pick(&old.right_stick[a], &edit.right_stick[a], &new.right_stick[a]);
            }
        }
    }
    out
}

/// What Apply would change on one board, one line per change, for the Advanced page and the log.
pub fn changes(before: &Mapping, after: &Mapping, side: Side) -> Vec<String> {
    let mut out = Vec::new();
    let on_board =
        |m: &Mapping| -> Vec<Binding> { m.bindings.iter().copied().filter(|b| b.source.board() == side).collect() };
    let (old, new) = (on_board(before), on_board(after));
    for b in new.iter().filter(|b| !old.contains(b)) {
        out.push(format!("+ {} presses {}", b.source.label(), target_name(b.target)));
    }
    for b in old.iter().filter(|b| !new.contains(b)) {
        out.push(format!("- {} no longer presses {}", b.source.label(), target_name(b.target)));
    }
    let i = side.index();
    let (c0, c1) = (before.colours[i], after.colours[i]);
    for (name, x, y) in [
        ("body", c0.body, c1.body),
        ("buttons", c0.buttons, c1.buttons),
        ("rail", c0.rail, c1.rail),
        ("stick", c0.stick, c1.stick),
    ] {
        if x != y {
            out.push(format!("colour of the {name}: {x} → {y}"));
        }
    }
    match side {
        Side::Left => {
            for mode in Mode::ALL {
                let (x, y) = (before.mode_key(mode), after.mode_key(mode));
                if x != y {
                    let show = |k: Option<Hotkey>| k.map_or("none".to_owned(), |k| k.label());
                    out.push(format!("{} mode shortcut: {} → {}", mode.label(), show(x), show(y)));
                }
            }
        }
        Side::Right => {
            let (t0, t1) = (before.tuning, after.tuning);
            for (name, x, y) in [
                ("pointer speed", t0.mouse_gain, t1.mouse_gain),
                ("stick sensitivity", t0.stick_sens, t1.stick_sens),
                ("anti-deadzone", t0.stick_deadzone, t1.stick_deadzone),
                ("stick speed", t0.stick_speed, t1.stick_speed),
            ] {
                if x != y {
                    out.push(format!("{name}: {x} → {y}"));
                }
            }
            if t0.accel != t1.accel {
                out.push(format!("acceleration: {} → {}", on_off(t0.accel), on_off(t1.accel)));
            }
            for (axis, x, y) in [
                ("left/right", before.right_stick[0], after.right_stick[0]),
                ("up/down", before.right_stick[1], after.right_stick[1]),
            ] {
                if x != y {
                    out.push(format!("right stick {axis} in stick mode: {} → {}", x.label(), y.label()));
                }
            }
        }
    }
    if before.kept[i] != after.kept[i] {
        // entries the app does not model (from a preset or a pasted blob), written back as they are
        out.push("settings this app does not show (unknown entries, stick gain)".to_owned());
    }
    out
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn sentence(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), c.as_str().trim_end_matches('.')),
        None => String::new(),
    }
}

/// A shortcut with what it does, for conflict checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedHotkey {
    pub what: String,
    pub key: Hotkey,
}

/// Every problem between shortcuts and bindings, as sentences.
///
/// - Two shortcuts where pressing one also fires the other (same key, one's modifiers a subset).
/// - The unpair key used by anything else: holding it 2 s unpairs the console.
/// - A shortcut whose key also presses a Joy-Con button: key bindings ignore modifiers, so
///   Ctrl+K also presses whatever K is bound to.
pub fn conflicts(edit: &Mapping, hotkeys: &[NamedHotkey], sync: Option<Hotkey>) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in hotkeys.iter().enumerate() {
        for b in &hotkeys[i + 1..] {
            if a.key.clashes(b.key) {
                out.push(format!("{} and {} use the same key ({}).", a.what, b.what, a.key.label()));
            }
        }
    }
    if let Some(s) = sync {
        for h in hotkeys {
            if h.key.key == s.key {
                out.push(format!(
                    "{} is also the unpair key. Holding it for 2 seconds unpairs the console.",
                    h.key.label()
                ));
            }
        }
        if edit.targets_of(Source::Key(s.key)).next().is_some() {
            out.push(format!(
                "The unpair key {} also presses a button. Holding it for 2 seconds unpairs the console.",
                Hotkey { key: s.key, mods: 0 }.label()
            ));
        }
    }
    for h in hotkeys {
        let targets: Vec<String> = edit.targets_of(Source::Key(h.key.key)).map(target_name).collect();
        if !targets.is_empty() && h.key.mods != 0 {
            out.push(format!(
                "{} also presses {}, because button bindings ignore modifiers.",
                h.key.label(),
                targets.join(" and ")
            ));
        } else if !targets.is_empty() {
            out.push(format!("{} ({}) also presses {}.", h.key.label(), h.what, targets.join(" and ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xd2joy_proto::Button;

    fn loaded() -> Editor {
        let mut e = Editor::default();
        for side in Side::BOTH {
            e.board_config(&ConfigBlob::firmware_default(side));
        }
        e
    }

    #[test]
    fn edits_made_offline_are_replayed_on_what_the_board_now_holds() {
        let mut e = loaded();
        let _ = e.bind(Source::Key(0x59), Target::Button(Button::Zl), BindMode::Add);
        e.tweak("gain", |m| m.tuning.mouse_gain = 300);
        // meanwhile the boards were changed elsewhere: new colours on the left, stick speed on the right
        let mut left = e.baseline.to_blob(Side::Left).expect("blob");
        left.colours.body = xd2joy_proto::Rgb([9, 9, 9]);
        let mut other = e.baseline.clone();
        other.tuning.stick_speed = 7;
        let right = other.to_blob(Side::Right).expect("blob");
        assert!(e.board_config(&left));
        assert!(e.board_config(&right));
        assert_eq!(e.edit.colours[0].body, xd2joy_proto::Rgb([9, 9, 9]), "untouched setting follows the board");
        assert_eq!(e.edit.tuning.stick_speed, 7);
        assert_eq!(e.edit.tuning.mouse_gain, 300, "the edit is kept");
        assert!(e.edit.bindings.contains(&Binding { source: Source::Key(0x59), target: Target::Button(Button::Zl) }));
    }

    #[test]
    fn reading_back_an_apply_is_not_a_change_underneath() {
        let mut e = loaded();
        e.tweak("gain", |m| m.tuning.mouse_gain = 300);
        let written = e.edit.to_blob(Side::Right).expect("blob");
        assert!(!e.board_config(&written));
        assert!(!e.dirty(Side::Right));
    }

    #[test]
    fn changes_are_listed_per_board() {
        let mut e = loaded();
        let _ = e.bind(Source::Key(0x59), Target::Button(Button::Zl), BindMode::Add);
        e.tweak("gain", |m| m.tuning.mouse_gain = 300);
        let left = changes(&e.baseline, &e.edit, Side::Left);
        assert_eq!(left.len(), 1);
        assert!(left[0].starts_with("+ ") && left[0].contains("ZL"), "{left:?}");
        assert_eq!(changes(&e.baseline, &e.edit, Side::Right), vec!["pointer speed: 420 → 300".to_owned()]);
        e.discard();
        assert!(changes(&e.baseline, &e.edit, Side::Left).is_empty());
    }

    #[test]
    fn a_drag_is_one_undo_step() {
        let mut e = loaded();
        for v in [300, 400, 500] {
            e.tweak("gain", |m| m.tuning.mouse_gain = v);
        }
        e.tweak("accel", |m| m.tuning.accel = false);
        e.tweak("gain", |m| m.tuning.mouse_gain = 600);
        e.undo();
        assert_eq!(e.edit.tuning.mouse_gain, 500);
        e.undo();
        assert!(e.edit.tuning.accel);
        e.undo();
        assert_eq!(e.edit.tuning, e.baseline.tuning);
        assert!(!e.can_undo());
    }

    #[test]
    fn edits_make_only_their_board_dirty_and_undo_works() {
        let mut e = loaded();
        assert!(!e.any_dirty());
        let msg = e.bind(Source::Key(0x04), Target::Button(Button::Zl), BindMode::Move);
        assert!(msg.is_ok_and(|m| m.contains("A now presses ZL (left)") && m.contains("no longer presses Stick left")));
        assert!(e.dirty(Side::Left));
        assert!(!e.dirty(Side::Right));
        e.undo();
        assert!(!e.any_dirty());
        e.redo();
        assert!(e.dirty(Side::Left));
        e.discard();
        assert!(!e.any_dirty());
    }

    #[test]
    fn a_board_read_follows_unless_there_are_edits() {
        let mut e = loaded();
        let mut changed = ConfigBlob::firmware_default(Side::Right);
        changed.entries.truncate(2);
        assert!(!e.board_config(&changed));
        assert_eq!(e.edit.tuning, xd2joy_proto::Tuning::FIRMWARE_FALLBACK);
        // now with an edit on the right board
        let _ =
            e.bind(Source::mouse(xd2joy_proto::hid::MouseButton::Middle), Target::Button(Button::Home), BindMode::Add);
        let kept = e.board_config(&ConfigBlob::firmware_default(Side::Right));
        assert!(kept);
        assert!(e.edit.has_home());
        assert!(e.dirty(Side::Right));
    }

    #[test]
    fn two_step_picking_in_either_order() {
        let mut e = loaded();
        let t = Target::Button(Button::Capture);
        let s = Source::Key(0x46);
        assert_eq!(e.pick_target(t, BindMode::Move), Ok(None));
        assert_eq!(e.capture, Capture::Source(t));
        assert!(e.pick_source(s, BindMode::Move).is_ok_and(|m| m.is_some()));
        assert_eq!(e.capture, Capture::Idle);
        // a key replaces the key that was there (F5)
        assert_eq!(e.edit.sources_of(t).collect::<Vec<_>>(), vec![s]);

        let s2 = Source::Key(0x47);
        assert_eq!(e.pick_source(s2, BindMode::Add), Ok(None));
        assert!(e.pick_target(t, BindMode::Add).is_ok_and(|m| m.is_some()));
        assert_eq!(e.edit.sources_of(t).count(), 2);
    }

    #[test]
    fn an_unread_board_cannot_be_edited_and_a_refused_bind_keeps_waiting() {
        let mut e = Editor::default();
        e.board_config(&ConfigBlob::firmware_default(Side::Left));
        let t = Target::Button(Button::A);
        let _ = e.pick_target(t, BindMode::Move);
        // the mouse is on the right board, which was never read
        let r = e.pick_source(Source::Wheel(xd2joy_proto::hid::WheelDir::Up), BindMode::Move);
        assert!(r.is_err_and(|m| m.contains("right board")));
        assert_eq!(e.capture, Capture::Source(t));
        assert!(e.pick_source(Source::Key(0x04), BindMode::Move).is_ok());
    }

    #[test]
    fn a_board_that_moved_on_clears_undo() {
        let mut e = loaded();
        e.tweak("gain", |m| m.tuning.mouse_gain = 300);
        e.discard();
        assert!(e.can_undo());
        let mut other = ConfigBlob::firmware_default(Side::Right);
        other.stick_gain = other.stick_gain.wrapping_add(1);
        e.board_config(&other);
        assert!(!e.can_undo() && !e.can_redo());
    }

    #[test]
    fn invalid_and_full_binds_explain_themselves() {
        let mut e = loaded();
        let r = e.bind(
            Source::mouse(xd2joy_proto::hid::MouseButton::Left),
            Target::Stick(xd2joy_proto::StickDir::Up),
            BindMode::Add,
        );
        assert_eq!(r, Err("Only keyboard keys can push the left stick.".into()));
        for k in 0x59..0x5E {
            assert!(e.bind(Source::Key(k), Target::Button(Button::L), BindMode::Add).is_ok());
        }
        let r = e.bind(Source::Key(0x5E), Target::Button(Button::L), BindMode::Add);
        assert_eq!(r, Err("The left board is full (32 entries). Remove a binding first.".into()));
        assert_eq!(e.edit.entries_used(Side::Left), 32);
    }

    #[test]
    fn conflicts_are_found() {
        let e = loaded();
        let hk = |what: &str, key, mods| NamedHotkey { what: what.into(), key: Hotkey { key, mods } };
        // default: Ctrl+K and Ctrl+J; K and J are not bound to buttons in the default
        let base = vec![hk("Keyboard mode", 0x0E, 1), hk("Stick mode", 0x0D, 1)];
        assert!(conflicts(&e.edit, &base, Some(Hotkey { key: 0x45, mods: 0 })).is_empty());
        let mut clash = base.clone();
        clash.push(hk("Profile 1", 0x0E, 0));
        let c = conflicts(&e.edit, &clash, None);
        assert!(c.iter().any(|s| s.contains("Keyboard mode and Profile 1")));
        // W is bound to the stick
        let c = conflicts(&e.edit, &[hk("Profile 2", 0x1A, 1)], None);
        assert!(c.iter().any(|s| s.contains("ignore modifiers")));
        // unpair key on a bound key
        let c = conflicts(&e.edit, &[], Some(Hotkey { key: 0x2C, mods: 0 }));
        assert!(c.iter().any(|s| s.contains("unpair")));
    }
}
