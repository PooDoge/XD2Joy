//! The 160-byte config blob (`struct jc_cfg` in the firmware), read with `cfg` and written with
//! `cfg <320 hex chars>`. Each board has its own; the layout is the same on both.
//!
//! This module only knows the bytes. What an entry means (which key presses which Joy-Con button)
//! is in `mapping`.

use std::fmt;

use crate::Side;

pub const BLOB_LEN: usize = 160;
pub const MAX_ENTRIES: usize = 32;
pub const BLOB_VERSION: u16 = 3;

const MAGIC0: u32 = 0x3243_4A55;
const MAGIC1: u32 = 0x1A5F_4247;
const OFF_ENTRIES: usize = 28;
const OFF_COLOURS: usize = 16;
const OFF_END: usize = 156;

/// The `side` byte: the firmware rejects a blob written to the other board.
fn side_byte(side: Side) -> u8 {
    match side {
        Side::Left => 0x67,
        Side::Right => 0x66,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// `RRGGBB` or `#RRGGBB`, either case.
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
}

/// The four colours the console reads when it connects, in blob order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Colours {
    pub body: Rgb,
    pub buttons: Rgb,
    pub rail: Rgb,
    pub stick: Rgb,
}

impl Colours {
    /// The firmware's built-in colours (both sides).
    pub const FIRMWARE_DEFAULT: Colours = Colours {
        body: Rgb([0x4B, 0xA1, 0x6B]),
        buttons: Rgb([0xD5, 0x4C, 0x58]),
        rail: Rgb([0x54, 0x3E, 0x38]),
        stick: Rgb([0xF7, 0x82, 0x6F]),
    };

    /// The colours of a grey retail Joy-Con 2, as the web tool's "Stock Joy-Con" button sets them.
    pub fn stock(side: Side) -> Colours {
        let rail = match side {
            Side::Left => Rgb([0x9B, 0xE1, 0xE6]),
            Side::Right => Rgb([0xFF, 0x8C, 0x5F]),
        };
        Colours { body: Rgb([0x32; 3]), buttons: Rgb([0xAA; 3]), rail, stick: Rgb([0x32; 3]) }
    }

    pub fn as_array(&self) -> [Rgb; 4] {
        [self.body, self.buttons, self.rail, self.stick]
    }

    pub fn from_array(c: [Rgb; 4]) -> Colours {
        Colours { body: c[0], buttons: c[1], rail: c[2], stick: c[3] }
    }
}

/// One `btn[]` slot exactly as stored. `kind` stays a raw byte so entries this tool does not
/// understand (newer firmware, hand edits) survive a read-modify-write untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entry {
    pub code: u8,
    pub kind: u8,
    pub mask: u16,
}

/// The entry kinds of protocol 1 (`JC_BTN_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Right: mouse button bits -> right Joy-Con.
    Mouse = 0,
    /// Left: key usage -> left Joy-Con.
    Key = 1,
    /// Left: modifier bits -> left Joy-Con.
    Mod = 2,
    /// Left: key usage -> right Joy-Con, over the link.
    Peer = 3,
    /// Left: key usage -> left stick offset (int8 dx, int8 dy).
    Stick = 4,
    /// Left: modifier bits -> right Joy-Con, over the link.
    PeerMod = 5,
    /// Right: mouse button bits -> left Joy-Con, over the link.
    MousePeer = 6,
    /// Left: key usage -> mode toggle, mask = mode << 8 | modifiers.
    Mode = 7,
    /// Right: setting id -> value.
    Set = 8,
    /// Right: 1 wheel up / 2 wheel down -> right Joy-Con, pulsed.
    Wheel = 9,
    /// Right: wheel -> left Joy-Con, over the link.
    WheelPeer = 10,
}

impl Kind {
    pub fn from_byte(b: u8) -> Option<Kind> {
        Some(match b {
            0 => Kind::Mouse,
            1 => Kind::Key,
            2 => Kind::Mod,
            3 => Kind::Peer,
            4 => Kind::Stick,
            5 => Kind::PeerMod,
            6 => Kind::MousePeer,
            7 => Kind::Mode,
            8 => Kind::Set,
            9 => Kind::Wheel,
            10 => Kind::WheelPeer,
            _ => return None,
        })
    }

    /// The board whose firmware reads this kind; on the other board the entry does nothing.
    pub fn board(self) -> Side {
        match self {
            Kind::Key | Kind::Mod | Kind::Peer | Kind::Stick | Kind::PeerMod | Kind::Mode => Side::Left,
            Kind::Mouse | Kind::MousePeer | Kind::Set | Kind::Wheel | Kind::WheelPeer => Side::Right,
        }
    }
}

impl Entry {
    pub fn new(kind: Kind, code: u8, mask: u16) -> Entry {
        Entry { code, kind: kind as u8, mask }
    }

    pub fn kind(&self) -> Option<Kind> {
        Kind::from_byte(self.kind)
    }
}

/// A decoded, valid config blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigBlob {
    pub side: Side,
    /// Right board: stick counts per unit of a `stick_src` input. Kept as read; the tool has no
    /// control for it, same as the web tool.
    pub stick_gain: u16,
    /// Right board: what drives the right stick's X and Y in stick mode. Raw bytes, see
    /// `mapping::StickSource`. Kept as read on the left board, where the firmware ignores them.
    pub stick_src: [u8; 2],
    pub colours: Colours,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlobError {
    #[error("the config is {0} hex characters, expected 320")]
    HexLength(usize),
    #[error("the config has a character that is not hex at position {0}")]
    BadHex(usize),
    #[error("the config does not start with the XD2Joy marker")]
    BadMagic,
    #[error("the config does not end with the XD2Joy marker")]
    BadEnd,
    #[error("the config is version {0}; this tool reads version 3")]
    Version(u16),
    #[error("the config's side byte is 0x{0:02X}")]
    Side(u8),
    #[error("the config says it has {0} entries; the most is 32")]
    TooManyEntries(usize),
}

impl ConfigBlob {
    /// Checks what the firmware checks (`cfg_record_ok`), so a blob that decodes here is one the
    /// board accepts.
    pub fn from_bytes(b: &[u8; BLOB_LEN]) -> Result<ConfigBlob, BlobError> {
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        if u32_at(0) != MAGIC0 || u32_at(4) != MAGIC1 {
            return Err(BlobError::BadMagic);
        }
        if u32_at(OFF_END) != MAGIC0 {
            return Err(BlobError::BadEnd);
        }
        if u16_at(8) != BLOB_VERSION {
            return Err(BlobError::Version(u16_at(8)));
        }
        let side = match b[10] {
            0x67 => Side::Left,
            0x66 => Side::Right,
            other => return Err(BlobError::Side(other)),
        };
        let n = usize::from(b[11]);
        if n > MAX_ENTRIES {
            return Err(BlobError::TooManyEntries(n));
        }
        let rgb = |i: usize| {
            let o = OFF_COLOURS + i * 3;
            Rgb([b[o], b[o + 1], b[o + 2]])
        };
        let entries = (0..n)
            .map(|i| {
                let o = OFF_ENTRIES + i * 4;
                Entry { code: b[o], kind: b[o + 1], mask: u16_at(o + 2) }
            })
            .collect();
        Ok(ConfigBlob {
            side,
            stick_gain: u16_at(12),
            stick_src: [b[14], b[15]],
            colours: Colours::from_array([rgb(0), rgb(1), rgb(2), rgb(3)]),
            entries,
        })
    }

    /// Unused entry slots are zero, as the web tool writes them.
    pub fn to_bytes(&self) -> Result<[u8; BLOB_LEN], BlobError> {
        if self.entries.len() > MAX_ENTRIES {
            return Err(BlobError::TooManyEntries(self.entries.len()));
        }
        let mut b = [0u8; BLOB_LEN];
        b[0..4].copy_from_slice(&MAGIC0.to_le_bytes());
        b[4..8].copy_from_slice(&MAGIC1.to_le_bytes());
        b[8..10].copy_from_slice(&BLOB_VERSION.to_le_bytes());
        b[10] = side_byte(self.side);
        // checked against MAX_ENTRIES above
        b[11] = self.entries.len() as u8;
        b[12..14].copy_from_slice(&self.stick_gain.to_le_bytes());
        b[14..16].copy_from_slice(&self.stick_src);
        for (i, c) in self.colours.as_array().iter().enumerate() {
            let o = OFF_COLOURS + i * 3;
            b[o..o + 3].copy_from_slice(&c.0);
        }
        for (i, e) in self.entries.iter().enumerate() {
            let o = OFF_ENTRIES + i * 4;
            b[o] = e.code;
            b[o + 1] = e.kind;
            b[o + 2..o + 4].copy_from_slice(&e.mask.to_le_bytes());
        }
        b[OFF_END..].copy_from_slice(&MAGIC0.to_le_bytes());
        Ok(b)
    }

    /// From the 320 hex characters of `[CFG] blob <hex>`.
    pub fn from_hex(hex: &str) -> Result<ConfigBlob, BlobError> {
        let hex = hex.trim();
        if hex.len() != BLOB_LEN * 2 {
            return Err(BlobError::HexLength(hex.len()));
        }
        let mut b = [0u8; BLOB_LEN];
        for (i, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let hi = hex_val(pair[0]).ok_or(BlobError::BadHex(i * 2))?;
            let lo = hex_val(pair[1]).ok_or(BlobError::BadHex(i * 2 + 1))?;
            b[i] = hi << 4 | lo;
        }
        ConfigBlob::from_bytes(&b)
    }

    /// Uppercase, no separators: the argument of `cfg <hex>`.
    pub fn to_hex(&self) -> Result<String, BlobError> {
        let bytes = self.to_bytes()?;
        let mut s = String::with_capacity(BLOB_LEN * 2);
        for byte in bytes {
            s.push_str(&format!("{byte:02X}"));
        }
        Ok(s)
    }

    /// The blob the firmware has built in, for when a board has never been configured.
    pub fn firmware_default(side: Side) -> ConfigBlob {
        let hex = match side {
            Side::Left => DEFAULT_LEFT_HEX,
            Side::Right => DEFAULT_RIGHT_HEX,
        };
        match ConfigBlob::from_hex(hex) {
            Ok(blob) => blob,
            // The constants are tested below; this arm is unreachable in a tested build.
            Err(_) => ConfigBlob {
                side,
                stick_gain: 500,
                stick_src: [0, 0],
                colours: Colours::FIRMWARE_DEFAULT,
                entries: Vec::new(),
            },
        }
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `cfg` output of a left board with nothing stored (firmware `CFG_FLASH`).
pub const DEFAULT_LEFT_HEX: &str = "554A433247425F1A0300671BF40100004BA16BD54C58543E38F7826F3E01000151010100140100040F01100050010400020280002B014000090302002C030100120300102903000128034000200300802103004015030800170304004F0102001E0100801F010040160400FF0404FF00070401001A04000152010800080120000E0701010D0701020000000000000000000000000000000000000000554A4332";
/// `cfg` output of a right board with nothing stored.
pub const DEFAULT_RIGHT_HEX: &str = "554A433247425F1A03006607F40100034BA16BD54C58543E38F7826F010010000200200001080A000208A40103080100040822010508050000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000554A4332";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_defaults_round_trip_exactly() {
        for (side, hex, n) in [(Side::Left, DEFAULT_LEFT_HEX, 27), (Side::Right, DEFAULT_RIGHT_HEX, 7)] {
            let blob = ConfigBlob::from_hex(hex).expect("default blob decodes");
            assert_eq!(blob.side, side);
            assert_eq!(blob.entries.len(), n);
            assert_eq!(blob.stick_gain, 500);
            assert_eq!(blob.colours, Colours::FIRMWARE_DEFAULT);
            assert_eq!(blob.to_hex().as_deref(), Ok(hex));
        }
        let right = ConfigBlob::firmware_default(Side::Right);
        assert_eq!(right.stick_src, [0, 3]);
        assert_eq!(right.entries[0], Entry::new(Kind::Mouse, 0x01, 0x0010));
        assert_eq!(right.entries[3], Entry::new(Kind::Set, 2, 420));
    }

    #[test]
    fn lowercase_hex_reads() {
        let blob = ConfigBlob::from_hex(&DEFAULT_LEFT_HEX.to_lowercase());
        assert_eq!(blob, Ok(ConfigBlob::firmware_default(Side::Left)));
    }

    #[test]
    fn rejects_what_the_firmware_rejects() {
        let good = ConfigBlob::firmware_default(Side::Left).to_bytes().expect("encodes");
        let with = |i: usize, v: u8| {
            let mut b = good;
            b[i] = v;
            ConfigBlob::from_bytes(&b)
        };
        assert_eq!(with(0, 0), Err(BlobError::BadMagic));
        assert_eq!(with(5, 0), Err(BlobError::BadMagic));
        assert_eq!(with(159, 0), Err(BlobError::BadEnd));
        assert_eq!(with(8, 2), Err(BlobError::Version(2)));
        assert_eq!(with(10, 0x65), Err(BlobError::Side(0x65)));
        assert_eq!(with(11, 33), Err(BlobError::TooManyEntries(33)));
        assert_eq!(ConfigBlob::from_hex("554A"), Err(BlobError::HexLength(4)));
        let mut bad = DEFAULT_LEFT_HEX.to_owned();
        bad.replace_range(7..8, "G");
        assert_eq!(ConfigBlob::from_hex(&bad), Err(BlobError::BadHex(7)));
    }

    #[test]
    fn unknown_kinds_survive() {
        let mut blob = ConfigBlob::firmware_default(Side::Right);
        blob.entries.push(Entry { code: 9, kind: 42, mask: 0xBEEF });
        let back = ConfigBlob::from_hex(&blob.to_hex().expect("encodes")).expect("decodes");
        assert_eq!(back.entries.last(), Some(&Entry { code: 9, kind: 42, mask: 0xBEEF }));
        assert_eq!(back.entries.last().and_then(Entry::kind), None);
    }

    #[test]
    fn too_many_entries_do_not_encode() {
        let mut blob = ConfigBlob::firmware_default(Side::Left);
        blob.entries.resize(33, Entry::new(Kind::Key, 4, 1));
        assert_eq!(blob.to_bytes(), Err(BlobError::TooManyEntries(33)));
    }

    #[test]
    fn colours_parse_and_print() {
        assert_eq!(Rgb::parse("#4ba16b"), Some(Rgb([0x4B, 0xA1, 0x6B])));
        assert_eq!(Rgb::parse("4BA16B"), Some(Rgb([0x4B, 0xA1, 0x6B])));
        assert_eq!(Rgb::parse("#4BA16"), None);
        assert_eq!(Rgb::parse("#4BA16G"), None);
        assert_eq!(Rgb([0x4B, 0xA1, 0x6B]).to_string(), "#4BA16B");
    }
}
