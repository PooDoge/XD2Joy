//! Parsing the firmware's reply lines. The firmware also prints free-form log lines, so anything
//! without a known tag is kept as `Reply::Other` for the log rather than treated as an error.

use crate::{InputSource, Mode, Side};

/// What `ver` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    /// `git describe` of the build, or `dev`.
    pub version: String,
    pub proto: u32,
    pub side: Side,
}

/// What `input` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputStatus {
    pub source: InputSource,
    /// A wired device is feeding this board right now.
    pub wired_active: bool,
    /// Bit mask of the devices the bridge has announced.
    pub bridge_devices: u8,
    pub frames: u32,
    pub bad_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Version(FirmwareInfo),
    /// The board is about to restart into its bootloader.
    Bootloader,
    /// `changed` is true for `[MODE] now X`, the line printed when a switch happened.
    Mode {
        mode: Mode,
        changed: bool,
    },
    Input(InputStatus),
    /// Any other line: a tag the tool does not read yet, or a log line.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("the {tag} line has a field the tool does not understand: {line:?}")]
    BadField { tag: &'static str, line: String },
}

/// Read one line. A known tag with a body that does not parse is an error (the firmware and the
/// tool disagree about the protocol, which the caller should log); an unknown tag is `Other`.
pub fn parse_line(line: &str) -> Result<Reply, ParseError> {
    let line = line.trim();
    let Some((tag, body)) = split_tag(line) else {
        return Ok(Reply::Other(line.to_owned()));
    };
    let bad = |tag: &'static str| ParseError::BadField { tag, line: line.to_owned() };
    match tag {
        "VER" => parse_ver(body).map(Reply::Version).ok_or_else(|| bad("VER")),
        "BOOT" => Ok(Reply::Bootloader),
        "MODE" => parse_mode(body).ok_or_else(|| bad("MODE")),
        "INPUT" => parse_input(body).map(Reply::Input).ok_or_else(|| bad("INPUT")),
        _ => Ok(Reply::Other(line.to_owned())),
    }
}

fn split_tag(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let (tag, body) = rest.split_once(']')?;
    Some((tag, body.trim()))
}

/// `fw <version> proto <n> side <L|R>`
fn parse_ver(body: &str) -> Option<FirmwareInfo> {
    let mut words = body.split_whitespace();
    let mut version = None;
    let mut proto = None;
    let mut side = None;
    while let Some(key) = words.next() {
        let value = words.next()?;
        match key {
            "fw" => version = Some(value.to_owned()),
            "proto" => proto = value.parse().ok(),
            "side" => side = Side::from_letter(value),
            // later firmware may add fields; they are not an error
            _ => {}
        }
    }
    Some(FirmwareInfo { version: version?, proto: proto?, side: side? })
}

/// `KEYBOARD`, `already KEYBOARD` or `now KEYBOARD, joy-con input ...`
fn parse_mode(body: &str) -> Option<Reply> {
    let (changed, rest) = match body.split_once(' ') {
        Some(("now", rest)) => (true, rest),
        Some(("already", rest)) => (false, rest),
        _ => (false, body),
    };
    let word = rest.split([',', ' ']).next()?;
    Mode::from_reply(word).map(|mode| Reply::Mode { mode, changed })
}

/// `src=auto wired=none bridge=0x00 frames=0 bad=0`
fn parse_input(body: &str) -> Option<InputStatus> {
    let mut source = None;
    let mut wired_active = None;
    let mut bridge_devices = None;
    let mut frames = None;
    let mut bad_frames = None;
    for pair in body.split_whitespace() {
        let (key, value) = pair.split_once('=')?;
        match key {
            "src" => source = InputSource::from_word(value),
            "wired" => wired_active = Some(value == "active"),
            "bridge" => bridge_devices = u8::from_str_radix(value.trim_start_matches("0x"), 16).ok(),
            "frames" => frames = value.parse().ok(),
            "bad" => bad_frames = value.parse().ok(),
            _ => {}
        }
    }
    Some(InputStatus {
        source: source?,
        wired_active: wired_active?,
        bridge_devices: bridge_devices?,
        frames: frames?,
        bad_frames: bad_frames?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line() {
        assert_eq!(
            parse_line("[VER] fw v1.2-3-gabc proto 1 side L"),
            Ok(Reply::Version(FirmwareInfo { version: "v1.2-3-gabc".into(), proto: 1, side: Side::Left }))
        );
        // fields added later are skipped
        assert!(matches!(parse_line("[VER] fw dev proto 2 side R bridge yes"), Ok(Reply::Version(_))));
        assert!(parse_line("[VER] fw dev side R").is_err());
        assert!(parse_line("[VER] fw dev proto x side R").is_err());
    }

    #[test]
    fn mode_lines_as_the_firmware_prints_them() {
        let m = |mode, changed| Ok(Reply::Mode { mode, changed });
        assert_eq!(parse_line("[MODE] KEYBOARD"), m(Mode::Keyboard, false));
        assert_eq!(parse_line("[MODE] already STICK"), m(Mode::Stick, false));
        assert_eq!(parse_line("[MODE] now JOYCON, joy-con input on"), m(Mode::Joycon, true));
        assert!(parse_line("[MODE] SIDEWAYS").is_err());
    }

    #[test]
    fn input_line() {
        assert_eq!(
            parse_line("[INPUT] src=usb wired=active bridge=0x03 frames=1200 bad=2"),
            Ok(Reply::Input(InputStatus {
                source: InputSource::Usb,
                wired_active: true,
                bridge_devices: 3,
                frames: 1200,
                bad_frames: 2,
            }))
        );
        assert!(parse_line("[INPUT] src=usb").is_err());
    }

    #[test]
    fn everything_else_is_kept_for_the_log() {
        assert_eq!(parse_line("[PROF] active 1 of 4"), Ok(Reply::Other("[PROF] active 1 of 4".into())));
        assert_eq!(parse_line("booting"), Ok(Reply::Other("booting".into())));
        assert_eq!(parse_line("[BOOT] restarting into the UF2 bootloader"), Ok(Reply::Bootloader));
    }
}
