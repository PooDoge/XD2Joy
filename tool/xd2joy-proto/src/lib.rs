//! The XD2Joy console protocol, with no I/O: which USB ids belong to which board, the commands the
//! tool sends, and the reply lines the firmware prints (`[TAG] text`, one per line).
//!
//! Files: `lib.rs` (board identity, commands), `reply.rs` (parsing reply lines), `lines.rs` (splitting
//! a byte stream into lines).

mod lines;
mod reply;

pub use lines::LineSplitter;
pub use reply::{FirmwareInfo, InputStatus, ParseError, Reply, parse_line};

/// The protocol version this tool speaks. The firmware reports its own in `[VER] ... proto N`; a
/// board without a `ver` command (firmware from before the tool existed) has none.
pub const PROTO_VERSION: u32 = 1;

/// Zephyr's default USB vendor id, which the firmware keeps.
pub const USB_VID: u16 = 0x2FE3;
pub const USB_PID_LEFT: u16 = 0x0005;
pub const USB_PID_RIGHT: u16 = 0x0004;

/// The baud rate the web tool uses. CDC ACM ignores it, but some host drivers want one set.
pub const BAUD: u32 = 115_200;

/// Which Joy-Con a board plays. Left takes the keyboard, right takes the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const BOTH: [Side; 2] = [Side::Left, Side::Right];

    /// The side a USB device is, from its ids; `None` for anything that is not an XD2Joy board.
    pub fn from_usb(vid: u16, pid: u16) -> Option<Side> {
        match (vid, pid) {
            (USB_VID, USB_PID_LEFT) => Some(Side::Left),
            (USB_VID, USB_PID_RIGHT) => Some(Side::Right),
            _ => None,
        }
    }

    /// The letter the firmware prints for this side in `[VER]`.
    pub fn letter(self) -> char {
        match self {
            Side::Left => 'L',
            Side::Right => 'R',
        }
    }

    pub fn from_letter(c: &str) -> Option<Side> {
        match c {
            "L" => Some(Side::Left),
            "R" => Some(Side::Right),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "Left (keyboard)",
            Side::Right => "Right (mouse)",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// What the board pretends to be to the console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Joycon,
    Stick,
    Keyboard,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Joycon, Mode::Stick, Mode::Keyboard];

    fn word(self) -> &'static str {
        match self {
            Mode::Joycon => "joycon",
            Mode::Stick => "stick",
            Mode::Keyboard => "keyboard",
        }
    }

    /// The firmware prints modes in capitals (`[MODE] KEYBOARD`).
    pub fn from_reply(s: &str) -> Option<Mode> {
        match s {
            "JOYCON" => Some(Mode::Joycon),
            "STICK" => Some(Mode::Stick),
            "KEYBOARD" => Some(Mode::Keyboard),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Joycon => "Joy-Con",
            Mode::Stick => "Stick",
            Mode::Keyboard => "Keyboard",
        }
    }
}

/// Where the board takes its keyboard or mouse from (`input` command, needs the USB bridge firmware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    /// Wired when the bridge has a device for this side, Bluetooth otherwise.
    Auto,
    Bluetooth,
    Usb,
}

impl InputSource {
    pub const ALL: [InputSource; 3] = [InputSource::Auto, InputSource::Bluetooth, InputSource::Usb];

    fn word(self) -> &'static str {
        match self {
            InputSource::Auto => "auto",
            InputSource::Bluetooth => "bt",
            InputSource::Usb => "usb",
        }
    }

    pub fn from_word(s: &str) -> Option<InputSource> {
        match s {
            "auto" => Some(InputSource::Auto),
            "bt" => Some(InputSource::Bluetooth),
            "usb" => Some(InputSource::Usb),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InputSource::Auto => "Auto",
            InputSource::Bluetooth => "Bluetooth",
            InputSource::Usb => "Wired USB",
        }
    }
}

/// A console command. Each one makes the firmware print at least one reply line, except
/// `Bootloader`, after which the board leaves USB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Version,
    Bootloader,
    /// `None` asks for the current mode.
    Mode(Option<Mode>),
    /// `None` asks for the current input source.
    Input(Option<InputSource>),
}

impl Command {
    /// The line to write, newline included.
    pub fn encode(self) -> String {
        match self {
            Command::Version => "ver\n".to_owned(),
            Command::Bootloader => "bootloader\n".to_owned(),
            Command::Mode(None) => "mode\n".to_owned(),
            Command::Mode(Some(m)) => format!("mode {}\n", m.word()),
            Command::Input(None) => "input\n".to_owned(),
            Command::Input(Some(s)) => format!("input {}\n", s.word()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_ids_pick_the_side() {
        assert_eq!(Side::from_usb(0x2FE3, 0x0005), Some(Side::Left));
        assert_eq!(Side::from_usb(0x2FE3, 0x0004), Some(Side::Right));
        assert_eq!(Side::from_usb(0x2FE3, 0x0001), None);
        assert_eq!(Side::from_usb(0x239A, 0x0005), None);
    }

    #[test]
    fn commands_encode_as_the_firmware_expects() {
        assert_eq!(Command::Version.encode(), "ver\n");
        assert_eq!(Command::Bootloader.encode(), "bootloader\n");
        assert_eq!(Command::Mode(Some(Mode::Keyboard)).encode(), "mode keyboard\n");
        assert_eq!(Command::Mode(None).encode(), "mode\n");
        assert_eq!(Command::Input(Some(InputSource::Bluetooth)).encode(), "input bt\n");
        assert_eq!(Command::Input(None).encode(), "input\n");
    }

    #[test]
    fn side_letters_round_trip() {
        for side in Side::BOTH {
            assert_eq!(Side::from_letter(&side.letter().to_string()), Some(side));
        }
    }
}
