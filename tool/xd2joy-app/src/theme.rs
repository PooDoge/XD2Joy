//! Colours and spacing, in one place so the look can change without touching the components.
//! Dark only for now; the palette follows the web tool's.

use freya::prelude::Color;

pub const BG: Color = Color::from_rgb(24, 25, 28);
pub const PANEL: Color = Color::from_rgb(36, 38, 43);
pub const BORDER: Color = Color::from_rgb(58, 61, 68);
pub const INK: Color = Color::from_rgb(232, 233, 236);
pub const INK_MUTED: Color = Color::from_rgb(160, 164, 172);
pub const OK: Color = Color::from_rgb(110, 200, 140);
pub const WARN: Color = Color::from_rgb(240, 190, 90);
pub const DANGER: Color = Color::from_rgb(240, 110, 100);
/// The Switch 2 Joy-Con colours, for the side markers.
pub const LEFT: Color = Color::from_rgb(0, 174, 227);
pub const RIGHT: Color = Color::from_rgb(255, 62, 70);

pub const GAP: f32 = 12.;
pub const PAD: f32 = 16.;
pub const RADIUS: f32 = 10.;
pub const TEXT: f32 = 14.;
pub const TEXT_SMALL: f32 = 12.;
pub const TITLE: f32 = 22.;
pub const HEADING: f32 = 16.;
