//! The console commands the tool sends, how to write them, and which reply line finishes each.
//!
//! The firmware drops every byte that arrives while a command runs, so the device layer sends one
//! command, waits for `is_done_by` (or a timeout), and only then sends the next.

use crate::blob::{BlobError, ConfigBlob};
use crate::mapping::Hotkey;
use crate::reply::{BtAddr, CfgReply, PeerReply, ProfileReply, Reply, ScanReply};
use crate::{InputSource, Mode};

/// A profile slot, 1..=4. The firmware casts the number to a byte, so 257 would mean slot 1;
/// this type keeps such numbers from ever being sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot(u8);

impl Slot {
    pub const ALL: [Slot; 4] = [Slot(1), Slot(2), Slot(3), Slot(4)];

    pub fn new(n: u8) -> Option<Slot> {
        (1..=4).contains(&n).then_some(Slot(n))
    }

    pub fn number(self) -> u8 {
        self.0
    }
}

/// The longest profile name the firmware stores (52-byte buffer).
pub const PROFILE_NAME_MAX: usize = 51;
/// The longest device name `peer` stores (22-byte buffer).
pub const PEER_NAME_MAX: usize = 21;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Version,
    Bootloader,
    /// `None` asks for the current mode.
    Mode(Option<Mode>),
    /// `None` asks for the current input source.
    Input(Option<InputSource>),
    CfgRead,
    /// Holds the encoded hex; build it with `Command::cfg_write`.
    CfgWrite(String),
    /// Deletes the stored mapping **and** the console pairing key.
    CfgDefault,
    /// The list of devices heard so far.
    ScanList,
    /// Clears the list and listens for 20 s. Prints nothing; devices arrive as `[SCAN]` lines.
    ScanStart,
    PeerQuery,
    /// Forget the pinned keyboard or mouse. Prints nothing.
    PeerClear,
    /// Pin a keyboard or mouse. Prints nothing, even when the line is malformed.
    PeerSet {
        addr: BtAddr,
        name: String,
    },
    Profiles,
    ProfileSave(Slot),
    ProfileLoad(Slot),
    ProfileClear(Slot),
    ProfileName(Slot, String),
    ProfileKey(Slot, Option<Hotkey>),
    /// `None` asks. Never `grip?`: that sets the grip byte to 0.
    Grip(Option<bool>),
    SyncQuery,
    /// Forget the console and open a 120 s pairing window, on this board only. Prints nothing.
    SyncNow,
    SyncEnable(bool),
    /// `None` clears the key.
    SyncKey(Option<Hotkey>),
}

impl Command {
    pub fn cfg_write(blob: &ConfigBlob) -> Result<Command, BlobError> {
        blob.to_hex().map(Command::CfgWrite)
    }

    /// The line to write, newline included. Never has trailing spaces (`ver ` does nothing) and
    /// numbers are decimal or `0x` (the firmware reads a leading 0 as octal).
    pub fn encode(&self) -> String {
        let hk = |k: Option<Hotkey>| k.map_or("0x00 0x00".to_owned(), |k| format!("0x{:02X} 0x{:02X}", k.key, k.mods));
        let line = match self {
            Command::Version => "ver".to_owned(),
            Command::Bootloader => "bootloader".to_owned(),
            Command::Mode(None) => "mode".to_owned(),
            Command::Mode(Some(m)) => format!("mode {}", m.word()),
            Command::Input(None) => "input".to_owned(),
            Command::Input(Some(s)) => format!("input {}", s.word()),
            Command::CfgRead => "cfg".to_owned(),
            Command::CfgWrite(hex) => format!("cfg {hex}"),
            Command::CfgDefault => "cfg default".to_owned(),
            Command::ScanList => "scan".to_owned(),
            Command::ScanStart => "scan!".to_owned(),
            Command::PeerQuery => "peer".to_owned(),
            Command::PeerClear => "peer none".to_owned(),
            Command::PeerSet { addr, name } => {
                // The board reconnects a device that changed address by comparing names byte for
                // byte, and fills in the advertised name itself when none is sent. So send the
                // name only when it survives cleaning unchanged.
                let cleaned = clean_text(name, PEER_NAME_MAX);
                let name = if cleaned == *name { cleaned } else { String::new() };
                let mut s = format!("peer {} {}", addr.hex(), addr.kind.peer_word());
                if !name.is_empty() {
                    s.push(' ');
                    s.push_str(&name);
                }
                s
            }
            Command::Profiles => "profile".to_owned(),
            Command::ProfileSave(n) => format!("profile save {}", n.0),
            Command::ProfileLoad(n) => format!("profile load {}", n.0),
            Command::ProfileClear(n) => format!("profile clear {}", n.0),
            Command::ProfileName(n, name) => {
                let name = clean_text(name, PROFILE_NAME_MAX);
                if name.is_empty() { format!("profile name {}", n.0) } else { format!("profile name {} {name}", n.0) }
            }
            Command::ProfileKey(n, key) => format!("profile key {} {}", n.0, hk(*key)),
            Command::Grip(None) => "grip".to_owned(),
            Command::Grip(Some(true)) => "grip on".to_owned(),
            Command::Grip(Some(false)) => "grip off".to_owned(),
            Command::SyncQuery => "sync?".to_owned(),
            Command::SyncNow => "sync".to_owned(),
            Command::SyncEnable(true) => "sync on".to_owned(),
            Command::SyncEnable(false) => "sync off".to_owned(),
            Command::SyncKey(key) => format!("sync key {}", hk(*key)),
        };
        line + "\n"
    }

    /// The command prints nothing, so the device layer waits a short guard time instead and then
    /// confirms with `follow_up`.
    pub fn is_silent(&self) -> bool {
        matches!(self, Command::ScanStart | Command::PeerClear | Command::PeerSet { .. } | Command::SyncNow)
    }

    /// The query that shows whether a silent command worked.
    pub fn follow_up(&self) -> Option<Command> {
        match self {
            Command::PeerClear | Command::PeerSet { .. } => Some(Command::PeerQuery),
            _ => None,
        }
    }

    /// Does this reply line finish the command? Commands that print several lines finish on the
    /// last one, so the next command is not sent while the board is still printing.
    pub fn is_done_by(&self, reply: &Reply) -> bool {
        match (self, reply) {
            (Command::Version, Reply::Version(_))
            | (Command::Bootloader, Reply::Bootloader)
            | (Command::Mode(_), Reply::Mode { .. })
            | (Command::Input(_), Reply::Input(_))
            | (Command::CfgRead, Reply::Cfg(CfgReply::Blob(_)))
            | (Command::CfgWrite(_), Reply::Cfg(CfgReply::WriteOk | CfgReply::WriteError(_)))
            // `cfg default` prints `[SYNC] ...` after the deleted line
            | (Command::CfgDefault, Reply::Cfg(CfgReply::DeleteFailed(_)) | Reply::Sync(_))
            | (Command::ScanList, Reply::Scan(ScanReply::End))
            | (Command::PeerQuery, Reply::Peer(PeerReply::Attached(_)))
            | (Command::Grip(_), Reply::Grip(_))
            | (Command::SyncQuery | Command::SyncEnable(_) | Command::SyncKey(_), Reply::Sync(_)) => true,
            // every `profile ...` ends with the 4-slot table
            (
                Command::Profiles
                | Command::ProfileSave(_)
                | Command::ProfileLoad(_)
                | Command::ProfileClear(_)
                | Command::ProfileName(..)
                | Command::ProfileKey(..),
                Reply::Profile(ProfileReply::Slot(s)),
            ) => s.slot == 4,
            _ => false,
        }
    }

    /// Is this line part of the command's answer (as opposed to chatter printed meanwhile)? The
    /// device layer keeps these for the caller.
    pub fn concerns(&self, reply: &Reply) -> bool {
        match self {
            Command::Version => matches!(reply, Reply::Version(_)),
            Command::Bootloader => matches!(reply, Reply::Bootloader),
            Command::Mode(_) => matches!(reply, Reply::Mode { .. }),
            Command::Input(_) => matches!(reply, Reply::Input(_)),
            Command::CfgRead | Command::CfgWrite(_) => matches!(reply, Reply::Cfg(_)),
            Command::CfgDefault => matches!(reply, Reply::Cfg(_) | Reply::Sync(_)),
            Command::ScanList => {
                matches!(reply, Reply::Scan(ScanReply::Begin(_) | ScanReply::Listed(_) | ScanReply::End))
            }
            Command::PeerQuery => matches!(reply, Reply::Peer(PeerReply::Pin(_) | PeerReply::Attached(_))),
            Command::Profiles
            | Command::ProfileSave(_)
            | Command::ProfileLoad(_)
            | Command::ProfileClear(_)
            | Command::ProfileName(..)
            | Command::ProfileKey(..) => matches!(reply, Reply::Profile(_)),
            Command::Grip(_) => matches!(reply, Reply::Grip(_)),
            Command::SyncQuery | Command::SyncEnable(_) | Command::SyncKey(_) => matches!(reply, Reply::Sync(_)),
            Command::ScanStart | Command::PeerClear | Command::PeerSet { .. } | Command::SyncNow => false,
        }
    }

    /// How long the board may take. Writes to flash can stall on garbage collection.
    pub fn timeout(&self) -> std::time::Duration {
        use std::time::Duration;
        match self {
            _ if self.is_silent() => Duration::from_millis(250),
            Command::CfgWrite(_) | Command::CfgDefault => Duration::from_secs(8),
            Command::ProfileSave(_)
            | Command::ProfileLoad(_)
            | Command::ProfileClear(_)
            | Command::ProfileName(..)
            | Command::ProfileKey(..)
            | Command::Grip(Some(_))
            | Command::SyncEnable(_)
            | Command::SyncKey(_)
            | Command::Input(Some(_)) => Duration::from_secs(5),
            _ => Duration::from_secs(2),
        }
    }

    /// Changes something a person would not want changed by accident; the UI asks first.
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Command::CfgDefault | Command::SyncNow | Command::PeerClear | Command::ProfileClear(_))
    }
}

/// Printable ASCII only (anything else becomes a space), runs of spaces collapsed, trimmed, and cut
/// to `max`. Names are stored as raw bytes and printed back inside quotes, so this keeps the
/// round trip readable and the line short.
fn clean_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let c = if c.is_ascii_graphic() { c } else { ' ' };
        if c == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        out.push(c);
    }
    let mut out = out.trim_end().to_owned();
    out.truncate(max);
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    use crate::blob::DEFAULT_LEFT_HEX;
    use crate::reply::{AddrType, ProfileSlot, SyncStatus, parse_line};

    #[test]
    fn encodes_as_the_firmware_expects() {
        let slot = Slot::new(2).expect("valid");
        let key = Some(Hotkey { key: 0x1E, mods: 0x01 });
        let cases = [
            (Command::Version, "ver\n"),
            (Command::Bootloader, "bootloader\n"),
            (Command::Mode(Some(Mode::Keyboard)), "mode keyboard\n"),
            (Command::Mode(None), "mode\n"),
            (Command::Input(Some(InputSource::Bluetooth)), "input bt\n"),
            (Command::Input(None), "input\n"),
            (Command::CfgRead, "cfg\n"),
            (Command::CfgDefault, "cfg default\n"),
            (Command::ScanStart, "scan!\n"),
            (Command::PeerClear, "peer none\n"),
            (Command::ProfileSave(slot), "profile save 2\n"),
            (Command::ProfileKey(slot, key), "profile key 2 0x1E 0x01\n"),
            (Command::ProfileKey(slot, None), "profile key 2 0x00 0x00\n"),
            (Command::Grip(None), "grip\n"),
            (Command::Grip(Some(false)), "grip off\n"),
            (Command::SyncQuery, "sync?\n"),
            (Command::SyncKey(Some(Hotkey { key: 0x45, mods: 0 })), "sync key 0x45 0x00\n"),
        ];
        for (cmd, want) in cases {
            assert_eq!(cmd.encode(), want, "{cmd:?}");
        }
        let write = Command::cfg_write(&ConfigBlob::firmware_default(Side::Left)).expect("encodes");
        assert_eq!(write.encode(), format!("cfg {DEFAULT_LEFT_HEX}\n"));
        // 324 characters, under the firmware's 399
        assert!(write.encode().len() < 400);
    }

    #[test]
    fn peer_uses_the_bare_type_word_and_only_an_exact_name() {
        let addr = BtAddr { bytes: [0xC1, 0x22, 0x33, 0x44, 0x55, 0xF6], kind: AddrType::RandomId };
        let cmd = Command::PeerSet { addr, name: "Keychron K3".into() };
        assert_eq!(cmd.encode(), "peer C1:22:33:44:55:F6 random Keychron K3\n");
        // a name cleaning would change is left to the board, which stores the advertised one
        let cmd = Command::PeerSet { addr, name: "  Logi\tK380 keyboard é extra long name".into() };
        assert_eq!(cmd.encode(), "peer C1:22:33:44:55:F6 random\n");
        let cmd = Command::PeerSet { addr, name: String::new() };
        assert_eq!(cmd.encode(), "peer C1:22:33:44:55:F6 random\n");
    }

    #[test]
    fn profile_names_are_cleaned() {
        let slot = Slot::new(1).expect("valid");
        assert_eq!(Command::ProfileName(slot, "Mario\nKart  8".into()).encode(), "profile name 1 Mario Kart 8\n");
        assert_eq!(Command::ProfileName(slot, "   ".into()).encode(), "profile name 1\n");
        let long = "x".repeat(80);
        assert_eq!(Command::ProfileName(slot, long).encode().len(), "profile name 1 ".len() + 51 + 1);
    }

    #[test]
    fn slots_are_one_to_four() {
        assert_eq!(Slot::new(0), None);
        assert_eq!(Slot::new(5), None);
        assert_eq!(Slot::ALL.map(Slot::number), [1, 2, 3, 4]);
    }

    #[test]
    fn multi_line_replies_finish_on_their_last_line() {
        let parse = |s: &str| parse_line(s).expect("parses");
        let table_end = parse("[PROF] 4 empty  key 0x00 mods 0x00 ");
        let table_mid =
            Reply::Profile(ProfileReply::Slot(ProfileSlot { slot: 2, saved: false, key: None, name: String::new() }));
        let load = Command::ProfileLoad(Slot::new(3).expect("valid"));
        assert!(!load.is_done_by(&parse("[PROF] now on 3")));
        assert!(!load.is_done_by(&table_mid));
        assert!(load.is_done_by(&table_end));

        assert!(!Command::CfgDefault.is_done_by(&parse("[CFG] stored config deleted, back to the built-in blob")));
        let sync = Reply::Sync(SyncStatus { key: Hotkey { key: 0x45, mods: 0 }, on: true });
        assert!(Command::CfgDefault.is_done_by(&sync));
        assert!(!Command::PeerQuery.is_done_by(&parse("[PEER] pin=none")));
        assert!(Command::PeerQuery.is_done_by(&parse("[PEER] attached=none")));
        assert!(!Command::ScanList.is_done_by(&parse("[SCAN] begin n=0")));
        assert!(Command::ScanList.is_done_by(&parse("[SCAN] end")));
        // unrelated chatter never finishes a command
        assert!(!Command::CfgRead.is_done_by(&parse("[RAW] h=0x0012 len=1: 00")));
        assert!(!Command::Version.is_done_by(&parse("[MAP] end")));
    }

    #[test]
    fn silent_commands_have_a_check() {
        assert!(Command::PeerClear.is_silent());
        assert_eq!(Command::PeerClear.follow_up(), Some(Command::PeerQuery));
        assert!(Command::ScanStart.is_silent());
        assert!(!Command::CfgRead.is_silent());
    }
}
