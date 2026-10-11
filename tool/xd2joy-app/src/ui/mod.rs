//! The window. Components draw the `Model` and turn clicks into model changes or `Effect`s for the
//! host; the rules live in `model`.
//!
//! Files: `mod.rs` (the frame: top bar, sidebar, notices, keyboard handling, shared widgets),
//! `mapper.rs` (the Joy-Con, keyboard and mouse drawings), `pages.rs` (the other pages),
//! `layout.rs` (where every key and button is drawn).

mod layout;
mod mapper;
mod pages;

use std::sync::{Arc, Mutex};

use freya::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;
use xd2joy_proto::{Side, hid};

use crate::{
    host::HostHandle,
    model::{Capture, Effect, HostEvent, Link, Model, Notice, Page},
    theme::*,
};

/// Something handed once to the first render (the toolkit may clone the root component).
pub struct Handoff<T>(Arc<Mutex<Option<T>>>);

impl<T> Clone for Handoff<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Handoff<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

pub struct Root {
    pub host: HostHandle,
    pub inbox: Handoff<UnboundedReceiver<HostEvent>>,
    pub start_page: Page,
    /// Each board's config from the last time it was connected.
    pub cached: Handoff<[Option<crate::cache::Cached>; 2]>,
    /// Told on every render what closing would lose.
    pub close: crate::close::CloseGuard,
}

/// Send whatever the model queued for the host.
fn flush(model: &mut Model, host: &HostHandle) {
    for e in std::mem::take(&mut model.outbox) {
        host.send(e);
    }
}

impl App for Root {
    fn render(&self) -> impl IntoElement {
        use_init_theme(dark_theme);
        let start_page = self.start_page;
        let cached = self.cached.clone();
        let mut model = use_state(move || {
            let mut m = Model { page: start_page, ..Model::default() };
            if let Some(c) = cached.take() {
                m.load_cached(c);
            }
            m
        });
        use_hook({
            let inbox = self.inbox.clone();
            let host = self.host.clone();
            move || {
                if let Some(mut rx) = inbox.take() {
                    spawn(async move {
                        while let Some(event) = rx.next().await {
                            // A mouse can print a report per movement; take everything waiting
                            // and redraw once.
                            let mut m = model.write();
                            m.apply(event);
                            while let Ok(more) = rx.try_recv() {
                                m.apply(more);
                            }
                            flush(&mut m, &host);
                        }
                    });
                }
            }
        });

        let host = self.host.clone();
        let key_host = self.host.clone();
        let m = model.read();
        self.close.set_reason(m.close_warning());
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(BG)
            .color(INK)
            .font_size(TEXT)
            .content(Content::flex())
            .on_global_key_down(move |e: Event<KeyboardEventData>| key_down(model, &key_host, &e))
            .on_global_key_up(move |e: Event<KeyboardEventData>| key_up(model, &e))
            .child(top_bar(model, &m, &host))
            .maybe_child(m.notice.clone().map(|n| notice_bar(model, n)))
            .child(
                rect()
                    .horizontal()
                    .width(Size::fill())
                    .height(Size::flex(1.))
                    .content(Content::flex())
                    .child(sidebar(model, &m))
                    .child(
                        ScrollView::new().width(Size::flex(1.)).height(Size::fill()).child(
                            rect()
                                .width(Size::fill())
                                .padding(Gaps::new_all(PAD))
                                .spacing(GAP)
                                .child(page(model, &m, &host)),
                        ),
                    ),
            )
    }
}

fn page(model: State<Model>, m: &Model, host: &HostHandle) -> Element {
    match m.page {
        Page::Mapper => mapper::page(model, m).into_element(),
        Page::Devices => pages::devices(model, m, host).into_element(),
        Page::Profiles => pages::profiles(model, m, host).into_element(),
        Page::Hotkeys => pages::hotkeys(model, m, host).into_element(),
        Page::Tuning => pages::tuning(model, m, host).into_element(),
        Page::Colours => pages::colours(model, m).into_element(),
        Page::Firmware => pages::boards(model, m, host).into_element(),
        Page::Advanced => pages::advanced(model, m, host).into_element(),
    }
}

/// Keys typed on this computer: shortcuts being recorded, keys for the mapper, Escape, undo.
fn key_down(mut model: State<Model>, host: &HostHandle, e: &KeyboardEventData) {
    let usage = hid::usage_from_code_name(&e.code.to_string());
    let mut m = model.write();
    // A modifier released while the window had no focus never sends key-up; the event's own
    // modifier state corrects that.
    m.pc_mods &= held_mask(e.modifiers);
    if let Some(bit) = usage.and_then(hid::Modifier::from_usage).map(hid::Modifier::bit) {
        m.pc_mods |= bit;
    }
    let escape = e.code == Code::Escape;
    if m.hotkey_capture.is_some() {
        if escape {
            m.hotkey_capture = None;
        } else if let Some(u) = usage.filter(|u| !hid::MODIFIER_USAGES.contains(u)) {
            let mods = m.pc_mods;
            m.finish_hotkey(xd2joy_proto::Hotkey { key: u, mods });
            flush(&mut m, host);
        } else if usage.is_none() {
            // keep recording: Fn and media keys never reach the boards, so another key is needed
            m.error("That key never reaches the boards (Fn and media keys don't). Press another one, or Esc.");
        }
        return;
    }
    match m.editor.capture {
        Capture::Source(_) if escape => m.editor.cancel(),
        Capture::Source(_) => {
            let Some(u) = usage else {
                m.error("That key never reaches the boards (Fn and media keys don't). Press another one.");
                return;
            };
            m.capture_press(layout::key_source(u), true);
        }
        Capture::Target(_) if escape => m.editor.cancel(),
        _ if escape => m.notice = None,
        _ if m.page == Page::Mapper && e.modifiers.contains(Modifiers::CONTROL) && e.code == Code::KeyZ => {
            if e.modifiers.contains(Modifiers::SHIFT) {
                m.editor.redo();
            } else {
                m.editor.undo();
            }
        }
        _ if m.page == Page::Mapper && e.modifiers.contains(Modifiers::CONTROL) && e.code == Code::KeyY => {
            m.editor.redo();
        }
        _ => {}
    }
}

fn key_up(mut model: State<Model>, e: &KeyboardEventData) {
    if let Some(bit) =
        hid::usage_from_code_name(&e.code.to_string()).and_then(hid::Modifier::from_usage).map(hid::Modifier::bit)
    {
        let mut m = model.write();
        m.pc_mods &= !bit;
        let held = m.pc_mods;
        m.capture_release(held, true);
    }
}

/// The modifier bits (left and right) that may still be held, given the toolkit's modifier state.
fn held_mask(mods: Modifiers) -> u8 {
    // HID modifier bits: Ctrl 0x01/0x10, Shift 0x02/0x20, Alt 0x04/0x40, GUI 0x08/0x80
    let mut mask = 0u8;
    if mods.contains(Modifiers::CONTROL) {
        mask |= 0x11;
    }
    if mods.contains(Modifiers::SHIFT) {
        mask |= 0x22;
    }
    if mods.contains(Modifiers::ALT) {
        mask |= 0x44;
    }
    if mods.contains(Modifiers::META) {
        mask |= 0x88;
    }
    mask
}

fn top_bar(mut model: State<Model>, m: &Model, host: &HostHandle) -> impl IntoElement {
    let dirty: Vec<Side> = Side::BOTH.into_iter().filter(|s| m.editor.dirty(*s)).collect();
    let busy = m.busy();
    let changes = match dirty.as_slice() {
        [] => "No unapplied changes".to_owned(),
        [s] => format!("Unapplied changes on the {} board", crate::model::side_word(*s)),
        _ => "Unapplied changes on both boards".to_owned(),
    };
    let apply_host = host.clone();
    let save_note = dirty
        .iter()
        .find_map(|s| m.board(*s).profiles.active_saved())
        .map(|n| format!("Also saves profile {n} (active)"));
    rect()
        .horizontal()
        .width(Size::fill())
        .padding(Gaps::new(10., PAD, 10., PAD))
        .spacing(GAP)
        .cross_align(Alignment::Center)
        .content(Content::flex())
        .background(PANEL)
        .border(Border::new().fill(BORDER).width(1.))
        .child(label().text("XD2Joy").font_size(TITLE).font_weight(FontWeight::BOLD))
        .children(Side::BOTH.map(|s| board_pill(m, s).into_element()))
        .child(rect().width(Size::flex(1.)))
        .maybe_child(m.working.clone().map(|w| label().text(format!("{w}…")).color(INK_MUTED)))
        .child(Button::new().enabled(m.editor.can_undo()).on_press(move |_| model.write().editor.undo()).child("Undo"))
        .child(Button::new().enabled(m.editor.can_redo()).on_press(move |_| model.write().editor.redo()).child("Redo"))
        .child(
            rect()
                .spacing(2.)
                .child(label().text(changes).color(if dirty.is_empty() { INK_MUTED } else { WARN }))
                .maybe_child(save_note.map(|n| label().text(n).font_size(TEXT_SMALL).color(INK_MUTED))),
        )
        .child(
            Button::new().enabled(!dirty.is_empty()).on_press(move |_| model.write().editor.discard()).child("Discard"),
        )
        .child(
            Button::new()
                .enabled(!dirty.is_empty() && !busy)
                .on_press(move |_| {
                    let (fx, waiting) = {
                        let m = model.read();
                        (m.apply_effects(), m.waiting_for_board().first().and_then(|&s| m.why_not_writable(s)))
                    };
                    match fx {
                        Ok(fx) => {
                            fx.into_iter().for_each(|e| apply_host.send(e));
                            if let Some(why) = waiting {
                                model.write().info(why);
                            }
                        }
                        Err(e) => model.write().error(e),
                    }
                })
                .child("Apply"),
        )
}

fn board_pill(m: &Model, side: Side) -> impl IntoElement {
    let b = m.board(side);
    let (text, dot) = match &b.link {
        Link::Absent => ("not connected".to_owned(), INK_MUTED),
        Link::Failed { .. } => ("can't open".to_owned(), DANGER),
        Link::Connected { .. } if b.config.is_none() => ("reading…".to_owned(), WARN),
        Link::Connected { .. } => {
            let mode = b.mode.map_or("connected", |m| m.label());
            (mode.to_owned(), if b.warning.is_some() { WARN } else { OK })
        }
    };
    rect()
        .horizontal()
        .spacing(6.)
        .padding(Gaps::new(4., 10., 4., 10.))
        .corner_radius(12.)
        .background(BG)
        .cross_align(Alignment::Center)
        .child(rect().width(Size::px(8.)).height(Size::px(8.)).corner_radius(4.).background(dot))
        .child(
            label()
                .text(format!("{}: {text}", if side == Side::Left { "Left" } else { "Right" }))
                .font_size(TEXT_SMALL),
        )
}

fn notice_bar(mut model: State<Model>, n: Notice) -> impl IntoElement {
    let (text, color) = match n {
        Notice::Info(t) => (t, ACCENT),
        Notice::Error(t) => (t, DANGER),
    };
    rect()
        .horizontal()
        .width(Size::fill())
        .padding(Gaps::new(8., PAD, 8., PAD))
        .spacing(GAP)
        .cross_align(Alignment::Center)
        .background(color.with_a(40))
        .child(label().text(text).width(Size::flex(1.)))
        .child(Button::new().on_press(move |_| model.write().notice = None).child("Dismiss"))
}

fn sidebar(mut model: State<Model>, m: &Model) -> impl IntoElement {
    rect()
        .width(Size::px(170.))
        .height(Size::fill())
        .padding(Gaps::new_all(10.))
        .spacing(4.)
        .background(PANEL)
        .children(Page::ALL.map(|p| {
            let selected = m.page == p;
            rect()
                .key(p.label())
                .width(Size::fill())
                .padding(Gaps::new(8., 12., 8., 12.))
                .corner_radius(8.)
                .background(if selected { SELECT } else { Color::TRANSPARENT })
                .on_press(move |_| {
                    let mut m = model.write();
                    m.page = p;
                    m.editor.cancel();
                    m.hotkey_capture = None;
                    m.armed = None;
                })
                .child(label().text(p.label()).color(if selected { INK } else { INK_MUTED }))
                .into_element()
        }))
}

/// A card with a title.
pub(crate) fn panel(title: &str) -> Rect {
    rect()
        .width(Size::fill())
        .background(PANEL)
        .corner_radius(RADIUS)
        .border(Border::new().fill(BORDER).width(1.))
        .padding(Gaps::new_all(PAD))
        .spacing(10.)
        .child(label().text(title.to_owned()).font_size(HEADING).font_weight(FontWeight::BOLD))
}

pub(crate) fn muted(text: impl Into<String>) -> Label {
    label().text(text.into()).color(INK_MUTED).font_size(TEXT_SMALL)
}

pub(crate) fn row() -> Rect {
    rect().horizontal().spacing(8.).cross_align(Alignment::Center)
}

/// A button for something hard to undo: the first press arms it, the second does it.
pub(crate) fn danger_button(
    mut model: State<Model>,
    m: &Model,
    id: String,
    text: &str,
    enabled: bool,
    effect: Effect,
    host: &HostHandle,
) -> Button {
    let armed = m.armed.as_deref() == Some(id.as_str());
    let host = host.clone();
    Button::new()
        .enabled(enabled)
        .on_press(move |_| {
            if model.write().confirm(&id) {
                host.send(effect.clone());
            }
        })
        .child(if armed { "Press again to confirm".to_owned() } else { text.to_owned() })
}
