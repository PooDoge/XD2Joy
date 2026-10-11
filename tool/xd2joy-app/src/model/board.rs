//! What the window knows about one board, filled in from the lines it prints.

use xd2joy_proto::{
    BtAddr, CfgReply, ConfigBlob, FirmwareInfo, HidState, InputStatus, LinkEvent, MapReply, Mode, PROTO_VERSION,
    PeerReply, ProfileReply, ProfileSlot, Reply, ScanDevice, ScanReply, SyncStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Link {
    #[default]
    Absent,
    Connected {
        port: String,
    },
    Failed {
        port: String,
        error: String,
    },
}

/// What firmware a board runs, as far as the tool knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Firmware {
    #[default]
    Unknown,
    Known(FirmwareInfo),
    /// No answer to `ver`.
    Old,
}

impl Firmware {
    /// Whether the board can be sent `bootloader`.
    pub fn can_restart_itself(&self) -> bool {
        matches!(self, Firmware::Known(info) if info.proto >= 1)
    }

    /// A board newer than the tool: commands may have changed shape, so the tool only reads.
    pub fn newer_than_tool(&self) -> bool {
        matches!(self, Firmware::Known(info) if info.proto > PROTO_VERSION)
    }
}

/// The board's four profile slots, from the table every `profile` command prints and from the
/// status block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profiles {
    pub active: Option<u8>,
    pub slots: [Option<ProfileSlot>; 4],
}

impl Profiles {
    pub fn slot(&self, n: u8) -> Option<&ProfileSlot> {
        self.slots.get(usize::from(n).checked_sub(1)?)?.as_ref()
    }

    /// The active slot, if it holds a saved mapping: the one the board loads at boot, which an
    /// Apply has to update too.
    pub fn active_saved(&self) -> Option<u8> {
        let n = self.active?;
        self.slot(n).is_some_and(|s| s.saved).then_some(n)
    }

    fn set(&mut self, s: ProfileSlot) {
        if let Some(cell) = usize::from(s.slot).checked_sub(1).and_then(|i| self.slots.get_mut(i)) {
            *cell = Some(s);
        }
    }
}

/// The Bluetooth keyboard or mouse link, as far as the lines tell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PairState {
    #[default]
    Unknown,
    Idle,
    Connecting,
    /// Type this code on the keyboard, then Enter.
    Passkey(u32),
    Connected,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardView {
    pub link: Link,
    pub firmware: Firmware,
    pub mode: Option<Mode>,
    pub input: Option<InputStatus>,
    /// A problem worth showing on the card (a duplicate board, a side mismatch).
    pub warning: Option<String>,
    /// The config as last read from the board. `None` until it has been read.
    pub config: Option<ConfigBlob>,
    pub profiles: Profiles,
    pub grip: Option<bool>,
    pub sync: Option<SyncStatus>,
    /// `Some(None)` once the board said nothing is pinned.
    pub pin: Option<Option<(BtAddr, String)>>,
    /// The device actually connected, its name, and whether it is the pinned one.
    pub attached: Option<Option<(BtAddr, String, bool)>>,
    pub hid: Option<HidState>,
    pub pairing: PairState,
    /// Devices heard since the last search, newest last.
    pub heard: Vec<ScanDevice>,
    pub scanning: bool,
    /// Live right-board settings from the status block (sens, msens, deadzone, speed).
    pub live_tuning: Option<LiveTuning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveTuning {
    pub stick_sens: i32,
    pub mouse_gain: u32,
    pub stick_deadzone: i32,
    pub stick_speed: i32,
}

/// Lines that only feed the status block or the live view; they would flood the console pane.
pub fn is_chatter(reply: &Reply) -> bool {
    matches!(reply, Reply::Map(_) | Reply::Hid(_) | Reply::Raw(_))
}

impl BoardView {
    pub fn connected(&self) -> bool {
        matches!(self.link, Link::Connected { .. })
    }

    /// The tool may write settings: the board is connected, its config was read, and it speaks a
    /// protocol the tool knows.
    pub fn writable(&self) -> bool {
        self.connected() && self.config.is_some() && !self.firmware.newer_than_tool()
    }

    /// Fold one reply into the view. Returns a config read from the board, for the editor.
    pub fn take(&mut self, reply: &Reply) -> Option<ConfigBlob> {
        match reply {
            Reply::Mode { mode, .. } => self.mode = Some(*mode),
            Reply::Input(status) => self.input = Some(*status),
            Reply::Cfg(CfgReply::Blob(Ok(blob))) => {
                self.config = Some(blob.clone());
                return Some(blob.clone());
            }
            Reply::Profile(p) => match p {
                ProfileReply::Active(n) => self.profiles.active = Some(*n),
                ProfileReply::Slot(s) => self.profiles.set(s.clone()),
                ProfileReply::NowOn { slot, .. } => self.profiles.active = Some(*slot),
            },
            Reply::Grip(byte) => self.grip = Some(byte.is_some()),
            Reply::Sync(s) => self.sync = Some(*s),
            Reply::Map(m) => self.take_map(m),
            Reply::Hid(h) => {
                self.hid = Some(*h);
                if *h == HidState::Subscribed && !matches!(self.pairing, PairState::Ready) {
                    self.pairing = PairState::Ready;
                }
            }
            Reply::Peer(p) => match p {
                PeerReply::Pin(pin) => self.pin = Some(pin.clone()),
                PeerReply::Attached(a) => {
                    self.attached = Some(a.clone());
                    if a.is_none() && matches!(self.pairing, PairState::Unknown) {
                        self.pairing = PairState::Idle;
                    }
                }
                PeerReply::AlsoAt { .. } => self.pairing = PairState::Connecting,
                PeerReply::PairedAt { name, addr } => {
                    self.pin = Some(Some((*addr, name.clone())));
                }
            },
            Reply::Scan(ScanReply::Heard(d) | ScanReply::Listed(d)) => {
                self.heard.retain(|h| h.addr != d.addr);
                self.heard.push(d.clone());
            }
            Reply::Link(e) => self.take_link(e),
            _ => {}
        }
        None
    }

    fn take_map(&mut self, m: &MapReply) {
        let t = self.live_tuning.get_or_insert_with(LiveTuning::default);
        match m {
            MapReply::Sync(s) => self.sync = Some(*s),
            MapReply::Grip(g) => self.grip = Some(*g),
            MapReply::ProfileActive(n) => self.profiles.active = Some(*n),
            MapReply::Profile(s) => self.profiles.set(s.clone()),
            MapReply::StickSens(v) => t.stick_sens = *v,
            MapReply::MouseGain(v) => t.mouse_gain = *v,
            MapReply::StickDeadzone(v) => t.stick_deadzone = *v,
            MapReply::StickSpeed(v) => t.stick_speed = *v,
            MapReply::Begin { .. } | MapReply::ModeKey { .. } | MapReply::Colours(_) | MapReply::End => {}
        }
    }

    fn take_link(&mut self, e: &LinkEvent) {
        self.pairing = match e {
            LinkEvent::Connecting { .. } => PairState::Connecting,
            LinkEvent::ConnectFailed { code, .. } => PairState::Failed(disconnect_reason(*code)),
            LinkEvent::Connected(_) => PairState::Connected,
            LinkEvent::Security { err: 0, .. } => PairState::Connected,
            // 2: the device forgot an old pairing; the firmware deletes the bond and retries
            LinkEvent::Security { err: 2, .. } => PairState::Connecting,
            // 4: a keyboard that needs a code; the next attempt shows it
            LinkEvent::Security { err: 4, .. } => PairState::Connecting,
            LinkEvent::Security { err, .. } => PairState::Failed(security_reason(*err)),
            LinkEvent::Passkey(code) => PairState::Passkey(*code),
            LinkEvent::Ready(n) if *n > 0 => PairState::Ready,
            LinkEvent::Ready(_) => PairState::Failed("it connected but offered no input reports".into()),
            LinkEvent::Disconnected { reason } => match self.pairing {
                // a drop during setup is a failure worth explaining; after that it is just gone
                PairState::Connecting | PairState::Connected | PairState::Passkey(_) => {
                    PairState::Failed(disconnect_reason(*reason))
                }
                _ => PairState::Idle,
            },
            LinkEvent::ConsoleConnectFailed(_) => return,
        };
    }
}

/// Bluetooth disconnect reasons, worded for people (the web tool's table, from the HCI codes).
pub fn disconnect_reason(code: u8) -> String {
    match code {
        0x05 => "pairing was refused".into(),
        0x06 => "it no longer has the pairing key".into(),
        0x08 => "the signal was lost".into(),
        0x13 => "the device hung up".into(),
        0x16 => "the board hung up".into(),
        0x22 => "the device stopped answering".into(),
        0x3E => "the connection could not be set up".into(),
        other => format!("Bluetooth error 0x{other:02X}"),
    }
}

/// Zephyr's `bt_security_err` values.
pub fn security_reason(err: u8) -> String {
    match err {
        1 => "authentication failed".into(),
        3 => "it does not support pairing".into(),
        5 => "it rejected the pairing".into(),
        6 => "it is not set up for pairing".into(),
        7 => "pairing timed out".into(),
        9 => "the pairing keys did not match".into(),
        other => format!("pairing error {other}"),
    }
}

/// A hint for a failure, from the web tool's tips.
pub fn pairing_tip(reason: &str) -> &'static str {
    if reason.contains("could not be set up") {
        "The Switch connection can block new devices. Try again with the Switch off or asleep."
    } else if reason.contains("pairing") || reason.contains("key") {
        "Reset the device's pairing (or remove it from other computers), then try again."
    } else {
        "Put it in pairing mode, then press Reconnect."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xd2joy_proto::{Side, parse_line};

    fn feed(b: &mut BoardView, lines: &[&str]) {
        for l in lines {
            let r = parse_line(l).expect("test lines parse");
            b.take(&r);
        }
    }

    #[test]
    fn profile_table_and_active_saved() {
        let mut b = BoardView::default();
        feed(
            &mut b,
            &[
                "[PROF] active 2 of 4",
                "[PROF] 1 empty  key 0x00 mods 0x00 ",
                "[PROF] 2 saved  key 0x1F mods 0x00 Racing",
                "[PROF] 3 empty  key 0x00 mods 0x00 ",
                "[PROF] 4 empty  key 0x00 mods 0x00 ",
            ],
        );
        assert_eq!(b.profiles.active_saved(), Some(2));
        assert_eq!(b.profiles.slot(2).map(|s| s.name.as_str()), Some("Racing"));
        feed(&mut b, &["[PROF] now on 1"]);
        assert_eq!(b.profiles.active_saved(), None, "slot 1 is empty");
    }

    #[test]
    fn config_reads_are_returned() {
        let mut b = BoardView::default();
        let hex = xd2joy_proto::blob::DEFAULT_LEFT_HEX;
        let r = parse_line(&format!("[CFG] blob {hex}")).expect("parses");
        assert_eq!(b.take(&r), Some(ConfigBlob::firmware_default(Side::Left)));
        assert!(b.config.is_some());
        // a broken blob changes nothing
        let r = parse_line(&format!("[CFG] blob {}", &hex[..20])).expect("parses");
        assert_eq!(b.take(&r), None);
        assert!(b.config.is_some());
    }

    #[test]
    fn pairing_states() {
        let mut b = BoardView::default();
        feed(&mut b, &["[MOUSE] HID advertiser C1:22:33:44:55:F6 (random) rssi=-50, connecting"]);
        assert_eq!(b.pairing, PairState::Connecting);
        feed(&mut b, &["[HIDLNK] PASSKEY 123456: type it on C1:22:33:44:55:F6 (random) and press Enter"]);
        assert_eq!(b.pairing, PairState::Passkey(123456));
        feed(&mut b, &["[MOUSE] *** DISCONNECTED, reason 0x05 *** reports=0"]);
        assert_eq!(b.pairing, PairState::Failed("pairing was refused".into()));
        feed(&mut b, &["[MOUSE] *** CONNECTED to C1:22:33:44:55:F6 (random) ***", "[MOUSE] security level=2 err=0"]);
        assert_eq!(b.pairing, PairState::Connected);
        feed(&mut b, &["[MOUSE] discovery done, 2 report subscriptions"]);
        assert_eq!(b.pairing, PairState::Ready);
        feed(&mut b, &["[MOUSE] *** DISCONNECTED, reason 0x13 *** reports=9"]);
        assert_eq!(b.pairing, PairState::Idle);
    }

    #[test]
    fn heard_devices_are_unique_by_address() {
        let mut b = BoardView::default();
        let l = "[SCAN] C1:22:33:44:55:F6 (random) rssi=-50 type=0 hid=1 appearance=0x03C2 name='Mouse'";
        feed(&mut b, &[l, l]);
        assert_eq!(b.heard.len(), 1);
    }

    #[test]
    fn status_block_fills_settings() {
        let mut b = BoardView::default();
        feed(&mut b, &["[MAP] sens 10", "[MAP] msens 420", "[MAP] grip 0", "[MAP] prof act 3"]);
        assert_eq!(b.grip, Some(false));
        assert_eq!(b.profiles.active, Some(3));
        assert_eq!(b.live_tuning.map(|t| t.mouse_gain), Some(420));
    }
}
