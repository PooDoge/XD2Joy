//! The mapper: both Joy-Con halves with every button as a target, the keyboard and the mouse with
//! every key and button as a source. Pick one of each, in either order, to bind them.

use freya::prelude::*;
use xd2joy_proto::{
    Side, Source, StickSource, Target,
    hid::{self, WheelDir},
};

use super::layout::{self, Cap, Spot};
use super::{muted, panel, row};
use crate::{
    model::{Capture, Hover, Model, target_name},
    theme::*,
};

/// Key unit on the on-screen keyboard, in pixels, and the gap between keys.
const U: f32 = 34.;
const KEY_GAP: f32 = 4.;

/// What a render needs to colour targets and sources.
pub(super) struct Looks {
    keys: Vec<u8>,
    mods: u8,
    buttons: u8,
    capture: Capture,
    hover: Option<Hover>,
}

impl Looks {
    pub(super) fn of(m: &Model) -> Looks {
        Looks {
            keys: m.live.held_keys().collect(),
            mods: m.live.held_mods(),
            buttons: m.live.held_buttons(),
            capture: m.editor.capture,
            hover: m.hover,
        }
    }

    fn held(&self, s: Source) -> bool {
        layout::is_held(s, &self.keys, self.mods, self.buttons)
    }
}

pub fn page(model: State<Model>, m: &Model) -> impl IntoElement {
    let looks = Looks::of(m);
    rect()
        .width(Size::fill())
        .spacing(GAP)
        .child(header(model, m))
        .child(
            rect()
                .horizontal()
                .width(Size::fill())
                .spacing(GAP)
                .main_align(Alignment::Center)
                .child(target_list(model, m, Side::Left, &looks))
                .child(joycon(model, m, Side::Left, &looks))
                .child(joycon(model, m, Side::Right, &looks))
                .child(target_list(model, m, Side::Right, &looks)),
        )
        .child(
            rect()
                .horizontal()
                .width(Size::fill())
                .spacing(GAP)
                .main_align(Alignment::Center)
                .child(keyboard(model, m, &looks))
                .child(rect().spacing(GAP).child(mouse(model, m, &looks)).child(stick_sources(model, m))),
        )
}

fn header(mut model: State<Model>, m: &Model) -> impl IntoElement {
    let hint = match m.editor.capture {
        Capture::Idle => {
            "Click a Joy-Con button, then press the key or mouse button for it: on the Bluetooth device, on \
                          this computer's keyboard, or on the drawings below. Right-click to clear."
                .to_owned()
        }
        Capture::Source(t) => format!("Now press the key or mouse button for {}. Esc cancels.", target_name(t)),
        Capture::Target(s) => format!("Now click the Joy-Con button {} should press. Esc cancels.", s.label()),
    };
    let used = |s: Side| m.editor.edit.entries_used(s);
    let count = |s: Side| {
        let n = used(s);
        label()
            .text(format!("{} board: {n} of 32 entries", if s == Side::Left { "Left" } else { "Right" }))
            .font_size(TEXT_SMALL)
            .color(if n > 30 { WARN } else { INK_MUTED })
    };
    let add = m.add_mode;
    panel("Mapping")
        .child(label().text(hint).color(if m.editor.capture == Capture::Idle { INK } else { ACCENT }))
        .child(
            row()
                .child(muted("When an input is bound again it"))
                .child(SegmentedButton::new().children([
                    ButtonSegment::new()
                        .key("move")
                        .selected(!add)
                        .on_press(move |_| model.write().add_mode = false)
                        .child("moves")
                        .into_element(),
                    ButtonSegment::new()
                        .key("add")
                        .selected(add)
                        .on_press(move |_| model.write().add_mode = true)
                        .child("presses both")
                        .into_element(),
                ]))
                .child(rect().width(Size::px(16.)))
                .child(count(Side::Left))
                .child(count(Side::Right)),
        )
        .maybe_child((!m.editor.edit.has_home()).then(|| {
            label()
                .text("Nothing presses HOME. The right board wakes a sleeping Switch when HOME is held, so bind it to something.")
                .color(WARN)
        }))
}

/// The half's colour, for chips and tints.
fn half_colour(side: Side) -> Color {
    match side {
        Side::Left => LEFT,
        Side::Right => RIGHT,
    }
}

/// Short text for a source on a chip.
fn chip_text(s: Source) -> String {
    match s {
        Source::Key(u) => hid::key_label(u),
        Source::Modifiers(bits) => hid::modifiers_in(bits).map(|m| m.label()).collect::<Vec<_>>().join("/"),
        Source::Mouse(bits) => hid::mouse_buttons_in(bits).map(|b| b.short()).collect::<Vec<_>>().join("/"),
        Source::Wheel(WheelDir::Up) => "Wheel ▲".into(),
        Source::Wheel(WheelDir::Down) => "Wheel ▼".into(),
    }
}

fn target_list(mut model: State<Model>, m: &Model, half: Side, looks: &Looks) -> impl IntoElement {
    rect().width(Size::px(200.)).spacing(3.).children(layout::target_order(half).into_iter().map(|t| {
        let sources: Vec<Source> = m.editor.edit.sources_of(t).collect();
        let selected = looks.capture == Capture::Source(t);
        let hovered = looks.hover == Some(Hover::Target(t))
            || matches!(looks.hover, Some(Hover::Source(s)) if sources.contains(&s));
        let live = sources.iter().any(|s| looks.held(*s));
        rect()
            .key(format!("{t:?}"))
            .horizontal()
            .width(Size::fill())
            .padding(Gaps::new(3., 6., 3., 6.))
            .spacing(4.)
            .corner_radius(6.)
            .cross_align(Alignment::Center)
            .background(if selected {
                SELECT
            } else if hovered {
                BORDER
            } else {
                Color::TRANSPARENT
            })
            .on_press(move |_| pick_target(model, t))
            .on_secondary_down(move |_| model.write().editor.clear_target(t))
            .on_pointer_enter(move |_| model.write().hover = Some(Hover::Target(t)))
            .on_pointer_leave(move |_| leave(model, Hover::Target(t)))
            .child(label().text(t.label()).width(Size::px(70.)).font_size(TEXT_SMALL).color(if live {
                GLOW
            } else {
                INK
            }))
            .children(sources.into_iter().map(|s| chip(model, s, t).into_element()))
            .into_element()
    }))
}

fn chip(mut model: State<Model>, s: Source, t: Target) -> impl IntoElement {
    let over_link = s.board() != t.half();
    let text = if over_link { format!("{} ⇄", chip_text(s)) } else { chip_text(s) };
    rect()
        .key(format!("{s:?}"))
        .padding(Gaps::new(1., 6., 1., 6.))
        .corner_radius(8.)
        .background(half_colour(s.board()).with_a(70))
        .on_press(move |e: Event<PressEventData>| {
            // the chip removes its binding; the row behind it must not also start a pick
            e.stop_propagation();
            model.write().editor.unbind(xd2joy_proto::Binding { source: s, target: t });
        })
        .child(label().text(text).font_size(TEXT_SMALL))
}

fn pick_target(mut model: State<Model>, t: Target) {
    let mut m = model.write();
    let mode = m.bind_mode();
    match m.editor.pick_target(t, mode) {
        Ok(Some(text)) => m.info(text),
        Ok(None) => {}
        Err(text) => m.error(text),
    }
}

fn pick_source(mut model: State<Model>, s: Source) {
    let mut m = model.write();
    let mode = m.bind_mode();
    match m.editor.pick_source(s, mode) {
        Ok(Some(text)) => m.info(text),
        Ok(None) => {}
        Err(text) => m.error(text),
    }
}

fn leave(mut model: State<Model>, h: Hover) {
    if model.read().hover == Some(h) {
        model.write().hover = None;
    }
}

fn at(s: Spot) -> Rect {
    rect()
        .position(Position::new_absolute().left(s.x).top(s.y))
        .width(Size::px(s.w))
        .height(Size::px(s.h))
        .corner_radius(s.r)
        .center()
}

pub(super) fn joycon(mut model: State<Model>, m: &Model, half: Side, looks: &Looks) -> impl IntoElement {
    let (w, h) = layout::JOYCON_SIZE;
    let colours = m.editor.edit.colours[half.index()];
    let rgb = |c: xd2joy_proto::Rgb| Color::from_rgb(c.0[0], c.0[1], c.0[2]);
    let body = layout::BODY;
    // the outer side is rounded, the rail side is flat
    let (rail_x, radius) = match half {
        Side::Left => (body.x + body.w - 10., CornerRadius::new(56., 0., 0., 56.)),
        Side::Right => (body.x, CornerRadius::new(0., 56., 56., 0.)),
    };
    let spots = match half {
        Side::Left => layout::left_spots(),
        Side::Right => layout::right_spots(),
    };
    rect()
        .width(Size::px(w))
        .height(Size::px(h))
        .child(
            rect()
                .position(Position::new_absolute().left(body.x).top(body.y))
                .width(Size::px(body.w))
                .height(Size::px(body.h))
                .corner_radius(radius)
                .background(rgb(colours.body))
                .border(Border::new().fill(BORDER).width(1.)),
        )
        .child(
            rect()
                .position(Position::new_absolute().left(rail_x).top(body.y + 8.))
                .width(Size::px(10.))
                .height(Size::px(body.h - 16.))
                .corner_radius(4.)
                .background(rgb(colours.rail)),
        )
        .children(spots.into_iter().map(|(t, text, s)| {
            let sources: Vec<_> = m.editor.edit.sources_of(t).collect();
            let bound = !sources.is_empty();
            let live = sources.iter().any(|s| looks.held(*s));
            let selected = looks.capture == Capture::Source(t);
            let related = looks.hover == Some(Hover::Target(t))
                || matches!(looks.hover, Some(Hover::Source(src)) if sources.contains(&src));
            let fill = if live {
                GLOW
            } else if selected {
                ACCENT
            } else if bound {
                rgb(colours.buttons)
            } else {
                JC_BUTTON
            };
            let stick = matches!(
                t,
                Target::Button(xd2joy_proto::Button::LeftStickPress | xd2joy_proto::Button::RightStickPress)
            );
            at(s)
                .key(format!("{t:?}"))
                .background(if stick && !live && !selected { rgb(colours.stick) } else { fill })
                .border(Border::new().fill(if related { ACCENT } else { BORDER }).width(if related { 2. } else { 1. }))
                .on_press(move |_| pick_target(model, t))
                .on_secondary_down(move |_| model.write().editor.clear_target(t))
                .on_pointer_enter(move |_| model.write().hover = Some(Hover::Target(t)))
                .on_pointer_leave(move |_| leave(model, Hover::Target(t)))
                .child(label().text(text).font_size(TEXT_SMALL).color(if bound || live || selected {
                    Color::BLACK
                } else {
                    INK
                }))
                .into_element()
        }))
}

fn keycap(model: State<Model>, m: &Model, looks: &Looks, cap: Cap) -> Element {
    let Cap::Key { usage, label: text, w } = cap else {
        let Cap::Gap(g) = cap else { return rect().into_element() };
        return rect().width(Size::px(g * U + (g - 1.).max(0.) * KEY_GAP)).height(Size::px(U)).into_element();
    };
    let source = layout::key_source(usage);
    source_cap(model, m, looks, source, text, Size::px(w * U + (w - 1.) * KEY_GAP), Size::px(U), 6.).into_element()
}

/// A key or mouse button: colour by what it is bound to, glow when held.
#[allow(clippy::too_many_arguments)]
fn source_cap(
    mut model: State<Model>,
    m: &Model,
    looks: &Looks,
    source: Source,
    text: &'static str,
    width: Size,
    height: Size,
    radius: f32,
) -> Rect {
    let targets: Vec<Target> = m.editor.edit.targets_of(source).collect();
    let live = looks.held(source);
    let selected = looks.capture == Capture::Target(source);
    let related = looks.hover == Some(Hover::Source(source))
        || matches!(looks.hover, Some(Hover::Target(t)) if targets.contains(&t));
    let fill = if live {
        GLOW
    } else if selected {
        ACCENT
    } else if let Some(t) = targets.first() {
        half_colour(t.half()).with_a(150)
    } else {
        KEY
    };
    rect()
        .key(format!("{source:?}"))
        .width(width)
        .height(height)
        .corner_radius(radius)
        .center()
        .background(fill)
        .border(Border::new().fill(if related { ACCENT } else { BORDER }).width(if related { 2. } else { 1. }))
        .on_press(move |_| pick_source(model, source))
        .on_secondary_down(move |_| {
            let _ = model.write().editor.change(|map| {
                map.bindings.retain(|b| b.source != source);
                Ok::<(), ()>(())
            });
        })
        .on_pointer_enter(move |_| model.write().hover = Some(Hover::Source(source)))
        .on_pointer_leave(move |_| leave(model, Hover::Source(source)))
        .child(label().text(text).font_size(TEXT_SMALL).color(if live || selected { Color::BLACK } else { INK }))
}

fn key_block(model: State<Model>, m: &Model, looks: &Looks, rows: &[&[Cap]; 6]) -> impl IntoElement {
    rect().spacing(KEY_GAP).children(rows.iter().enumerate().map(|(i, r)| {
        rect()
            .key(i)
            .horizontal()
            .height(Size::px(U))
            .spacing(KEY_GAP)
            .children(r.iter().map(|c| keycap(model, m, looks, *c)))
            .into_element()
    }))
}

fn keyboard(model: State<Model>, m: &Model, looks: &Looks) -> impl IntoElement {
    let unreadable = m.live.unreadable_reports();
    panel("Keyboard (left board)")
        .width(Size::auto())
        .child(
            rect()
                .horizontal()
                .spacing(14.)
                .child(key_block(model, m, looks, &layout::MAIN))
                .child(key_block(model, m, looks, &layout::NAV))
                .child(key_block(model, m, looks, &layout::NUMPAD)),
        )
        .maybe_child((unreadable > 0).then(|| {
            muted(
                "The keyboard also sent reports the board can't use (media keys). For F1-F12 on a compact keyboard, \
                 hold Fn.",
            )
        }))
}

fn mouse(model: State<Model>, m: &Model, looks: &Looks) -> impl IntoElement {
    let (w, h) = layout::MOUSE_SIZE;
    // the buttons are children of the body so they are drawn over it; positions are relative to
    // the drawing, which starts 10 px left of the body
    panel("Mouse (right board)").width(Size::auto()).child(
        rect().width(Size::px(w)).height(Size::px(h)).padding(Gaps::new(0., 10., 0., 10.)).child(
            rect()
                .width(Size::fill())
                .height(Size::fill())
                .corner_radius(CornerRadius::new(60., 60., 70., 70.))
                .background(JC_BODY)
                .border(Border::new().fill(BORDER).width(1.))
                .children(layout::mouse_spots().into_iter().map(|(source, text, s)| {
                    source_cap(model, m, looks, source, text, Size::px(s.w), Size::px(s.h), s.r)
                        .position(Position::new_absolute().left(s.x - 10.).top(s.y))
                        .into_element()
                })),
        ),
    )
}

fn stick_sources(mut model: State<Model>, m: &Model) -> impl IntoElement {
    let current = m.editor.edit.right_stick;
    let axis = |i: usize, name: &'static str| {
        rect().spacing(4.).child(muted(name)).child(SegmentedButton::new().children(StickSource::ALL.map(|src| {
            ButtonSegment::new()
                .key(src.label())
                .selected(current[i] == src)
                .on_press(move |_| {
                    let _ = model.write().editor.change(|map| {
                        map.right_stick[i] = src;
                        Ok::<(), ()>(())
                    });
                })
                .child(match src {
                    StickSource::None => "—",
                    StickSource::MouseX => "Mouse ↔",
                    StickSource::MouseY => "Mouse ↕",
                    StickSource::Wheel => "Wheel",
                })
                .into_element()
        })))
    };
    panel("Right stick in stick mode")
        .width(Size::auto())
        .child(axis(0, "Left and right"))
        .child(axis(1, "Up and down"))
}
