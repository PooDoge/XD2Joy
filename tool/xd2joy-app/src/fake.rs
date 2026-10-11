//! Two simulated boards that answer console commands the way the firmware does (see the protocol
//! spec): used by the demo (`XD2JOY_DEMO=1`) and by the host's tests. They keep a live config, the
//! stored one, four profile slots, grip, the unpair key, a pinned device and a scan list, and they
//! follow each other's profile switches and grip setting as the real link does.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use xd2joy_device::{BoardPort, ConnectionEvent};
use xd2joy_flash::{BootDrive, BootInfo, Uf2Image};
use xd2joy_proto::{AddrType, BtAddr, ConfigBlob, Mode, ScanDevice, Side, parse_line};

use crate::host::{BoardLink, Bus, LineSink};

#[derive(Debug, Clone)]
pub struct FakeBoard {
    pub side: Side,
    pub present: bool,
    /// `None`: firmware from before `ver` existed.
    pub proto: Option<u32>,
    pub version: String,
    pub live: ConfigBlob,
    pub stored: Option<ConfigBlob>,
    pub slots: [Option<ConfigBlob>; 4],
    pub names: [String; 4],
    pub keys: [(u8, u8); 4],
    pub active: u8,
    pub grip: bool,
    pub sync: (u8, u8, bool),
    pub mode: Mode,
    pub input: &'static str,
    pub pin: Option<(BtAddr, String)>,
    pub heard: Vec<ScanDevice>,
    /// The commands it received, for tests.
    pub received: Vec<String>,
    /// Command lines equal to one of these are lost on the way in, as when the firmware drops
    /// bytes mid-command (for tests).
    pub deaf: Vec<&'static str>,
}

impl FakeBoard {
    pub fn new(side: Side) -> FakeBoard {
        FakeBoard {
            side,
            present: true,
            proto: Some(1),
            version: "v1.1-demo".into(),
            live: ConfigBlob::firmware_default(side),
            stored: None,
            slots: [None, None, None, None],
            names: Default::default(),
            keys: [(0, 0); 4],
            active: 1,
            grip: true,
            sync: (0x45, 0, true),
            mode: Mode::Joycon,
            input: "auto",
            pin: None,
            heard: demo_devices(side),
            received: Vec::new(),
            deaf: Vec::new(),
        }
    }

    /// What a restart does: the stored config, then the active profile over it.
    pub fn reboot(&mut self) {
        self.live = self.stored.clone().unwrap_or_else(|| ConfigBlob::firmware_default(self.side));
        if let Some(Some(p)) = self.slots.get(usize::from(self.active) - 1) {
            self.live = p.clone();
        }
        self.mode = Mode::Joycon;
    }

    fn profile_table(&self) -> Vec<String> {
        let mut out = vec![format!("[PROF] active {} of 4", self.active)];
        for i in 0..4 {
            out.push(format!(
                "[PROF] {} {:<6} key 0x{:02X} mods 0x{:02X} {}",
                i + 1,
                if self.slots[i].is_some() { "saved" } else { "empty" },
                self.keys[i].0,
                self.keys[i].1,
                self.names[i]
            ));
        }
        out
    }

    fn sync_line(&self) -> String {
        format!(
            "[SYNC] shortcut key 0x{:02X} mods 0x{:02X} {}",
            self.sync.0,
            self.sync.1,
            if self.sync.2 { "on" } else { "off" }
        )
    }

    /// Run one command line; returns what it prints, and what the link tells the other board.
    fn handle(&mut self, line: &str) -> (Vec<String>, Option<LinkMsg>) {
        self.received.push(line.to_owned());
        let c = line.trim_start_matches(' ');
        if self.deaf.iter().any(|d| c.trim_end() == *d) {
            return (Vec::new(), None);
        }
        let mut out = Vec::new();
        let mut link = None;
        let num = |s: &str| -> Option<u32> {
            let s = s.trim();
            match s.strip_prefix("0x") {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => s.parse().ok(),
            }
        };
        if let Some(rest) = c.strip_prefix("cfg") {
            match rest {
                "" | "?" => out.push(format!("[CFG] blob {}", self.live.to_hex().unwrap_or_default())),
                " default" => {
                    self.stored = None;
                    self.live = ConfigBlob::firmware_default(self.side);
                    self.sync = (0x45, 0, true);
                    out.push("[CFG] stored config deleted, back to the built-in blob".into());
                    out.push(self.sync_line());
                }
                r if r.starts_with(' ') => match ConfigBlob::from_hex(r.trim()) {
                    Ok(b) if b.side == self.side => {
                        self.stored = Some(b.clone());
                        self.live = b;
                        out.push("[CFG] write ok, 160 bytes stored and live".into());
                    }
                    Ok(_) => out.push("[CFG] stored record rejected: side mismatch".into()),
                    Err(e) => out.push(format!("[CFG] write rejected: {e}")),
                },
                _ => {}
            }
        } else if let Some(rest) = c.strip_prefix("scan") {
            match rest {
                "" | "?" => {
                    out.push(format!("[SCAN] begin n={}", self.heard.len()));
                    for d in &self.heard {
                        out.push(format!(
                            "[SCAN] dev {} ({}) rssi={} hid={} appearance=0x{:04X} name='{}'",
                            d.addr.hex(),
                            d.addr.kind.peer_word(),
                            d.rssi,
                            u8::from(d.hid),
                            d.appearance,
                            d.name
                        ));
                    }
                    out.push("[SCAN] end".into());
                }
                "!" => {
                    // prints nothing now; the devices are "heard" right away in the simulation
                    for d in &self.heard {
                        out.push(format!(
                            "[SCAN] {} ({}) rssi={} type=0 hid={} appearance=0x{:04X} name='{}'",
                            d.addr.hex(),
                            d.addr.kind.peer_word(),
                            d.rssi,
                            u8::from(d.hid),
                            d.appearance,
                            d.name
                        ));
                    }
                }
                _ => {}
            }
        } else if let Some(rest) = c.strip_prefix("mode") {
            let want = match rest.trim() {
                "keyboard" | "kbd" => Some(Mode::Keyboard),
                "stick" => Some(Mode::Stick),
                "joycon" | "jc" => Some(Mode::Joycon),
                _ => None,
            };
            let word = |m: Mode| match m {
                Mode::Joycon => "JOYCON",
                Mode::Stick => "STICK",
                Mode::Keyboard => "KEYBOARD",
            };
            match want {
                None => out.push(format!("[MODE] {}", word(self.mode))),
                Some(m) if m == self.mode => out.push(format!("[MODE] already {}", word(m))),
                Some(m) => {
                    self.mode = m;
                    out.push(format!("[MODE] now {}, joy-con input live", word(m)));
                }
            }
        } else if let Some(rest) = c.strip_prefix("profile") {
            let mut w = rest.split_whitespace();
            let verb = w.next();
            let n = w.next().and_then(num).filter(|n| (1..=4).contains(n)).map(|n| n as usize);
            match (verb, n) {
                (Some("save"), Some(n)) => self.slots[n - 1] = Some(self.live.clone()),
                (Some("load"), Some(n)) => {
                    if let Some(p) = self.slots[n - 1].clone() {
                        self.live = p;
                        self.active = n as u8;
                        let name = &self.names[n - 1];
                        out.push(format!("[PROF] now on {n}{}{name}", if name.is_empty() { "" } else { ": " }));
                        link = Some(LinkMsg::Profile(n as u8));
                    }
                }
                (Some("clear"), Some(n)) => {
                    self.slots[n - 1] = None;
                    self.names[n - 1].clear();
                    self.keys[n - 1] = (0, 0);
                }
                (Some("name"), Some(n)) => {
                    let name = rest.trim_start().splitn(3, ' ').nth(2).unwrap_or("");
                    self.names[n - 1] = name.chars().take(51).collect();
                }
                (Some("key"), Some(n)) => {
                    let code = w.next().and_then(num).unwrap_or(0) as u8;
                    let mods = w.next().and_then(num).unwrap_or(0) as u8;
                    self.keys[n - 1] = (code, mods);
                }
                _ => {}
            }
            out.extend(self.profile_table());
        } else if let Some(rest) = c.strip_prefix("grip") {
            match rest.trim() {
                "on" | "off" => {
                    self.grip = rest.trim() == "on";
                    link = Some(LinkMsg::Grip(self.grip));
                }
                "" => {}
                // `grip?` and typos set the byte to 0, as the firmware does
                _ => self.grip = true,
            }
            out.push(if self.grip {
                "[GRIP] byte 0x04 = 0x01 (IN THE CHARGING GRIP, as a real one reports)".into()
            } else {
                "[GRIP] off: byte 0x04 replayed (0x07, not in a grip)".into()
            });
        } else if let Some(rest) = c.strip_prefix("sync") {
            match rest {
                "?" => out.push(self.sync_line()),
                " on" => {
                    self.sync.2 = true;
                    out.push(self.sync_line());
                }
                " off" => {
                    self.sync.2 = false;
                    out.push(self.sync_line());
                }
                r if r.starts_with(" key ") => {
                    let mut w = r.split_whitespace().skip(1);
                    if let (Some(k), m) = (w.next().and_then(num), w.next().and_then(num).unwrap_or(0)) {
                        self.sync.0 = k as u8;
                        self.sync.1 = m as u8;
                    }
                    out.push(self.sync_line());
                }
                // bare `sync`: pairing window, prints nothing
                _ => {}
            }
        } else if let Some(rest) = c.strip_prefix("input") {
            if let w @ ("auto" | "bt" | "usb") = rest.trim() {
                self.input = match w {
                    "auto" => "auto",
                    "bt" => "bt",
                    _ => "usb",
                };
            }
            out.push(format!("[INPUT] src={} wired=none bridge=0x00 frames=0 bad=0", self.input));
        } else if (c == "ver" || c == "ver?") && self.proto.is_some() {
            out.push(format!(
                "[VER] fw {} proto {} side {}",
                self.version,
                self.proto.unwrap_or(1),
                self.side.letter()
            ));
        } else if c == "bootloader" && self.proto.is_some() {
            out.push("[BOOT] restarting into the UF2 bootloader".into());
            link = Some(LinkMsg::Bootloader);
        } else if let Some(rest) = c.strip_prefix("peer") {
            match rest.trim() {
                "" | "?" => {
                    match &self.pin {
                        Some((a, n)) => out.push(format!("[PEER] pin={} ({}) name='{n}'", a.hex(), a.kind.peer_word())),
                        None => out.push("[PEER] pin=none".into()),
                    }
                    out.push("[PEER] attached=none".into());
                }
                "none" | "any" => self.pin = None,
                r => {
                    let mut w = r.splitn(3, ' ');
                    let addr = w.next().unwrap_or("");
                    let kind = w.next().unwrap_or("random");
                    let name = w.next().unwrap_or("").to_owned();
                    if let Some((a, _)) = BtAddr::parse_prefix(&format!("{addr} ({kind})")) {
                        self.pin = Some((a, name));
                    }
                }
            }
        }
        (out, link)
    }
}

enum LinkMsg {
    Profile(u8),
    Grip(bool),
    Bootloader,
}

fn demo_devices(side: Side) -> Vec<ScanDevice> {
    let dev = |b: u8, kind, rssi, appearance, name: &str| ScanDevice {
        addr: BtAddr { bytes: [0xC4, 0x12, 0x5A, 0x77, 0x10, b], kind },
        rssi,
        hid: appearance != 0,
        appearance,
        name: name.into(),
    };
    match side {
        Side::Left => vec![
            dev(1, AddrType::Random, -48, 0x03C1, "Keychron K3"),
            dev(2, AddrType::Public, -71, 0x03C1, "Logi K380"),
            dev(3, AddrType::Random, -80, 0, "Pixel 9"),
        ],
        Side::Right => vec![
            dev(4, AddrType::Random, -45, 0x03C2, "MX Master 3S"),
            dev(5, AddrType::Random, -66, 0x03C2, "Razer Basilisk"),
        ],
    }
}

/// Both boards plus a pretend UF2 drive.
pub struct FakeWorld {
    pub boards: [FakeBoard; 2],
    sinks: [Option<Arc<LineSink>>; 2],
    /// Set while a board sits in its bootloader.
    pub drive: Option<Side>,
}

impl FakeWorld {
    pub fn new() -> FakeWorld {
        FakeWorld {
            boards: [FakeBoard::new(Side::Left), FakeBoard::new(Side::Right)],
            sinks: [None, None],
            drive: None,
        }
    }
}

#[derive(Clone)]
pub struct FakeBus(pub Arc<Mutex<FakeWorld>>);

impl FakeBus {
    pub fn new() -> FakeBus {
        FakeBus(Arc::new(Mutex::new(FakeWorld::new())))
    }

    pub fn world(&self) -> MutexGuard<'_, FakeWorld> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Print lines from a board as if it said them by itself.
    pub fn say(&self, side: Side, lines: &[String]) {
        let sink = self.world().sinks[side.index()].clone();
        if let Some(sink) = sink {
            for l in lines {
                deliver(&sink, l);
            }
        }
    }
}

fn deliver(sink: &LineSink, raw: &str) {
    let event = match parse_line(raw) {
        Ok(reply) => ConnectionEvent::Line { raw: raw.to_owned(), reply },
        Err(e) => ConnectionEvent::Unreadable { raw: raw.to_owned(), error: e.to_string() },
    };
    sink(event);
}

fn port_of(side: Side) -> String {
    format!("/dev/fake-{}", side.letter())
}

struct FakeLink {
    bus: FakeBus,
    side: Side,
}

impl BoardLink for FakeLink {
    fn send(&mut self, command: &xd2joy_proto::Command) -> Result<(), String> {
        let line = command.encode();
        let line = line.trim_end_matches('\n');
        let (own, other) = {
            let mut w = self.bus.world();
            if !w.boards[self.side.index()].present {
                return Err("the port is gone".into());
            }
            let (out, link) = w.boards[self.side.index()].handle(line);
            let other_side = match self.side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
            };
            let mut other_lines = Vec::new();
            match &link {
                Some(LinkMsg::Profile(n)) => {
                    let o = &mut w.boards[other_side.index()];
                    if o.present {
                        o.active = *n;
                        if let Some(p) = o.slots[usize::from(*n) - 1].clone() {
                            o.live = p;
                            other_lines.push(format!("[PROF] now on {n}"));
                        }
                    }
                }
                Some(LinkMsg::Grip(on)) => w.boards[other_side.index()].grip = *on,
                Some(LinkMsg::Bootloader) => {
                    w.boards[self.side.index()].present = false;
                    w.drive = Some(self.side);
                    w.sinks[self.side.index()] = None;
                }
                None => {}
            }
            let own = w.sinks[self.side.index()].clone();
            let other = w.sinks[other_side.index()].clone().map(|s| (s, other_lines));
            (own.map(|s| (s, out)), other)
        };
        // deliver outside the lock: the sink may be slow
        if let Some((sink, lines)) = own {
            for l in lines {
                deliver(&sink, &l);
            }
        }
        if let Some((sink, lines)) = other {
            for l in lines {
                deliver(&sink, &l);
            }
        }
        Ok(())
    }
}

impl Bus for FakeBus {
    fn list(&mut self) -> Result<Vec<BoardPort>, String> {
        let w = self.world();
        Ok(w.boards
            .iter()
            .filter(|b| b.present)
            .map(|b| BoardPort { port: port_of(b.side), side: b.side, serial_number: None })
            .collect())
    }

    fn open(&mut self, board: &BoardPort, on_event: LineSink) -> Result<Box<dyn BoardLink>, String> {
        let mut w = self.world();
        if !w.boards[board.side.index()].present {
            return Err("no such port".into());
        }
        w.sinks[board.side.index()] = Some(Arc::new(on_event));
        Ok(Box::new(FakeLink { bus: self.clone(), side: board.side }))
    }

    fn boot_drives(&mut self) -> Vec<BootDrive> {
        let w = self.world();
        w.drive
            .map(|side| BootDrive {
                mount: PathBuf::from(format!("/media/fake/NICENANO-{}", side.letter())),
                info: BootInfo::parse(
                    "UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0)\nModel: nice!nano\nBoard-ID: nRF52840-nicenano\n",
                ),
            })
            .into_iter()
            .collect()
    }

    fn copy(&mut self, _drive: &Path, _image: &Uf2Image) -> Result<(), String> {
        let mut w = self.world();
        let side = w.drive.take().ok_or("no drive")?;
        let b = &mut w.boards[side.index()];
        b.present = true;
        b.proto = Some(1);
        b.version = "v1.2-demo".into();
        b.reboot();
        Ok(())
    }
}
