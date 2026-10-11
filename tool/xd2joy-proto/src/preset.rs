//! Preset files in the web tool's format (`joycon2_preset.json`), read and written so presets move
//! between the two tools.
//!
//! The web tool keeps one keyboard and one mouse binding per Joy-Con button. This tool allows any
//! number, so the extra ones go under keys like `L|ZL#2`; the web tool skips keys it does not know.
//! Entries the tool keeps but cannot show go under an `xd2joy` field, which the web tool ignores.

use serde_json::{Map, Value, json};

use crate::Mode;
use crate::Side;
use crate::blob::{Colours, Entry, Rgb};
use crate::hid::{self, WheelDir};
use crate::joycon::{Button, StickDir, Target};
use crate::mapping::{Binding, Hotkey, MOUSE_GAIN, Mapping, STICK_DEADZONE, STICK_SENS, StickSource};

pub const FILE_NAME: &str = "joycon2_preset.json";

/// What a preset carries besides the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresetExtras {
    /// The console unpair key; `Some(None)` means the preset clears it.
    pub sync_key: Option<Option<Hotkey>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresetError {
    #[error("the file is not JSON: {0}")]
    Json(String),
    #[error("the file is not a preset (it has no bindings)")]
    NotAPreset,
}

/// The web tool's slider limits; a value outside them is ignored on import, as the web tool does.
const PRESET_STICK_SPEED_MAX: u16 = 20;
const PRESET_DEADZONE_MAX: u16 = 40;

/// Builds the preset file for a mapping.
pub fn to_json(m: &Mapping, sync_key: Option<Hotkey>) -> String {
    let mut bind = Map::new();
    let mut used: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for b in &m.bindings {
        let Some((key, rec)) = binding_record(*b) else { continue };
        let n = used.entry(key.clone()).or_insert(0);
        *n += 1;
        let key = if *n == 1 { key } else { format!("{key}#{n}") };
        bind.insert(key, rec);
    }
    for (axis, src) in ["SX", "SY"].into_iter().zip(m.right_stick) {
        if let Some(name) = source_name(src) {
            bind.insert(
                axis.to_owned(),
                json!({"kind": "src", "src": name, "text": src.label(), "byteIdx": -1, "mask": 0, "code": 0, "handle": 0, "len": 0}),
            );
        }
    }
    let colours = |c: &Colours| json!({"body": c.body.to_string(), "btn": c.buttons.to_string(), "rail": c.rail.to_string(), "stick": c.stick.to_string()});
    let mode = |mode: Mode| match m.mode_key(mode) {
        Some(k) => json!({"code": k.key, "mods": k.mods, "mode": mode.firmware_id()}),
        None => Value::Null,
    };
    let kept = |side: Side| -> Vec<Value> {
        m.kept[side.index()].entries.iter().map(|e| json!({"code": e.code, "kind": e.kind, "mask": e.mask})).collect()
    };
    let t = &m.tuning;
    let mut o = json!({
        "version": 3,
        "side": "L",
        "bind": bind,
        "colorsBySide": {"L": colours(&m.colours[0]), "R": colours(&m.colours[1])},
        "modes": {"0": mode(Mode::Joycon), "1": mode(Mode::Keyboard), "2": mode(Mode::Stick)},
        "stickSens": t.stick_sens,
        // the web tool shows the dead zone divided by 10; this tool's files keep its full value too
        "stickDz": (t.stick_deadzone + 5) / 10,
        "stickVg": t.stick_speed,
        "mouseSens": t.mouse_gain,
        "mouseAccel": t.accel,
        "xd2joy": {"stickDeadzone": t.stick_deadzone, "kept": {"L": kept(Side::Left), "R": kept(Side::Right)}},
    });
    if let (Some(k), Some(obj)) = (sync_key, o.as_object_mut()) {
        // the web tool's pairing key takes no modifiers
        obj.insert("sync".to_owned(), json!({"code": k.key, "mods": 0}));
    }
    serde_json::to_string_pretty(&o).unwrap_or_default()
}

/// Applies a preset over `m`, as the web tool does: bindings and right-stick sources are replaced;
/// colours, mode hotkeys and settings are replaced only when the file has valid values for them.
/// Unlike the web tool, a file without `mouseAccel` leaves acceleration as it was.
pub fn apply(json: &str, m: &mut Mapping) -> Result<PresetExtras, PresetError> {
    let o: Value = serde_json::from_str(json).map_err(|e| PresetError::Json(e.to_string()))?;
    let bind = o.get("bind").and_then(Value::as_object).ok_or(PresetError::NotAPreset)?;

    m.bindings.clear();
    m.right_stick = [StickSource::None; 2];
    for (key, rec) in bind {
        match key.as_str() {
            "SX" => m.right_stick[0] = record_source(rec),
            "SY" => m.right_stick[1] = record_source(rec),
            _ => {
                if let Some(b) = record_binding(key, rec)
                    && !m.bindings.contains(&b)
                {
                    m.bindings.push(b);
                }
            }
        }
    }

    if let Some(cbs) = o.get("colorsBySide") {
        for side in Side::BOTH {
            let letter = side.letter().to_string();
            if let Some(c) = cbs.get(&letter).and_then(parse_colours) {
                m.colours[side.index()] = c;
            }
        }
    }

    if let Some(modes) = o.get("modes").and_then(Value::as_object) {
        for mode in Mode::ALL {
            let Some(v) = modes.get(&mode.firmware_id().to_string()) else { continue };
            if v.is_null() {
                m.set_mode_key(mode, None);
            } else if let (Some(code), Some(mods)) = (num_u8(v.get("code")), num_u8(v.get("mods")))
                && num_u8(v.get("mode")) == Some(mode.firmware_id())
            {
                m.set_mode_key(mode, Some(Hotkey { key: code, mods }));
            }
        }
    }

    let num = |k: &str| o.get(k).and_then(Value::as_f64).map(|v| v.round());
    let t = &mut m.tuning;
    if let Some(v) = num("stickSens").filter(|v| in_range(*v, *STICK_SENS.start(), *STICK_SENS.end())) {
        t.stick_sens = v as u16;
    }
    let full_dz = o.pointer("/xd2joy/stickDeadzone").and_then(Value::as_f64);
    if let Some(v) = full_dz.filter(|v| in_range(*v, *STICK_DEADZONE.start(), *STICK_DEADZONE.end())) {
        t.stick_deadzone = v as u16;
    } else if let Some(v) = num("stickDz").filter(|v| in_range(*v, 0, PRESET_DEADZONE_MAX)) {
        t.stick_deadzone = v as u16 * 10;
    }
    if let Some(v) = num("stickVg").filter(|v| in_range(*v, 1, PRESET_STICK_SPEED_MAX)) {
        t.stick_speed = v as u16;
    }
    if let Some(v) = num("mouseSens").filter(|v| in_range(*v, *MOUSE_GAIN.start(), *MOUSE_GAIN.end())) {
        t.mouse_gain = v as u16;
    }
    if let Some(a) = o.get("mouseAccel").and_then(Value::as_bool) {
        t.accel = a;
    }

    if let Some(kept) = o.pointer("/xd2joy/kept") {
        for side in Side::BOTH {
            if let Some(list) = kept.get(side.letter().to_string()).and_then(Value::as_array) {
                m.kept[side.index()].entries = list
                    .iter()
                    .filter_map(|e| {
                        Some(Entry {
                            code: num_u8(e.get("code"))?,
                            kind: num_u8(e.get("kind"))?,
                            mask: num_u16(e.get("mask"))?,
                        })
                    })
                    .collect();
            }
        }
    }

    let sync_key = o
        .get("sync")
        .and_then(|s| num_u8(s.get("code")))
        .map(|code| (code != 0).then_some(Hotkey { key: code, mods: 0 }));
    Ok(PresetExtras { sync_key })
}

fn in_range(v: f64, lo: u16, hi: u16) -> bool {
    v >= f64::from(lo) && v <= f64::from(hi)
}

fn num_u8(v: Option<&Value>) -> Option<u8> {
    v?.as_u64().and_then(|n| u8::try_from(n).ok())
}

fn num_u16(v: Option<&Value>) -> Option<u16> {
    v?.as_u64().and_then(|n| u16::try_from(n).ok())
}

fn source_name(s: StickSource) -> Option<&'static str> {
    match s {
        StickSource::None => None,
        StickSource::MouseX => Some("mouse_dx"),
        StickSource::MouseY => Some("mouse_dy"),
        StickSource::Wheel => Some("mouse_wheel"),
    }
}

fn record_source(rec: &Value) -> StickSource {
    match rec.get("src").and_then(Value::as_str) {
        Some("mouse_dx") => StickSource::MouseX,
        Some("mouse_dy") => StickSource::MouseY,
        Some("mouse_wheel") => StickSource::Wheel,
        _ => StickSource::None,
    }
}

fn parse_colours(v: &Value) -> Option<Colours> {
    let c = |k: &str| v.get(k).and_then(Value::as_str).and_then(Rgb::parse);
    Some(Colours { body: c("body")?, buttons: c("btn")?, rail: c("rail")?, stick: c("stick")? })
}

/// `L|ZL`, `L|R:A`, `R|L:UP`, `L|STICK_UP`: the board letter, then the target, prefixed with the
/// other half's letter when the target is on the other half.
fn binding_record(b: Binding) -> Option<(String, Value)> {
    let board = b.source.board();
    let target = match b.target {
        Target::Stick(d) => d.preset_id().to_owned(),
        Target::Button(btn) if btn.half() == board => btn.preset_id().to_owned(),
        Target::Button(btn) => format!("{}:{}", btn.half().letter(), btn.preset_id()),
    };
    let key = format!("{}|{target}", board.letter());
    let text = b.source.label();
    let rec = match b.source {
        crate::mapping::Source::Key(code) => {
            json!({"kind": "key", "code": code, "mask": 0, "byteIdx": 1, "handle": 0, "len": 0, "text": text})
        }
        crate::mapping::Source::Modifiers(bits) => {
            json!({"kind": "mod", "code": 0, "mask": bits, "byteIdx": 0, "handle": 0, "len": 0, "text": text})
        }
        crate::mapping::Source::Mouse(bits) => {
            json!({"kind": "bit", "code": 0, "mask": bits, "byteIdx": 0, "handle": 0, "len": 0, "text": text})
        }
        crate::mapping::Source::Wheel(w) => {
            json!({"kind": "wheel", "code": w.code(), "mask": 0, "byteIdx": -1, "handle": 0, "len": 0, "text": text})
        }
    };
    Some((key, rec))
}

fn record_binding(key: &str, rec: &Value) -> Option<Binding> {
    use crate::mapping::Source;
    let key = key.split('#').next()?;
    let (board, target) = key.split_once('|')?;
    let board = Side::from_letter(board)?;
    let target = if let Some(d) = StickDir::from_preset_id(target) {
        Target::Stick(d)
    } else if let Some((half, id)) = target.split_once(':') {
        Target::Button(Button::from_preset_id(Side::from_letter(half)?, id)?)
    } else {
        // GL and GR belong to their half whatever board the binding is on
        let half = match target {
            "GL" => Side::Left,
            "GR" => Side::Right,
            _ => board,
        };
        Target::Button(Button::from_preset_id(half, target)?)
    };
    let source = match rec.get("kind").and_then(Value::as_str)? {
        "key" => Source::Key(num_u8(rec.get("code"))?),
        "mod" => Source::Modifiers(num_u8(rec.get("mask"))?),
        "bit" => Source::Mouse(num_u8(rec.get("mask"))?),
        "wheel" => Source::Wheel(WheelDir::from_code(num_u8(rec.get("code"))?)?),
        _ => return None,
    };
    // A keyboard record on the mouse board (or the reverse) does not exist in the firmware.
    if source.board() != board {
        return None;
    }
    // Keys in the modifier range belong in the modifier byte.
    if let Source::Key(u) = source
        && hid::MODIFIER_USAGES.contains(&u)
    {
        return None;
    }
    Binding::new(source, target).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::MouseButton;
    use crate::mapping::{Source, Tuning};

    /// A preset as the web tool writes it (trimmed to the fields that matter).
    const WEB_PRESET: &str = r##"{
      "version": 3, "side": "R",
      "bind": {
        "L|ZL": {"kind":"key","code":8,"mask":0,"byteIdx":1,"handle":0,"len":0,"text":"E"},
        "L|LS": {"kind":"mod","code":0,"mask":2,"text":"L Shift"},
        "L|R:A": {"kind":"key","code":9,"mask":0,"text":"F"},
        "L|STICK_UP": {"kind":"key","code":26,"mask":0,"text":"W"},
        "R|ZR": {"kind":"bit","code":0,"mask":2,"text":"Right click"},
        "R|L:UP": {"kind":"wheel","code":1,"mask":0,"text":"Scroll up"},
        "R|GL": {"kind":"bit","code":0,"mask":8,"text":"Side button 1"},
        "L|GR": {"kind":"key","code":20,"mask":0,"text":"Q"},
        "SY": {"kind":"src","src":"mouse_wheel"},
        "L|NOPE": {"kind":"key","code":4}
      },
      "colorsBySide": {"L": {"body":"#323232","btn":"#AAAAAA","rail":"#9BE1E6","stick":"#323232"}, "R": null},
      "modes": {"1": {"code":14,"mods":1,"mode":1}, "2": null},
      "stickSens": 7, "stickDz": 30, "stickVg": 9, "mouseSens": 500,
      "sync": {"code":69,"mods":0}
    }"##;

    #[test]
    fn reads_a_web_tool_preset() {
        let mut m = Mapping::firmware_default();
        m.tuning.accel = true;
        let extras = apply(WEB_PRESET, &mut m).expect("reads");
        let has = |s, t| m.bindings.contains(&Binding { source: s, target: t });
        assert!(has(Source::Key(8), Target::Button(Button::Zl)));
        assert!(has(Source::Modifiers(2), Target::Button(Button::LeftStickPress)));
        assert!(has(Source::Key(9), Target::Button(Button::A)));
        assert!(has(Source::Key(26), Target::Stick(StickDir::Up)));
        assert!(has(Source::mouse(MouseButton::Right), Target::Button(Button::Zr)));
        assert!(has(Source::Wheel(WheelDir::Up), Target::Button(Button::Up)));
        assert!(has(Source::mouse(MouseButton::Back), Target::Button(Button::Gl)));
        assert!(has(Source::Key(20), Target::Button(Button::Gr)));
        assert_eq!(m.bindings.len(), 8);
        assert_eq!(m.right_stick, [StickSource::None, StickSource::Wheel]);
        assert_eq!(m.colours[0], Colours::stock(Side::Left));
        assert_eq!(m.colours[1], Colours::FIRMWARE_DEFAULT);
        assert_eq!(m.mode_key(Mode::Keyboard), Some(Hotkey { key: 14, mods: 1 }));
        assert_eq!(m.mode_key(Mode::Stick), None);
        assert_eq!(
            m.tuning,
            Tuning { stick_sens: 7, mouse_gain: 500, accel: true, stick_deadzone: 300, stick_speed: 9 }
        );
        assert_eq!(extras.sync_key, Some(Some(Hotkey { key: 0x45, mods: 0 })));
    }

    #[test]
    fn round_trips_through_its_own_files() {
        let mut m = Mapping::firmware_default();
        // a second source on B and a second target for Space: both survive
        m.bindings.push(Binding { source: Source::mouse(MouseButton::Forward), target: Target::Button(Button::B) });
        m.bindings.push(Binding { source: Source::Key(0x2C), target: Target::Button(Button::A) });
        m.tuning.stick_deadzone = 123;
        m.kept[1].entries.push(Entry { code: 3, kind: 9, mask: 1 });
        let file = to_json(&m, Some(Hotkey { key: 0x45, mods: 0 }));
        let mut back = Mapping::firmware_default();
        back.bindings.clear();
        let extras = apply(&file, &mut back).expect("reads");
        for side in Side::BOTH {
            assert!(back.same_on_board(&m, side), "{side:?}");
        }
        assert_eq!(extras.sync_key, Some(Some(Hotkey { key: 0x45, mods: 0 })));
    }

    #[test]
    fn rejects_non_presets() {
        let mut m = Mapping::firmware_default();
        assert!(matches!(apply("not json", &mut m), Err(PresetError::Json(_))));
        assert_eq!(apply("{\"version\":3}", &mut m), Err(PresetError::NotAPreset));
        assert_eq!(m, Mapping::firmware_default());
    }

    #[test]
    fn out_of_range_values_are_ignored() {
        let mut m = Mapping::firmware_default();
        let before = m.tuning;
        apply(r#"{"bind":{},"stickSens":0,"stickDz":41,"stickVg":21,"mouseSens":5,"mouseAccel":"yes"}"#, &mut m)
            .expect("reads");
        assert_eq!(m.tuning, before);
        assert!(m.bindings.is_empty());
    }

    #[test]
    fn mismatched_records_are_skipped() {
        let mut m = Mapping::firmware_default();
        // a mouse record under the keyboard board, a modifier usage as a key, a mouse on the stick
        let json = r#"{"bind":{
            "L|ZL":{"kind":"bit","mask":1},
            "L|L":{"kind":"key","code":225},
            "R|STICK_UP":{"kind":"bit","mask":1}}}"#;
        apply(json, &mut m).expect("reads");
        assert!(m.bindings.is_empty());
    }
}
