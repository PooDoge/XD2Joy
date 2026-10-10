//! The window. Components draw the `Model` and turn clicks into `Effect`s for the host; they make
//! no decisions of their own (those live in `model.rs`).

use std::sync::{Arc, Mutex};

use freya::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;
use xd2joy_proto::{InputSource, Mode, Side};

use crate::{
    host::HostHandle,
    model::{BoardView, Effect, FlashStage, HostEvent, Link, Model},
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
}

impl App for Root {
    fn render(&self) -> impl IntoElement {
        use_init_theme(dark_theme);
        let mut model = use_state(Model::default);
        use_hook({
            let inbox = self.inbox.clone();
            move || {
                if let Some(mut rx) = inbox.take() {
                    spawn(async move {
                        while let Some(event) = rx.next().await {
                            model.write().apply(event);
                        }
                    });
                }
            }
        });

        let m = model.read();
        let busy = m.busy();
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(BG)
            .color(INK)
            .font_size(TEXT)
            .padding(Gaps::new_all(PAD))
            .spacing(GAP)
            .content(Content::flex())
            .child(header())
            .child(rect().horizontal().width(Size::fill()).spacing(GAP).content(Content::flex()).children(
                Side::BOTH.map(|side| {
                    BoardCard { side, view: m.board(side).clone(), busy, host: self.host.clone(), key: DiffKey::None }
                        .key(side.letter())
                }),
            ))
            .maybe_child(m.flash.as_ref().map(flash_line))
            .child(log_panel(m.log.iter().cloned().collect()))
    }
}

fn header() -> impl IntoElement {
    rect()
        .spacing(4.)
        .child(label().text("XD2Joy").font_size(TITLE).font_weight(FontWeight::BOLD))
        .child(label().text("Plug both boards into this computer by USB. They connect by themselves.").color(INK_MUTED))
}

fn panel() -> Rect {
    rect()
        .background(PANEL)
        .corner_radius(RADIUS)
        .border(Border::new().fill(BORDER).width(1.))
        .padding(Gaps::new_all(PAD))
        .spacing(10.)
}

#[derive(Clone, PartialEq)]
struct BoardCard {
    side: Side,
    view: BoardView,
    busy: bool,
    host: HostHandle,
    key: DiffKey,
}

impl KeyExt for BoardCard {
    fn write_key(&mut self) -> &mut DiffKey {
        &mut self.key
    }
}

impl Component for BoardCard {
    fn render(&self) -> impl IntoElement {
        let side = self.side;
        let v = &self.view;
        let connected = v.connected();
        let accent = match side {
            Side::Left => LEFT,
            Side::Right => RIGHT,
        };

        let (status, status_color) = match &v.link {
            Link::Absent => ("Not connected".to_owned(), INK_MUTED),
            Link::Connected { port } => (format!("Connected on {port}"), OK),
            Link::Failed { port, error } => (format!("Could not open {port}: {error}"), DANGER),
        };

        let firmware = match &v.firmware {
            _ if !connected => None,
            crate::model::Firmware::Unknown => Some(("Checking firmware…".to_owned(), INK_MUTED)),
            crate::model::Firmware::Old => Some((
                "Older firmware. Update it once by hand (RST to GND twice); after that this app can do it.".to_owned(),
                WARN,
            )),
            crate::model::Firmware::Known(info) if v.firmware.newer_than_tool() => Some((
                format!("Firmware {} is newer than this app. Update the app before changing settings.", info.version),
                WARN,
            )),
            crate::model::Firmware::Known(info) => Some((format!("Firmware {}", info.version), INK_MUTED)),
        };

        let host = self.host.clone();
        let modes = SegmentedButton::new().children(Mode::ALL.map(|mode| {
            let host = host.clone();
            ButtonSegment::new()
                .key(mode.label())
                .selected(v.mode == Some(mode))
                .enabled(connected && !self.busy)
                .on_press(move |_| host.send(Effect::SetMode(side, mode)))
                .child(mode.label())
        }));

        let inputs = v.input.map(|status| {
            let host = host.clone();
            rect()
                .spacing(6.)
                .child(label().text("Input").color(INK_MUTED))
                .child(SegmentedButton::new().children(InputSource::ALL.map(|source| {
                    let host = host.clone();
                    ButtonSegment::new()
                        .key(source.label())
                        .selected(status.source == source)
                        .enabled(!self.busy)
                        .on_press(move |_| host.send(Effect::SetInput(side, source)))
                        .child(source.label())
                })))
                .child(
                    label()
                        .text(if status.wired_active { "A wired device is in use." } else { "No wired device in use." })
                        .font_size(TEXT_SMALL)
                        .color(INK_MUTED),
                )
        });

        let update = {
            let host = host.clone();
            Button::new()
                .enabled(!self.busy)
                .on_press(move |_| {
                    let host = host.clone();
                    spawn_forever(async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title(format!("Firmware for the {} board", side.label()))
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
                })
        };

        panel()
            .width(Size::flex(1.))
            .child(
                rect()
                    .horizontal()
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(rect().width(Size::px(10.)).height(Size::px(10.)).corner_radius(5.).background(accent))
                    .child(label().text(side.label()).font_size(HEADING).font_weight(FontWeight::BOLD)),
            )
            .child(label().text(status).color(status_color))
            .maybe_child(firmware.map(|(text, color)| label().text(text).color(color)))
            .maybe_child(v.warning.clone().map(|w| label().text(w).color(WARN)))
            .maybe_child(
                connected.then(|| rect().spacing(6.).child(label().text("Mode").color(INK_MUTED)).child(modes)),
            )
            .maybe_child(inputs)
            .child(update)
    }
}

fn flash_line(stage: &FlashStage) -> impl IntoElement {
    let color = match stage {
        FlashStage::Done { .. } => OK,
        FlashStage::Failed { .. } => DANGER,
        FlashStage::NeedsManualReset { .. } => WARN,
        _ => INK,
    };
    panel()
        .width(Size::fill())
        .child(label().text(format!("{}: {}", stage.side().label(), stage.describe())).color(color))
}

fn log_panel(lines: Vec<String>) -> impl IntoElement {
    panel().width(Size::fill()).height(Size::flex(1.)).child(label().text("Console").color(INK_MUTED)).child(
        ScrollView::new().width(Size::fill()).height(Size::fill()).children(
            lines
                .into_iter()
                .enumerate()
                .map(|(i, line)| label().key(i).text(line).font_size(TEXT).font_family("monospace")),
        ),
    )
}
