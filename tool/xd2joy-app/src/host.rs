//! The host: one thread that owns the boards and does everything the window asks (`Effect`s),
//! reporting back through `HostEvent`s. It rescans USB every half second, connects boards as they
//! appear, and runs jobs such as applying a mapping or updating firmware.
//!
//! The firmware drops every byte that arrives while it is busy with a command, so the host sends
//! one command at a time per board and waits for its answer (`request`). While it waits it keeps
//! forwarding lines from both boards to the window; requests from the window that arrive in the
//! meantime are queued and run after the current job.
//!
//! The boards are reached through `Bus`, so the same host drives real USB serial ports, the demo
//! (`XD2JOY_DEMO=1`) and the tests, which use the simulated firmware in `fake.rs`.

use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

use xd2joy_device::{BoardPort, Connection, ConnectionEvent, Exchange, Outcome, list_boards};
use xd2joy_flash::{BootDrive, FAMILY_NRF52840, Uf2Image, copy_image, list_boot_drives, new_drive};
use xd2joy_proto::{
    CfgReply, Command, ConfigBlob, FirmwareInfo, PROTO_VERSION, PeerReply, ProfileReply, Reply, Side, Slot,
};

use crate::model::{Effect, FlashStage, HostEvent, side_word};

const SCAN_EVERY: Duration = Duration::from_millis(500);
/// A port that would not open is retried after this long, not on every scan.
const RETRY_OPEN: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);
const DRIVE_WAIT_AUTO: Duration = Duration::from_secs(15);
/// Long enough for someone to find a wire and short RST twice.
const DRIVE_WAIT_MANUAL: Duration = Duration::from_secs(90);
const BOARD_WAIT: Duration = Duration::from_secs(20);
/// How long `scan!` listens (`SCAN_FORCE_MS` in the firmware).
const SCAN_TIME: Duration = Duration::from_secs(20);
/// The other board follows a profile switch over the link; give it a moment before reading it.
const LINK_SETTLE: Duration = Duration::from_millis(400);
/// A blob line can be cut by other output; read it again this many times.
const CFG_TRIES: usize = 3;

pub type Sink = Arc<dyn Fn(HostEvent) + Send + Sync>;
pub type LineSink = Box<dyn Fn(ConnectionEvent) + Send + Sync + 'static>;

/// One open board.
pub trait BoardLink: Send {
    fn send(&mut self, command: &Command) -> Result<(), String>;
}

/// Where boards and bootloader drives come from.
pub trait Bus: Send + 'static {
    fn list(&mut self) -> Result<Vec<BoardPort>, String>;
    /// Open a board; `on_event` gets every line it prints, from another thread.
    fn open(&mut self, board: &BoardPort, on_event: LineSink) -> Result<Box<dyn BoardLink>, String>;
    fn boot_drives(&mut self) -> Vec<BootDrive>;
    fn copy(&mut self, drive: &Path, image: &Uf2Image) -> Result<(), String>;
}

/// The real thing: USB serial ports and mounted UF2 drives.
pub struct SerialBus;

impl BoardLink for Connection {
    fn send(&mut self, command: &Command) -> Result<(), String> {
        Connection::send(self, command).map_err(|e| e.to_string())
    }
}

impl Bus for SerialBus {
    fn list(&mut self) -> Result<Vec<BoardPort>, String> {
        list_boards().map_err(|e| e.to_string())
    }

    fn open(&mut self, board: &BoardPort, on_event: LineSink) -> Result<Box<dyn BoardLink>, String> {
        Connection::open(board.clone(), on_event).map(|c| Box::new(c) as Box<dyn BoardLink>).map_err(|e| e.to_string())
    }

    fn boot_drives(&mut self) -> Vec<BootDrive> {
        list_boot_drives()
    }

    fn copy(&mut self, drive: &Path, image: &Uf2Image) -> Result<(), String> {
        copy_image(drive, image).map_err(|e| e.to_string())
    }
}

pub(crate) enum Msg {
    Effect(Effect),
    /// From a connection's reader thread; `port` drops events from a connection already closed.
    Board {
        side: Side,
        port: String,
        event: ConnectionEvent,
    },
}

/// The window's handle to the host.
#[derive(Clone)]
pub struct HostHandle {
    tx: Sender<Msg>,
}

impl HostHandle {
    pub fn send(&self, effect: Effect) {
        if self.tx.send(Msg::Effect(effect)).is_err() {
            tracing::error!("the host thread has stopped; the request was dropped");
        }
    }
}

impl PartialEq for HostHandle {
    fn eq(&self, _: &Self) -> bool {
        // one host per process
        true
    }
}

pub fn start(bus: impl Bus, sink: Sink) -> std::io::Result<HostHandle> {
    let (tx, rx) = mpsc::channel();
    let host = Host {
        bus,
        tx: tx.clone(),
        rx,
        sink,
        live: [None, None],
        failed: Vec::new(),
        warned_duplicate: [false, false],
        deferred: VecDeque::new(),
        timers: Vec::new(),
    };
    std::thread::Builder::new().name("xd2joy-host".into()).spawn(move || host.run())?;
    Ok(HostHandle { tx })
}

struct Live {
    link: Box<dyn BoardLink>,
    port: String,
    firmware: Option<FirmwareInfo>,
    inflight: Option<Exchange>,
}

enum Timer {
    ScanOver { side: Side, port: String },
}

struct Host<B: Bus> {
    bus: B,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    sink: Sink,
    live: [Option<Live>; 2],
    /// Ports that would not open, and when.
    failed: Vec<(String, Instant)>,
    warned_duplicate: [bool; 2],
    /// Requests from the window that arrived during a job.
    deferred: VecDeque<Effect>,
    timers: Vec<(Instant, Timer)>,
}

type Job<T = ()> = Result<T, String>;

impl<B: Bus> Host<B> {
    fn emit(&self, event: HostEvent) {
        (self.sink)(event);
    }

    fn run(mut self) {
        let mut last_scan: Option<Instant> = None;
        loop {
            if let Some(effect) = self.deferred.pop_front() {
                self.effect(effect);
                continue;
            }
            match self.rx.recv_timeout(POLL) {
                Ok(Msg::Effect(effect)) => {
                    let flashed = matches!(effect, Effect::Flash { .. });
                    self.effect(effect);
                    if flashed {
                        // look for the board again straight away
                        last_scan = None;
                    }
                }
                Ok(Msg::Board { side, port, event }) => self.board_event(side, &port, event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.fire_timers();
            if last_scan.is_none_or(|t| t.elapsed() >= SCAN_EVERY) {
                self.scan();
                last_scan = Some(Instant::now());
            }
        }
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timers).into_iter().partition(|(at, _)| *at <= now);
        self.timers = later;
        for (_, timer) in due {
            match timer {
                Timer::ScanOver { side, port } => {
                    if self.live[side.index()].as_ref().is_some_and(|l| l.port == port) {
                        // the full list, in case a line was lost
                        if let Err(e) = self.run_cmd(side, Command::ScanList) {
                            tracing::warn!(?side, error = %e, "scan list");
                        }
                    }
                    self.emit(HostEvent::Scanning { side, on: false });
                }
            }
        }
    }

    fn scan(&mut self) {
        let ports = match self.bus.list() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "scan failed");
                return;
            }
        };
        self.failed.retain(|(_, at)| at.elapsed() < RETRY_OPEN);
        for side in Side::BOTH {
            let mine: Vec<&BoardPort> = ports.iter().filter(|p| p.side == side).collect();
            if let Some(live) = &self.live[side.index()]
                && !mine.iter().any(|p| p.port == live.port)
            {
                self.drop_connection(side);
            }
            let duplicate = mine.len() > 1;
            if duplicate && !self.warned_duplicate[side.index()] {
                self.emit(HostEvent::Duplicate { side, ignored: mine[1].port.clone() });
            }
            self.warned_duplicate[side.index()] = duplicate;
            if self.live[side.index()].is_none()
                && let Some(port) = mine.first()
                && !self.failed.iter().any(|(p, _)| *p == port.port)
            {
                self.connect((*port).clone());
            }
        }
    }

    fn connect(&mut self, board: BoardPort) {
        let (side, port) = (board.side, board.port.clone());
        let tx = self.tx.clone();
        let reader_port = port.clone();
        let opened = self.bus.open(
            &board,
            Box::new(move |event| {
                // the host may have stopped; nothing to do with the line then
                let _ = tx.send(Msg::Board { side, port: reader_port.clone(), event });
            }),
        );
        match opened {
            Ok(link) => {
                self.emit(HostEvent::Connected { side, port: port.clone() });
                self.live[side.index()] = Some(Live { link, port, firmware: None, inflight: None });
                self.first_read(side);
            }
            Err(e) => {
                tracing::warn!(%port, error = %e, "open failed");
                self.failed.push((port.clone(), Instant::now()));
                self.emit(HostEvent::ConnectFailed { side, port, error: e });
            }
        }
    }

    /// Everything the window shows about a board, one command at a time.
    fn first_read(&mut self, side: Side) {
        match self.request(side, Command::Version) {
            Ok(Outcome::Answered(_)) => {}
            Ok(_) => self.emit(HostEvent::NoVersion { side }),
            Err(e) => {
                tracing::warn!(?side, error = %e, "ver");
                return;
            }
        }
        if let Err(e) = self.reload(side) {
            tracing::warn!(?side, error = %e, "first read");
        }
    }

    fn reload(&mut self, side: Side) -> Job {
        self.read_cfg(side)?;
        // `input` needs the USB bridge firmware; older boards just do not answer
        for cmd in [
            Command::Mode(None),
            Command::Input(None),
            Command::Profiles,
            Command::Grip(None),
            Command::SyncQuery,
            Command::PeerQuery,
        ] {
            if let Err(e) = self.run_cmd(side, cmd) {
                tracing::debug!(?side, error = %e, "reload");
            }
        }
        Ok(())
    }

    fn drop_connection(&mut self, side: Side) {
        if self.live[side.index()].take().is_some() {
            self.emit(HostEvent::Disconnected { side });
        }
    }

    fn board_event(&mut self, side: Side, port: &str, event: ConnectionEvent) {
        let Some(live) = &mut self.live[side.index()] else { return };
        if live.port != port {
            return;
        }
        if let ConnectionEvent::Line { reply, .. } = &event {
            if let Reply::Version(info) = reply {
                live.firmware = Some(info.clone());
            }
            if let Some(ex) = &mut live.inflight {
                ex.offer(reply, Instant::now());
            }
        }
        let lost = matches!(event, ConnectionEvent::Lost(_));
        self.emit(HostEvent::Board { side, event });
        if lost {
            self.drop_connection(side);
        }
    }

    /// Send one command and wait for its answer, forwarding everything else meanwhile.
    fn request(&mut self, side: Side, cmd: Command) -> Job<Outcome> {
        let i = side.index();
        let live = self.live[i].as_mut().ok_or_else(|| not_connected(side))?;
        live.link.send(&cmd)?;
        live.inflight = Some(Exchange::new(cmd, Instant::now()));
        loop {
            let now = Instant::now();
            let ex = self.live[i].as_mut().and_then(|l| l.inflight.as_mut()).ok_or_else(|| gone(side))?;
            if let Some(outcome) = ex.poll(now) {
                if let Some(l) = self.live[i].as_mut() {
                    l.inflight = None;
                }
                return Ok(outcome);
            }
            let wait = ex.wake_at().saturating_duration_since(now).max(Duration::from_millis(1));
            match self.rx.recv_timeout(wait) {
                Ok(Msg::Effect(e)) => self.deferred.push_back(e),
                Ok(Msg::Board { side, port, event }) => self.board_event(side, &port, event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the app is closing".into()),
            }
        }
    }

    /// `request`, with a timeout as an error and a silent command confirmed by its query.
    fn run_cmd(&mut self, side: Side, cmd: Command) -> Job<Vec<Reply>> {
        let follow = cmd.follow_up();
        let text = cmd.encode();
        match self.request(side, cmd)? {
            Outcome::Answered(r) => Ok(r),
            Outcome::Silent => match follow {
                Some(f) => self.run_cmd(side, f),
                None => Ok(Vec::new()),
            },
            Outcome::TimedOut(_) => {
                Err(format!("The {} board did not answer `{}`.", side_word(side), text.trim_end()))
            }
        }
    }

    /// Let lines arrive for a while (after something that makes the other board change).
    fn pump(&mut self, how_long: Duration) {
        let until = Instant::now() + how_long;
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(Msg::Effect(e)) => self.deferred.push_back(e),
                Ok(Msg::Board { side, port, event }) => self.board_event(side, &port, event),
                Err(_) => return,
            }
        }
    }

    fn read_cfg(&mut self, side: Side) -> Job<ConfigBlob> {
        let mut last = String::new();
        for _ in 0..CFG_TRIES {
            let replies = self.run_cmd(side, Command::CfgRead)?;
            match replies.into_iter().find_map(|r| match r {
                Reply::Cfg(CfgReply::Blob(b)) => Some(b),
                _ => None,
            }) {
                Some(Ok(blob)) if blob.side == side => return Ok(blob),
                Some(Ok(_)) => {
                    return Err(format!(
                        "The {} board holds a config for the other side. Restore its defaults.",
                        side_word(side)
                    ));
                }
                Some(Err(e)) => last = e.to_string(),
                None => last = "no config line".into(),
            }
        }
        Err(format!("Couldn't read the {} board's config ({last}).", side_word(side)))
    }

    fn live_sides(&self) -> Vec<Side> {
        Side::BOTH.into_iter().filter(|s| self.live[s.index()].is_some()).collect()
    }

    fn may_write(&self, side: Side) -> Job {
        let live = self.live[side.index()].as_ref().ok_or_else(|| not_connected(side))?;
        if live.firmware.as_ref().is_some_and(|f| f.proto > PROTO_VERSION) {
            return Err(format!("The {} board runs newer firmware than this app. Update the app.", side_word(side)));
        }
        Ok(())
    }

    /// Run a job with the busy indicator, reporting its outcome.
    fn job(&mut self, title: &str, f: impl FnOnce(&mut Self) -> Job<Option<String>>) {
        self.emit(HostEvent::Working(Some(title.to_owned())));
        let result = f(self);
        self.emit(HostEvent::Working(None));
        match result {
            Ok(Some(done)) => self.emit(HostEvent::Done(done)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(job = title, error = %e, "job failed");
                self.emit(HostEvent::Failed(e));
            }
        }
    }

    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::SetMode(side, mode) => self.job("Switching mode", |h| {
                h.run_cmd(side, Command::Mode(Some(mode)))?;
                Ok(None)
            }),
            Effect::SetInput(side, source) => self.job("Switching input", |h| {
                h.may_write(side)?;
                h.run_cmd(side, Command::Input(Some(source)))?;
                Ok(None)
            }),
            Effect::Reload(side) => self.job("Reading the board", |h| {
                h.reload(side)?;
                Ok(None)
            }),
            Effect::Apply { side, blob, expected, save_slot } => {
                self.job(&format!("Applying to the {} board", side_word(side)), |h| {
                    h.apply(side, &blob, expected.as_ref(), save_slot).map(Some)
                })
            }
            Effect::RestoreDefaults(side) => {
                self.job(&format!("Restoring the {} board", side_word(side)), |h| h.restore(side).map(Some))
            }
            Effect::ProfileSwitch(slot) => self.job("Switching profile", |h| h.profile_switch(slot).map(Some)),
            Effect::ProfileSave { slot, name } => self.job("Saving the profile", |h| {
                h.on_all(|h, side| {
                    h.may_write(side)?;
                    let r = h.run_cmd(side, Command::ProfileSave(slot))?;
                    if !slot_saved(&r, slot) {
                        return Err(format!("The {} board did not save profile {}.", side_word(side), slot.number()));
                    }
                    h.run_cmd(side, Command::ProfileName(slot, name.clone()))?;
                    Ok(())
                })?;
                Ok(Some(format!("Saved the current mapping as profile {}.", slot.number())))
            }),
            Effect::ProfileRename { slot, name } => self.job("Renaming the profile", |h| {
                h.on_all(|h, side| h.run_cmd(side, Command::ProfileName(slot, name.clone())).map(drop))?;
                Ok(None)
            }),
            Effect::ProfileKey { slot, key } => self.job("Setting the profile shortcut", |h| {
                h.on_all(|h, side| h.run_cmd(side, Command::ProfileKey(slot, key)).map(drop))?;
                Ok(None)
            }),
            Effect::ProfileClear(slot) => self.job("Clearing the profile", |h| {
                h.on_all(|h, side| {
                    h.run_cmd(side, Command::ProfileKey(slot, None))?;
                    h.run_cmd(side, Command::ProfileClear(slot)).map(drop)
                })?;
                Ok(Some(format!("Profile {} cleared.", slot.number())))
            }),
            Effect::Grip(on) => self.job("Setting the charging grip", |h| {
                h.on_all(|h, side| h.run_cmd(side, Command::Grip(Some(on))).map(drop))?;
                Ok(Some("Reconnect on the console for the grip setting to be seen.".into()))
            }),
            Effect::SyncKey(key) => self.job("Setting the unpair key", |h| {
                h.on_all(|h, side| {
                    h.run_cmd(side, Command::SyncKey(key))?;
                    h.run_cmd(side, Command::SyncEnable(key.is_some())).map(drop)
                })?;
                Ok(None)
            }),
            Effect::SyncNow => self.job("Unpairing from the console", |h| {
                h.on_all(|h, side| h.run_cmd(side, Command::SyncNow).map(drop))?;
                Ok(Some(
                    "The boards forgot the console and are pairable for 2 minutes. On the Switch, open Controllers > \
                     Change Grip/Order."
                        .into(),
                ))
            }),
            Effect::Scan(side) => self.job("Listening for devices", |h| {
                h.run_cmd(side, Command::ScanStart)?;
                let port = h.live[side.index()].as_ref().map(|l| l.port.clone()).unwrap_or_default();
                h.emit(HostEvent::Scanning { side, on: true });
                h.timers.push((Instant::now() + SCAN_TIME, Timer::ScanOver { side, port }));
                Ok(None)
            }),
            Effect::Pin { side, addr, name } => self.job("Connecting the device", |h| {
                let r = h.run_cmd(side, Command::PeerSet { addr, name })?;
                let pinned = r.iter().any(|r| matches!(r, Reply::Peer(PeerReply::Pin(Some((a, _)))) if a.bytes == addr.bytes));
                if pinned {
                    Ok(None)
                } else {
                    Err(format!("The {} board did not take the device.", side_word(side)))
                }
            }),
            Effect::Unpin(side) => self.job("Forgetting the device", |h| {
                h.run_cmd(side, Command::PeerClear)?;
                Ok(None)
            }),
            Effect::Flash { side, file } => {
                self.emit(HostEvent::Flash(FlashStage::Checking { side, file: file.clone() }));
                let stage = match self.flash(side, &file) {
                    Ok(()) => FlashStage::Done { side },
                    Err(error) => {
                        tracing::warn!(?side, file = %file.display(), %error, "update failed");
                        FlashStage::Failed { side, error }
                    }
                };
                self.emit(HostEvent::Flash(stage));
            }
        }
    }

    /// Run on every connected board; at least one must be connected.
    fn on_all(&mut self, mut f: impl FnMut(&mut Self, Side) -> Job) -> Job {
        let sides = self.live_sides();
        if sides.is_empty() {
            return Err("Connect a board first.".into());
        }
        for side in sides {
            f(self, side)?;
        }
        Ok(())
    }

    /// Read, check nothing changed underneath, write, read back, and save the active profile.
    fn apply(&mut self, side: Side, blob: &ConfigBlob, expected: Option<&ConfigBlob>, save: Option<Slot>) -> Job<String> {
        self.may_write(side)?;
        let current = self.read_cfg(side)?;
        if expected.is_some_and(|e| *e != current) {
            return Err(format!(
                "The {} board's mapping changed since it was loaded (a profile switch?). Look it over and apply again.",
                side_word(side)
            ));
        }
        let cmd = Command::cfg_write(blob).map_err(|e| e.to_string())?;
        let replies = self.run_cmd(side, cmd)?;
        match replies.iter().find_map(|r| match r {
            Reply::Cfg(c) => Some(c),
            _ => None,
        }) {
            Some(CfgReply::WriteOk) => {}
            Some(CfgReply::WriteError(why)) => return Err(format!("The {} board refused it: {why}.", side_word(side))),
            _ => return Err(format!("The {} board did not confirm the write.", side_word(side))),
        }
        let back = self.read_cfg(side)?;
        if back != *blob {
            return Err(format!("The {} board stored something other than what was sent.", side_word(side)));
        }
        let mut done = format!("Applied to the {} board.", side_word(side));
        if let Some(slot) = save {
            let r = self.run_cmd(side, Command::ProfileSave(slot))?;
            if !slot_saved(&r, slot) {
                return Err(format!(
                    "Applied, but profile {} (active) was not updated, so the change is lost on restart.",
                    slot.number()
                ));
            }
            done = format!("Applied to the {} board and saved to profile {}.", side_word(side), slot.number());
        }
        Ok(done)
    }

    fn restore(&mut self, side: Side) -> Job<String> {
        self.may_write(side)?;
        let replies = self.run_cmd(side, Command::CfgDefault)?;
        if let Some(why) = replies.iter().find_map(|r| match r {
            Reply::Cfg(CfgReply::DeleteFailed(why)) => Some(why.clone()),
            _ => None,
        }) {
            return Err(format!("The {} board could not restore its defaults: {why}.", side_word(side)));
        }
        self.reload(side)?;
        Ok(format!("Restored the {} board's built-in mapping and unpair key.", side_word(side)))
    }

    fn profile_switch(&mut self, slot: Slot) -> Job<String> {
        // the left board tells the right one; either works, the left is preferred
        let side = self.live_sides().first().copied().ok_or("Connect a board first.")?;
        let replies = self.run_cmd(side, Command::ProfileLoad(slot))?;
        let switched = replies.iter().any(|r| matches!(r, Reply::Profile(ProfileReply::NowOn { slot: n, .. }) if *n == slot.number()));
        if !switched {
            return Err(format!("Profile {} is empty on the {} board.", slot.number(), side_word(side)));
        }
        self.pump(LINK_SETTLE);
        for s in self.live_sides() {
            self.read_cfg(s)?;
            self.run_cmd(s, Command::Profiles)?;
        }
        Ok(format!("Switched to profile {}.", slot.number()))
    }

    /// Check the image, get the board into its bootloader, copy, and wait for it to come back.
    /// Errors are sentences for the person.
    fn flash(&mut self, side: Side, file: &Path) -> Job {
        let bytes = std::fs::read(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
        let image = Uf2Image::parse(bytes).map_err(|e| format!("{} is not a usable UF2 file: {e}", file.display()))?;
        image.require_family(FAMILY_NRF52840).map_err(|e| e.to_string())?;

        let before = self.bus.boot_drives();
        let other = match side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        let other_port = self.live[other.index()].as_ref().map(|l| l.port.clone());
        let can_restart =
            self.live[side.index()].as_ref().and_then(|l| l.firmware.as_ref()).is_some_and(|f| f.proto >= 1);

        let wait = if can_restart {
            // the board leaves USB before it can answer; send it and stop listening
            if let Some(live) = self.live[side.index()].as_mut() {
                live.link.send(&Command::Bootloader)?;
            }
            self.drop_connection(side);
            self.emit(HostEvent::Flash(FlashStage::Restarting { side }));
            DRIVE_WAIT_AUTO
        } else {
            self.drop_connection(side);
            self.emit(HostEvent::Flash(FlashStage::NeedsManualReset { side }));
            DRIVE_WAIT_MANUAL
        };

        let deadline = Instant::now() + wait;
        let drive = loop {
            std::thread::sleep(POLL);
            if let Some(drive) = new_drive(&before, &self.bus.boot_drives()) {
                break drive;
            }
            if Instant::now() >= deadline {
                return Err(if can_restart {
                    "the board did not show up as a NICENANO drive. If your system does not mount USB drives \
                     by itself, mount it and try again."
                        .into()
                } else {
                    "no bootloader drive appeared. Short RST to GND twice, quickly, on this board.".into()
                });
            }
        };

        // Both boards' bootloaders look the same, so make sure the other board did not restart.
        if let Some(port) = other_port
            && !self.bus.list().is_ok_and(|ports| ports.iter().any(|p| p.port == port))
        {
            return Err(format!(
                "the {} board restarted instead, so nothing was copied. Unplug it and plug it back in, then try again.",
                side_word(other)
            ));
        }
        if !drive.info.looks_like_nrf52840() {
            tracing::warn!(mount = %drive.mount.display(), board_id = ?drive.info.board_id, "unexpected bootloader id");
        }

        self.emit(HostEvent::Flash(FlashStage::Copying { side, drive: drive.mount.clone() }));
        self.bus.copy(&drive.mount, &image)?;

        self.emit(HostEvent::Flash(FlashStage::WaitingForBoard { side }));
        let deadline = Instant::now() + BOARD_WAIT;
        loop {
            std::thread::sleep(POLL);
            if self.bus.list().is_ok_and(|ports| ports.iter().any(|p| p.side == side)) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("the firmware was copied but the board did not come back as an XD2Joy board. \
                            Check that the file was built for this side."
                    .into());
            }
        }
    }
}

fn slot_saved(replies: &[Reply], slot: Slot) -> bool {
    replies.iter().any(|r| matches!(r, Reply::Profile(ProfileReply::Slot(s)) if s.slot == slot.number() && s.saved))
}

fn not_connected(side: Side) -> String {
    format!("The {} board isn't connected.", side_word(side))
}

fn gone(side: Side) -> String {
    format!("The {} board went away.", side_word(side))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::fake::FakeBus;
    use xd2joy_proto::{Binding, Button, Mapping, Source, Target};

    struct Rig {
        bus: FakeBus,
        host: HostHandle,
        events: Arc<Mutex<Vec<HostEvent>>>,
    }

    impl Rig {
        fn new(bus: FakeBus) -> Rig {
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink_events = events.clone();
            let sink: Sink = Arc::new(move |e| sink_events.lock().unwrap_or_else(|p| p.into_inner()).push(e));
            let host = start(bus.clone(), sink).expect("host starts");
            Rig { bus, host, events }
        }

        /// Wait until an event matches, up to a few seconds.
        fn wait(&self, what: &str, pred: impl Fn(&HostEvent) -> bool) -> HostEvent {
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if let Some(e) = self.events.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|e| pred(e)) {
                    return e.clone();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("no {what}; got {:#?}", self.events.lock().unwrap_or_else(|p| p.into_inner()));
        }

        fn both_read(&self) {
            for side in Side::BOTH {
                self.wait("peer line", |e| {
                    matches!(e, HostEvent::Board { side: s, event: ConnectionEvent::Line { reply: Reply::Peer(PeerReply::Attached(_)), .. } } if *s == side)
                });
            }
        }

        fn received(&self, side: Side) -> Vec<String> {
            self.bus.world().boards[side.index()].received.clone()
        }
    }

    fn done(e: &HostEvent) -> bool {
        matches!(e, HostEvent::Done(_) | HostEvent::Failed(_))
    }

    #[test]
    fn connecting_reads_the_board_one_command_at_a_time() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        assert_eq!(rig.received(Side::Left), ["ver", "cfg", "mode", "input", "profile", "grip", "sync?", "peer"]);
    }

    #[test]
    fn old_firmware_is_reported_and_still_read() {
        let bus = FakeBus::new();
        bus.world().boards[1].proto = None;
        let rig = Rig::new(bus);
        rig.wait("no version", |e| matches!(e, HostEvent::NoVersion { side: Side::Right }));
        rig.both_read();
    }

    #[test]
    fn apply_writes_reads_back_and_saves_the_active_profile() {
        let bus = FakeBus::new();
        {
            let mut w = bus.world();
            let b = &mut w.boards[0];
            b.active = 2;
            b.slots[1] = Some(ConfigBlob::firmware_default(Side::Left));
            b.live = ConfigBlob::firmware_default(Side::Left);
        }
        let rig = Rig::new(bus);
        rig.both_read();
        let mut m = Mapping::firmware_default();
        m.bindings.push(Binding { source: Source::Key(0x46), target: Target::Button(Button::Capture) });
        let blob = m.to_blob(Side::Left).expect("encodes");
        rig.host.send(Effect::Apply {
            side: Side::Left,
            blob: blob.clone(),
            expected: Some(ConfigBlob::firmware_default(Side::Left)),
            save_slot: Slot::new(2),
        });
        let e = rig.wait("apply result", done);
        assert_eq!(e, HostEvent::Done("Applied to the left board and saved to profile 2.".into()));
        let w = rig.bus.world();
        assert_eq!(w.boards[0].stored.as_ref(), Some(&blob));
        assert_eq!(w.boards[0].slots[1].as_ref(), Some(&blob));
        // a restart keeps it, which is the point of saving the profile
        let mut after = w.boards[0].clone();
        after.reboot();
        assert_eq!(after.live, blob);
    }

    #[test]
    fn apply_stops_when_the_board_changed_underneath() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let mut stale = ConfigBlob::firmware_default(Side::Right);
        stale.entries.pop();
        rig.host.send(Effect::Apply {
            side: Side::Right,
            blob: stale.clone(),
            expected: Some(stale),
            save_slot: None,
        });
        let e = rig.wait("apply result", done);
        assert!(matches!(e, HostEvent::Failed(t) if t.contains("changed since it was loaded")));
        assert!(!rig.received(Side::Right).iter().any(|l| l.starts_with("cfg 55")));
    }

    #[test]
    fn a_profile_switch_on_the_left_is_followed_by_the_right() {
        let bus = FakeBus::new();
        {
            let mut w = bus.world();
            for (i, side) in Side::BOTH.into_iter().enumerate() {
                let mut p = ConfigBlob::firmware_default(side);
                p.entries.truncate(1);
                w.boards[i].slots[2] = Some(p);
            }
        }
        let rig = Rig::new(bus);
        rig.both_read();
        rig.host.send(Effect::ProfileSwitch(Slot::new(3).expect("slot")));
        let e = rig.wait("switch result", done);
        assert_eq!(e, HostEvent::Done("Switched to profile 3.".into()));
        let w = rig.bus.world();
        assert_eq!(w.boards[1].active, 3);
        assert_eq!(w.boards[1].live.entries.len(), 1);
        // both boards' configs were read again after the switch
        drop(w);
        for side in Side::BOTH {
            assert!(rig.received(side).iter().filter(|l| *l == "cfg").count() >= 2);
        }
    }

    #[test]
    fn switching_to_an_empty_profile_says_so() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        rig.host.send(Effect::ProfileSwitch(Slot::new(4).expect("slot")));
        let e = rig.wait("switch result", done);
        assert_eq!(e, HostEvent::Failed("Profile 4 is empty on the left board.".into()));
    }

    #[test]
    fn pinning_a_device_is_confirmed_by_a_query() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let addr = rig.bus.world().boards[0].heard[0].addr;
        rig.host.send(Effect::Pin { side: Side::Left, addr, name: "Keychron K3".into() });
        rig.wait("working done", |e| matches!(e, HostEvent::Working(None)));
        std::thread::sleep(Duration::from_millis(50));
        let got = rig.received(Side::Left);
        let n = got.len();
        assert_eq!(&got[n - 2..], ["peer C4:12:5A:77:10:01 random Keychron K3", "peer"]);
        assert!(rig.bus.world().boards[0].pin.is_some());
        assert!(!rig.events.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|e| matches!(e, HostEvent::Failed(_))));
    }

    #[test]
    fn firmware_update_runs_through_the_bootloader() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let dir = std::env::temp_dir().join(format!("xd2joy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("right.uf2");
        std::fs::write(&file, uf2(4)).expect("write image");
        rig.host.send(Effect::Flash { side: Side::Right, file: file.clone() });
        let e = rig.wait("flash end", |e| matches!(e, HostEvent::Flash(s) if s.is_finished()));
        assert_eq!(e, HostEvent::Flash(FlashStage::Done { side: Side::Right }));
        assert!(rig.received(Side::Right).iter().any(|l| l == "bootloader"));
        assert_eq!(rig.bus.world().boards[1].version, "v1.2-demo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A small valid nRF52840 UF2 image.
    fn uf2(n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..n {
            let mut b = vec![0u8; 512];
            let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
            put(0, 0x0A32_4655);
            put(4, 0x9E5D_5157);
            put(8, 0x2000);
            put(12, 0x2_6000 + i * 256);
            put(16, 256);
            put(20, i);
            put(24, n);
            put(28, FAMILY_NRF52840);
            put(508, 0x0AB1_6F30);
            out.extend(b);
        }
        out
    }
}
