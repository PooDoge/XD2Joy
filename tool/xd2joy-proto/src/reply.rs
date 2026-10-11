//! Parsing the firmware's reply lines. The firmware also prints free-form log lines, so anything
//! without a known tag is kept as `Reply::Other` for the log rather than treated as an error.
//!
//! Tags are read trimmed (`[CFG]` and `[CFG   ]` are the same): this firmware prints them bare,
//! but the web tool accepts padded ones, so a build that pads them must not break the app.

use crate::blob::{BlobError, Colours, ConfigBlob, Rgb};
use crate::mapping::Hotkey;
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

/// The address type Zephyr prints after a Bluetooth address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddrType {
    Public,
    Random,
    PublicId,
    RandomId,
}

impl AddrType {
    fn parse(s: &str) -> Option<AddrType> {
        Some(match s {
            "public" => AddrType::Public,
            "random" => AddrType::Random,
            "public-id" => AddrType::PublicId,
            "random-id" => AddrType::RandomId,
            _ => return None,
        })
    }

    /// The word `peer <addr> <type>` takes. The firmware reads at most 7 characters, so the `-id`
    /// forms are sent as their base type.
    pub fn peer_word(self) -> &'static str {
        match self {
            AddrType::Public | AddrType::PublicId => "public",
            AddrType::Random | AddrType::RandomId => "random",
        }
    }
}

/// A Bluetooth LE address as printed: `AA:BB:CC:DD:EE:FF (random)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BtAddr {
    pub bytes: [u8; 6],
    pub kind: AddrType,
}

impl BtAddr {
    /// Reads the address at the start of `s`; returns it and the text after it.
    pub fn parse_prefix(s: &str) -> Option<(BtAddr, &str)> {
        let s = s.trim_start();
        let (hex, rest) = (s.get(..17)?, s.get(17..)?);
        let mut bytes = [0u8; 6];
        for (i, part) in hex.split(':').enumerate() {
            if i >= 6 || part.len() != 2 {
                return None;
            }
            bytes[i] = u8::from_str_radix(part, 16).ok()?;
        }
        let rest = rest.strip_prefix(" (")?;
        let (kind, rest) = rest.split_once(')')?;
        Some((BtAddr { bytes, kind: AddrType::parse(kind)? }, rest))
    }

    /// `AA:BB:CC:DD:EE:FF`, as `peer` takes it.
    pub fn hex(&self) -> String {
        let b = self.bytes;
        format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
}

/// A device heard while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanDevice {
    pub addr: BtAddr,
    pub rssi: i16,
    /// Advertises the HID service.
    pub hid: bool,
    pub appearance: u16,
    pub name: String,
}

impl ScanDevice {
    pub fn kind(&self) -> DeviceKind {
        match self.appearance {
            0x03C1 => DeviceKind::Keyboard,
            0x03C2 => DeviceKind::Mouse,
            a if a >> 6 == 0x0F => DeviceKind::OtherInput,
            _ if self.hid => DeviceKind::OtherInput,
            _ => DeviceKind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    /// HID but neither a keyboard nor a mouse by its appearance (combos often say this).
    OtherInput,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgReply {
    /// `[CFG] blob <hex>`; a blob that fails to decode (often a line another one cut into) is the
    /// error, and the caller should ask again.
    Blob(Result<ConfigBlob, BlobError>),
    WriteOk,
    /// `write rejected`, `write FAILED` or `stored record rejected`: the text after the tag.
    WriteError(String),
    Deleted,
    DeleteFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSlot {
    /// 1..=4
    pub slot: u8,
    pub saved: bool,
    /// `None` when the slot has no hotkey (key 0).
    pub key: Option<Hotkey>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileReply {
    /// `[PROF] active N of 4`, the first line of the table every `profile` command prints.
    Active(u8),
    Slot(ProfileSlot),
    /// A switch happened (hotkey, link, or `profile load`).
    NowOn {
        slot: u8,
        name: String,
    },
}

/// Lines of the 5-second status block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapReply {
    Begin { side: Side, tag: String },
    ModeKey { key: Hotkey, mode: Mode },
    Sync(SyncStatus),
    StickSens(i32),
    MouseGain(u32),
    StickDeadzone(i32),
    StickSpeed(i32),
    Grip(bool),
    ProfileActive(u8),
    Profile(ProfileSlot),
    Colours(Colours),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncStatus {
    pub key: Hotkey,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidState {
    None,
    Linked,
    Subscribed,
}

/// Bluetooth keyboard/mouse link events (`[MOUSE]` on both boards, `[HIDLNK]`, `[JC]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// The pinned device was heard; the board is connecting.
    Connecting {
        addr: BtAddr,
        rssi: i16,
    },
    ConnectFailed {
        addr: BtAddr,
        code: u8,
    },
    Connected(BtAddr),
    Security {
        level: u8,
        err: u8,
    },
    /// HID discovery finished with this many report subscriptions.
    Ready(u32),
    Disconnected {
        reason: u8,
    },
    /// Type this on the keyboard, then Enter.
    Passkey(u32),
    /// The console-side connection failed.
    ConsoleConnectFailed(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerReply {
    Pin(Option<(BtAddr, String)>),
    Attached(Option<(BtAddr, String, bool)>),
    /// The pinned name was heard at another address; the board is trying it.
    AlsoAt {
        name: String,
        addr: BtAddr,
    },
    /// The new address paired and is now the pin.
    PairedAt {
        name: String,
        addr: BtAddr,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanReply {
    Begin(u32),
    /// A row of the list `scan` prints.
    Listed(ScanDevice),
    End,
    /// Printed as soon as a device is heard (after `scan!`).
    Heard(ScanDevice),
}

/// One `[RAW]` line: a Bluetooth HID report as the board received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReport {
    pub handle: u16,
    /// The real length; `bytes` holds at most the first 32.
    pub len: usize,
    pub bytes: Vec<u8>,
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
    Cfg(CfgReply),
    Profile(ProfileReply),
    /// `Some(byte)` when the board reports the grip byte; `None` for "off".
    Grip(Option<u8>),
    Sync(SyncStatus),
    Map(MapReply),
    Hid(HidState),
    Link(LinkEvent),
    Peer(PeerReply),
    Scan(ScanReply),
    Raw(RawReport),
    /// Any other line: a tag the tool does not read, or a log line.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("the {tag} line has a field the tool does not understand: {line:?}")]
    BadField { tag: &'static str, line: String },
}

/// Read one line. A known reply with a body that does not parse is an error (the firmware and the
/// tool disagree about the protocol, which the caller should log); an unknown tag, or a known tag
/// with a log message the tool does not read, is `Other`.
pub fn parse_line(line: &str) -> Result<Reply, ParseError> {
    let line = line.trim();
    let Some((tag, body)) = split_tag(line) else {
        return Ok(Reply::Other(line.to_owned()));
    };
    let bad = |tag: &'static str| ParseError::BadField { tag, line: line.to_owned() };
    let other = || Ok(Reply::Other(line.to_owned()));
    match tag {
        "VER" => parse_ver(body).map(Reply::Version).ok_or_else(|| bad("VER")),
        "BOOT" => Ok(Reply::Bootloader),
        "MODE" => parse_mode(body).ok_or_else(|| bad("MODE")),
        "INPUT" => parse_input(body).map(Reply::Input).ok_or_else(|| bad("INPUT")),
        "CFG" => match parse_cfg(body) {
            Some(r) => Ok(Reply::Cfg(r)),
            None => other(),
        },
        "PROF" => match parse_prof(body) {
            Parsed::Yes(r) => Ok(Reply::Profile(r)),
            Parsed::Bad => Err(bad("PROF")),
            Parsed::NotOurs => other(),
        },
        "GRIP" => parse_grip(body).map(Reply::Grip).ok_or_else(|| bad("GRIP")),
        "SYNC" => parse_sync(body.strip_prefix("shortcut ").ok_or_else(|| bad("SYNC"))?)
            .map(Reply::Sync)
            .ok_or_else(|| bad("SYNC")),
        "MAP" => match parse_map(body) {
            Parsed::Yes(r) => Ok(Reply::Map(r)),
            Parsed::Bad => Err(bad("MAP")),
            Parsed::NotOurs => other(),
        },
        "HID" => parse_hid(body).map(Reply::Hid).map_or_else(other, Ok),
        "MOUSE" | "HIDLNK" | "JC" => match parse_link(tag, body) {
            Parsed::Yes(r) => Ok(Reply::Link(r)),
            Parsed::Bad => Err(bad("MOUSE")),
            Parsed::NotOurs => other(),
        },
        "PEER" => match parse_peer(body) {
            Parsed::Yes(r) => Ok(Reply::Peer(r)),
            Parsed::Bad => Err(bad("PEER")),
            Parsed::NotOurs => other(),
        },
        "SCAN" => match parse_scan(body) {
            Parsed::Yes(r) => Ok(Reply::Scan(r)),
            Parsed::Bad => Err(bad("SCAN")),
            Parsed::NotOurs => other(),
        },
        "RAW" => parse_raw(body).map(Reply::Raw).ok_or_else(|| bad("RAW")),
        _ => other(),
    }
}

/// A line with a known tag either is a reply the tool reads (and must parse), is a malformed one,
/// or is a log message under that tag the tool does not read.
enum Parsed<T> {
    Yes(T),
    Bad,
    NotOurs,
}

impl<T> Parsed<T> {
    fn from(o: Option<T>) -> Parsed<T> {
        o.map_or(Parsed::Bad, Parsed::Yes)
    }
}

fn split_tag(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let (tag, body) = rest.split_once(']')?;
    Some((tag.trim(), body.trim()))
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
            "bridge" => bridge_devices = hex_u8(value),
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

fn parse_cfg(body: &str) -> Option<CfgReply> {
    if let Some(hex) = body.strip_prefix("blob") {
        return Some(CfgReply::Blob(ConfigBlob::from_hex(hex)));
    }
    if body.starts_with("write ok") {
        return Some(CfgReply::WriteOk);
    }
    if body.starts_with("write rejected")
        || body.starts_with("write FAILED")
        || body.starts_with("stored record rejected")
    {
        return Some(CfgReply::WriteError(body.to_owned()));
    }
    if body.starts_with("stored config deleted") {
        return Some(CfgReply::Deleted);
    }
    if body.starts_with("delete FAILED") {
        return Some(CfgReply::DeleteFailed(body.to_owned()));
    }
    None
}

fn parse_prof(body: &str) -> Parsed<ProfileReply> {
    if let Some(rest) = body.strip_prefix("active ") {
        let n = rest.split_whitespace().next().and_then(|n| n.parse().ok());
        return Parsed::from(n.map(ProfileReply::Active));
    }
    if let Some(rest) = body.strip_prefix("now on ") {
        let (n, name) = match rest.split_once(": ") {
            Some((n, name)) => (n, name),
            None => (rest, ""),
        };
        return Parsed::from(n.trim().parse().ok().map(|slot| ProfileReply::NowOn { slot, name: name.to_owned() }));
    }
    // `N saved|empty key 0xCC mods 0xMM name` (the status word is padded to 6)
    let mut w = body.splitn(2, ' ');
    let Some(slot) = w.next().and_then(|n| n.parse::<u8>().ok()) else {
        return Parsed::NotOurs;
    };
    let rest = w.next().unwrap_or("").trim_start();
    let parsed = (|| {
        let (state, rest) = rest.split_once(' ')?;
        let saved = match state {
            "saved" => true,
            "empty" => false,
            _ => return None,
        };
        let rest = rest.trim_start().strip_prefix("key ")?;
        let (key, rest) = rest.split_once(" mods ")?;
        let (mods, name) = match rest.split_once(' ') {
            Some((m, n)) => (m, n),
            None => (rest, ""),
        };
        let key = hex_u8(key)?;
        let mods = hex_u8(mods)?;
        Some(ProfileSlot { slot, saved, key: (key != 0).then_some(Hotkey { key, mods }), name: name.to_owned() })
    })();
    Parsed::from(parsed.map(ProfileReply::Slot))
}

/// `off: byte 0x04 replayed ...` or `byte 0x04 = 0x01 ...`
fn parse_grip(body: &str) -> Option<Option<u8>> {
    if body.starts_with("off") {
        return Some(None);
    }
    let v = body.strip_prefix("byte 0x04 = ")?.split_whitespace().next()?;
    hex_u8(v).map(Some)
}

/// `key 0x45 mods 0x00 on`
fn parse_sync(body: &str) -> Option<SyncStatus> {
    let mut w = body.split_whitespace();
    if w.next()? != "key" {
        return None;
    }
    let key = hex_u8(w.next()?)?;
    if w.next()? != "mods" {
        return None;
    }
    let mods = hex_u8(w.next()?)?;
    let on = match w.next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    Some(SyncStatus { key: Hotkey { key, mods }, on })
}

fn parse_map(body: &str) -> Parsed<MapReply> {
    let (word, rest) = body.split_once(' ').unwrap_or((body, ""));
    let r = match word {
        "side=L" | "side=R" => {
            let side = if word == "side=L" { Side::Left } else { Side::Right };
            let tag = rest.trim().strip_prefix("tag=").unwrap_or("").to_owned();
            Some(MapReply::Begin { side, tag })
        }
        "end" => Some(MapReply::End),
        // `mode KK MM NAME`
        "mode" => (|| {
            let mut w = rest.split_whitespace();
            let key = u8::from_str_radix(w.next()?, 16).ok()?;
            let mods = u8::from_str_radix(w.next()?, 16).ok()?;
            let mode = Mode::from_reply(w.next()?)?;
            Some(MapReply::ModeKey { key: Hotkey { key, mods }, mode })
        })(),
        // `sync KK mods MM on|off` (no 0x here, unlike `[SYNC]`)
        "sync" => parse_sync(&format!("key 0x{}", rest.trim())).map(MapReply::Sync),
        "sens" => rest.trim().parse().ok().map(MapReply::StickSens),
        "msens" => rest.trim().parse().ok().map(MapReply::MouseGain),
        "stickdz" => rest.trim().parse().ok().map(MapReply::StickDeadzone),
        "stickvg" => rest.trim().parse().ok().map(MapReply::StickSpeed),
        "grip" => match rest.trim() {
            "0" => Some(MapReply::Grip(false)),
            "1" => Some(MapReply::Grip(true)),
            _ => None,
        },
        "prof" => (|| {
            let mut w = rest.splitn(2, ' ');
            let first = w.next()?;
            let rest = w.next().unwrap_or("");
            if first == "act" {
                return rest.trim().parse().ok().map(MapReply::ProfileActive);
            }
            // `N F KK MM name`
            let slot = first.parse().ok()?;
            let mut w = rest.splitn(4, ' ');
            let saved = w.next()? == "1";
            let key = u8::from_str_radix(w.next()?, 16).ok()?;
            let mods = u8::from_str_radix(w.next()?, 16).ok()?;
            let name = w.next().unwrap_or("").to_owned();
            Some(MapReply::Profile(ProfileSlot { slot, saved, key: (key != 0).then_some(Hotkey { key, mods }), name }))
        })(),
        "colors" => (|| {
            let c: Vec<Rgb> = rest.split_whitespace().map(Rgb::parse).collect::<Option<_>>()?;
            let c: [Rgb; 4] = c.try_into().ok()?;
            Some(MapReply::Colours(Colours::from_array(c)))
        })(),
        _ => return Parsed::NotOurs,
    };
    Parsed::from(r)
}

fn parse_hid(body: &str) -> Option<HidState> {
    let (_, state) = body.split_once('=')?;
    Some(match state.trim() {
        "none" => HidState::None,
        "linked" => HidState::Linked,
        "subscribed" => HidState::Subscribed,
        _ => return None,
    })
}

fn parse_link(tag: &str, body: &str) -> Parsed<LinkEvent> {
    let r = match tag {
        // `PASSKEY 123456: type it on <addr> and press Enter`
        "HIDLNK" => match body.strip_prefix("PASSKEY ") {
            Some(rest) => rest.get(..6).and_then(|d| d.parse().ok()).map(LinkEvent::Passkey),
            None => return Parsed::NotOurs,
        },
        // `connect failed (0x3e)`
        "JC" => match body.strip_prefix("connect failed (") {
            Some(rest) => paren_hex(rest).map(LinkEvent::ConsoleConnectFailed),
            None => return Parsed::NotOurs,
        },
        _ => {
            if let Some(rest) = body.strip_prefix("HID advertiser ") {
                BtAddr::parse_prefix(rest).and_then(|(addr, rest)| {
                    let rssi = rest.trim_start().strip_prefix("rssi=")?.split(',').next()?.parse().ok()?;
                    Some(LinkEvent::Connecting { addr, rssi })
                })
            } else if let Some(rest) = body.strip_prefix("connect to ") {
                BtAddr::parse_prefix(rest).and_then(|(addr, rest)| {
                    let code = paren_hex(rest.trim_start().strip_prefix("failed (")?)?;
                    Some(LinkEvent::ConnectFailed { addr, code })
                })
            } else if let Some(rest) = body.strip_prefix("*** CONNECTED to ") {
                BtAddr::parse_prefix(rest).map(|(addr, _)| LinkEvent::Connected(addr))
            } else if let Some(rest) = body.strip_prefix("*** DISCONNECTED, reason ") {
                rest.split_whitespace().next().and_then(hex_u8).map(|reason| LinkEvent::Disconnected { reason })
            } else if let Some(rest) = body.strip_prefix("security level=") {
                (|| {
                    let (level, err) = rest.split_once(" err=")?;
                    Some(LinkEvent::Security { level: level.parse().ok()?, err: err.trim().parse().ok()? })
                })()
            } else if let Some(rest) = body.strip_prefix("discovery done, ") {
                rest.split_whitespace().next().and_then(|n| n.parse().ok()).map(LinkEvent::Ready)
            } else {
                return Parsed::NotOurs;
            }
        }
    };
    Parsed::from(r)
}

/// `0x3e)` -> 0x3E
fn paren_hex(s: &str) -> Option<u8> {
    hex_u8(s.split(')').next()?)
}

fn parse_peer(body: &str) -> Parsed<PeerReply> {
    let r = if let Some(rest) = body.strip_prefix("pin=") {
        if rest.trim() == "none" {
            Some(PeerReply::Pin(None))
        } else {
            BtAddr::parse_prefix(rest).and_then(|(addr, rest)| {
                let name = quoted_to_end(rest.trim_start().strip_prefix("name=")?)?;
                Some(PeerReply::Pin(Some((addr, name))))
            })
        }
    } else if let Some(rest) = body.strip_prefix("attached=") {
        if rest.trim() == "none" {
            Some(PeerReply::Attached(None))
        } else {
            BtAddr::parse_prefix(rest).and_then(|(addr, rest)| {
                let (name, matches) = rest.trim_start().strip_prefix("name=")?.rsplit_once(" match=")?;
                let matches = match matches.trim() {
                    "1" => true,
                    "0" => false,
                    _ => return None,
                };
                Some(PeerReply::Attached(Some((addr, quoted_to_end(name)?, matches))))
            })
        }
    } else if let Some(rest) = body.strip_prefix('\'') {
        // `'name' also at <addr>, trying it` / `'name' paired at <addr>, pin updated`
        let follow = |marker: &str| {
            let (name, after) = rest.rsplit_once(marker)?;
            let (addr, _) = BtAddr::parse_prefix(after)?;
            Some((name.to_owned(), addr))
        };
        if let Some((name, addr)) = follow("' also at ") {
            Some(PeerReply::AlsoAt { name, addr })
        } else if let Some((name, addr)) = follow("' paired at ") {
            Some(PeerReply::PairedAt { name, addr })
        } else {
            return Parsed::NotOurs;
        }
    } else {
        return Parsed::NotOurs;
    };
    Parsed::from(r)
}

/// `'text'` up to the **last** quote: names are raw advertising bytes and may contain quotes.
fn quoted_to_end(s: &str) -> Option<String> {
    let inner = s.trim().strip_prefix('\'')?;
    let end = inner.rfind('\'')?;
    Some(inner[..end].to_owned())
}

fn parse_scan(body: &str) -> Parsed<ScanReply> {
    if let Some(rest) = body.strip_prefix("begin n=") {
        return Parsed::from(rest.trim().parse().ok().map(ScanReply::Begin));
    }
    if body == "end" {
        return Parsed::Yes(ScanReply::End);
    }
    let (listed, rest) = match body.strip_prefix("dev ") {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let Some((addr, rest)) = BtAddr::parse_prefix(rest) else {
        return Parsed::NotOurs;
    };
    // `rssi=-60 [type=0 ]hid=1 appearance=0x03C1 name='...'`
    let dev = (|| {
        let (fields, name) = rest.split_once(" name=")?;
        let mut rssi = None;
        let mut hid = None;
        let mut appearance = None;
        for pair in fields.split_whitespace() {
            let (k, v) = pair.split_once('=')?;
            match k {
                "rssi" => rssi = v.parse().ok(),
                "hid" => hid = Some(v == "1"),
                "appearance" => appearance = u16::from_str_radix(v.trim_start_matches("0x"), 16).ok(),
                _ => {}
            }
        }
        Some(ScanDevice { addr, rssi: rssi?, hid: hid?, appearance: appearance?, name: quoted_to_end(name)? })
    })();
    Parsed::from(dev.map(|d| if listed { ScanReply::Listed(d) } else { ScanReply::Heard(d) }))
}

/// `h=0x0012 len=8: 00 00 1A 00 00 00 00 00`
fn parse_raw(body: &str) -> Option<RawReport> {
    let rest = body.strip_prefix("h=")?;
    let (handle, rest) = rest.split_once(' ')?;
    let handle = u16::from_str_radix(handle.trim_start_matches("0x"), 16).ok()?;
    let (len, bytes) = rest.trim_start().strip_prefix("len=")?.split_once(':')?;
    let len = len.parse().ok()?;
    let bytes = bytes.split_whitespace().map(|b| u8::from_str_radix(b, 16).ok()).collect::<Option<Vec<_>>>()?;
    Some(RawReport { handle, len, bytes })
}

fn hex_u8(s: &str) -> Option<u8> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u8::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::DEFAULT_RIGHT_HEX;

    fn addr(kind: AddrType) -> BtAddr {
        BtAddr { bytes: [0xC1, 0x22, 0x33, 0x44, 0x55, 0xF6], kind }
    }

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
    fn padded_tags_read_the_same() {
        assert_eq!(parse_line("[MODE  ] STICK"), parse_line("[MODE] STICK"));
        assert_eq!(parse_line("[CFG   ] write ok, 160 bytes stored and live"), Ok(Reply::Cfg(CfgReply::WriteOk)));
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
    fn cfg_lines() {
        let blob = parse_line(&format!("[CFG] blob {DEFAULT_RIGHT_HEX}"));
        assert_eq!(blob, Ok(Reply::Cfg(CfgReply::Blob(Ok(ConfigBlob::firmware_default(Side::Right))))));
        // a blob another line cut into fails to decode, so the caller asks again
        assert!(matches!(
            parse_line(&format!("[CFG] blob {}", &DEFAULT_RIGHT_HEX[..100])),
            Ok(Reply::Cfg(CfgReply::Blob(Err(BlobError::HexLength(100)))))
        ));
        let err = "write rejected: 12 bytes, expected 160";
        assert_eq!(parse_line(&format!("[CFG] {err}")), Ok(Reply::Cfg(CfgReply::WriteError(err.into()))));
        let err = "stored record rejected: side 0x67, this is a 0x66 board";
        assert_eq!(parse_line(&format!("[CFG] {err}")), Ok(Reply::Cfg(CfgReply::WriteError(err.into()))));
        assert_eq!(
            parse_line("[CFG] stored config deleted, back to the built-in blob"),
            Ok(Reply::Cfg(CfgReply::Deleted))
        );
        assert!(matches!(parse_line("[CFG] something new"), Ok(Reply::Other(_))));
    }

    #[test]
    fn profile_table() {
        assert_eq!(parse_line("[PROF] active 2 of 4"), Ok(Reply::Profile(ProfileReply::Active(2))));
        assert_eq!(
            parse_line("[PROF] 1 saved  key 0x1E mods 0x01 Elden Ring"),
            Ok(Reply::Profile(ProfileReply::Slot(ProfileSlot {
                slot: 1,
                saved: true,
                key: Some(Hotkey { key: 0x1E, mods: 0x01 }),
                name: "Elden Ring".into(),
            })))
        );
        // empty name: the firmware still prints the trailing space, and trim drops it
        assert_eq!(
            parse_line("[PROF] 4 empty  key 0x00 mods 0x00 "),
            Ok(Reply::Profile(ProfileReply::Slot(ProfileSlot {
                slot: 4,
                saved: false,
                key: None,
                name: String::new()
            })))
        );
        assert_eq!(
            parse_line("[PROF] now on 3: Mario Kart"),
            Ok(Reply::Profile(ProfileReply::NowOn { slot: 3, name: "Mario Kart".into() }))
        );
        assert_eq!(
            parse_line("[PROF] now on 1"),
            Ok(Reply::Profile(ProfileReply::NowOn { slot: 1, name: String::new() }))
        );
        assert!(parse_line("[PROF] 2 maybe key 0x00 mods 0x00").is_err());
    }

    #[test]
    fn grip_and_sync() {
        assert_eq!(parse_line("[GRIP] off: byte 0x04 replayed (0x07, not in a grip)"), Ok(Reply::Grip(None)));
        assert_eq!(
            parse_line("[GRIP] byte 0x04 = 0x01 (IN THE CHARGING GRIP, as a real one reports)"),
            Ok(Reply::Grip(Some(1)))
        );
        assert_eq!(
            parse_line("[SYNC] shortcut key 0x45 mods 0x00 on"),
            Ok(Reply::Sync(SyncStatus { key: Hotkey { key: 0x45, mods: 0 }, on: true }))
        );
    }

    #[test]
    fn status_block() {
        let map = |s: &str| {
            parse_line(s).map(|r| match r {
                Reply::Map(m) => Some(m),
                _ => None,
            })
        };
        assert_eq!(map("[MAP] side=R tag=ease1"), Ok(Some(MapReply::Begin { side: Side::Right, tag: "ease1".into() })));
        assert_eq!(
            map("[MAP] mode 0E 01 KEYBOARD"),
            Ok(Some(MapReply::ModeKey { key: Hotkey { key: 0x0E, mods: 1 }, mode: Mode::Keyboard }))
        );
        assert_eq!(
            map("[MAP] sync 45 mods 00 off"),
            Ok(Some(MapReply::Sync(SyncStatus { key: Hotkey { key: 0x45, mods: 0 }, on: false })))
        );
        assert_eq!(map("[MAP] msens 420"), Ok(Some(MapReply::MouseGain(420))));
        assert_eq!(map("[MAP] stickdz 290"), Ok(Some(MapReply::StickDeadzone(290))));
        assert_eq!(map("[MAP] grip 1"), Ok(Some(MapReply::Grip(true))));
        assert_eq!(map("[MAP] prof act 1"), Ok(Some(MapReply::ProfileActive(1))));
        assert_eq!(
            map("[MAP] prof 2 1 1F 00 Two words"),
            Ok(Some(MapReply::Profile(ProfileSlot {
                slot: 2,
                saved: true,
                key: Some(Hotkey { key: 0x1F, mods: 0 }),
                name: "Two words".into()
            })))
        );
        assert_eq!(
            map("[MAP] colors 4BA16B D54C58 543E38 F7826F"),
            Ok(Some(MapReply::Colours(Colours::FIRMWARE_DEFAULT)))
        );
        assert_eq!(map("[MAP] end"), Ok(Some(MapReply::End)));
        assert!(parse_line("[MAP] colors 4BA16B").is_err());
        assert_eq!(parse_line("[HID] kbd=subscribed"), Ok(Reply::Hid(HidState::Subscribed)));
    }

    #[test]
    fn link_events() {
        let r = |s: &str| parse_line(s);
        assert_eq!(
            r("[MOUSE] HID advertiser C1:22:33:44:55:F6 (random) rssi=-61, connecting"),
            Ok(Reply::Link(LinkEvent::Connecting { addr: addr(AddrType::Random), rssi: -61 }))
        );
        assert_eq!(
            r("[MOUSE] connect to C1:22:33:44:55:F6 (public) failed (0x3e), scanning again"),
            Ok(Reply::Link(LinkEvent::ConnectFailed { addr: addr(AddrType::Public), code: 0x3E }))
        );
        assert_eq!(
            r("[MOUSE] *** CONNECTED to C1:22:33:44:55:F6 (random) ***"),
            Ok(Reply::Link(LinkEvent::Connected(addr(AddrType::Random))))
        );
        assert_eq!(r("[MOUSE] security level=2 err=0"), Ok(Reply::Link(LinkEvent::Security { level: 2, err: 0 })));
        assert_eq!(r("[MOUSE] discovery done, 3 report subscriptions"), Ok(Reply::Link(LinkEvent::Ready(3))));
        assert_eq!(
            r("[MOUSE] *** DISCONNECTED, reason 0x13 *** reports=120"),
            Ok(Reply::Link(LinkEvent::Disconnected { reason: 0x13 }))
        );
        assert_eq!(
            r("[HIDLNK] PASSKEY 012345: type it on C1:22:33:44:55:F6 (random) and press Enter"),
            Ok(Reply::Link(LinkEvent::Passkey(12345)))
        );
        assert_eq!(r("[JC] connect failed (0x3e)"), Ok(Reply::Link(LinkEvent::ConsoleConnectFailed(0x3E))));
        assert!(matches!(r("[MOUSE] report map 312 bytes"), Ok(Reply::Other(_))));
    }

    #[test]
    fn peer_lines() {
        assert_eq!(parse_line("[PEER] pin=none"), Ok(Reply::Peer(PeerReply::Pin(None))));
        assert_eq!(
            parse_line("[PEER] pin=C1:22:33:44:55:F6 (random) name='K380 'Pro''"),
            Ok(Reply::Peer(PeerReply::Pin(Some((addr(AddrType::Random), "K380 'Pro'".into())))))
        );
        assert_eq!(
            parse_line("[PEER] attached=C1:22:33:44:55:F6 (public) name='MX' match=0"),
            Ok(Reply::Peer(PeerReply::Attached(Some((addr(AddrType::Public), "MX".into(), false)))))
        );
        assert_eq!(parse_line("[PEER] attached=none"), Ok(Reply::Peer(PeerReply::Attached(None))));
        assert_eq!(
            parse_line("[PEER] 'MX' also at C1:22:33:44:55:F6 (random), trying it"),
            Ok(Reply::Peer(PeerReply::AlsoAt { name: "MX".into(), addr: addr(AddrType::Random) }))
        );
        assert_eq!(
            parse_line("[PEER] 'MX' paired at C1:22:33:44:55:F6 (random-id), pin updated"),
            Ok(Reply::Peer(PeerReply::PairedAt { name: "MX".into(), addr: addr(AddrType::RandomId) }))
        );
    }

    #[test]
    fn scan_lines() {
        assert_eq!(parse_line("[SCAN] begin n=2"), Ok(Reply::Scan(ScanReply::Begin(2))));
        assert_eq!(parse_line("[SCAN] end"), Ok(Reply::Scan(ScanReply::End)));
        let dev = ScanDevice {
            addr: addr(AddrType::Random),
            rssi: -48,
            hid: true,
            appearance: 0x03C1,
            name: "Keys [BT]".into(),
        };
        assert_eq!(
            parse_line("[SCAN] dev C1:22:33:44:55:F6 (random) rssi=-48 hid=1 appearance=0x03C1 name='Keys [BT]'"),
            Ok(Reply::Scan(ScanReply::Listed(dev.clone())))
        );
        assert_eq!(
            parse_line("[SCAN] C1:22:33:44:55:F6 (random) rssi=-48 type=0 hid=1 appearance=0x03C1 name='Keys [BT]'"),
            Ok(Reply::Scan(ScanReply::Heard(dev.clone())))
        );
        assert_eq!(dev.kind(), DeviceKind::Keyboard);
        assert!(parse_line("[SCAN] C1:22:33:44:55:F6 (random) rssi=x hid=1 appearance=0x03C1 name=''").is_err());
    }

    #[test]
    fn raw_reports() {
        assert_eq!(
            parse_line("[RAW] h=0x0012 len=8: 02 00 1A 00 00 00 00 00"),
            Ok(Reply::Raw(RawReport { handle: 0x12, len: 8, bytes: vec![2, 0, 0x1A, 0, 0, 0, 0, 0] }))
        );
        assert!(parse_line("[RAW] h=0x0012 len=8: 02 zz").is_err());
    }

    #[test]
    fn everything_else_is_kept_for_the_log() {
        assert_eq!(parse_line("booting"), Ok(Reply::Other("booting".into())));
        assert_eq!(parse_line("[STORE] wrote jc/cfg"), Ok(Reply::Other("[STORE] wrote jc/cfg".into())));
        assert_eq!(parse_line("[BOOT] restarting into the UF2 bootloader"), Ok(Reply::Bootloader));
    }

    #[test]
    fn addresses() {
        assert_eq!(addr(AddrType::Random).hex(), "C1:22:33:44:55:F6");
        assert_eq!(AddrType::RandomId.peer_word(), "random");
        assert_eq!(BtAddr::parse_prefix("C1:22:33:44:55 (random)"), None);
        assert_eq!(BtAddr::parse_prefix("C1:22:33:44:55:F6 (weird)"), None);
    }
}
