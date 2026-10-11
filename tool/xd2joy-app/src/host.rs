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
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

use xd2joy_device::{BoardPort, Connection, ConnectionEvent, Exchange, Outcome, list_boards};
use xd2joy_flash::{BootDrive, FAMILY_NRF52840, Uf2Image, check_layout, copy_image, list_boot_drives, new_drive};
use xd2joy_proto::{
    CfgReply, Command, ConfigBlob, FirmwareInfo, PROTO_VERSION, PeerReply, ProfileReply, Reply, Side, Slot,
};

use crate::logs;
use crate::model::{Effect, FlashStage, HostEvent, side_word};

const SCAN_EVERY: Duration = Duration::from_millis(500);
/// A port that would not open is retried after this long, not on every scan.
const RETRY_OPEN: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);
const DRIVE_WAIT_AUTO: Duration = Duration::from_secs(15);
/// Long enough for someone to double-press reset, or find a wire and short RST twice.
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
    /// Where each board's last config is remembered; `None` remembers nothing (tests, the demo).
    fn cache_file(&self) -> Option<PathBuf> {
        None
    }
}

/// The real thing: USB serial ports and mounted UF2 drives.
pub struct SerialBus;

impl BoardLink for Connection {
    fn send(&mut self, command: &Command) -> Result<(), String> {
        Connection::send(self, command).map_err(|e| e.to_string())
    }
}

impl Bus for SerialBus {
    fn cache_file(&self) -> Option<PathBuf> {
        crate::cache::path()
    }

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
        hidden: [false; 2],
        skipped: None,
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
    /// A board `on_all` skipped because it was not connected, for the job's message.
    skipped: Option<Side>,
    timers: Vec<(Instant, Timer)>,
    /// Boards temporarily holding a config the person did not choose (a preset being saved into a
    /// slot): their config reads are not shown to the window or remembered on disk.
    hidden: [bool; 2],
}

type Job<T = ()> = Result<T, String>;

impl<B: Bus> Host<B> {
    fn emit(&self, event: HostEvent) {
        log_event(&event);
        if let HostEvent::Board { side, event: ConnectionEvent::Line { reply: Reply::Cfg(CfgReply::Blob(_)), .. } } =
            &event
            && self.hidden[side.index()]
        {
            return;
        }
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
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.timers).into_iter().partition(|(at, _)| *at <= now);
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
        // The port enumerates before the firmware's console is listening, so a first `ver` can
        // be lost; ask again before calling the firmware old.
        let mut answer = self.request(side, Command::Version);
        for _ in 0..VER_RETRIES {
            if !matches!(answer, Ok(Outcome::TimedOut(_) | Outcome::Silent)) {
                break;
            }
            self.pump(VER_RETRY_GAP);
            answer = self.request(side, Command::Version);
        }
        match answer {
            Ok(Outcome::Answered(_)) => {}
            Ok(_) => self.emit(HostEvent::NoVersion { side }),
            Err(e) => {
                tracing::warn!(?side, error = %e, "ver");
                self.emit(HostEvent::Failed(format!("Couldn't talk to the {} board: {e}", side_word(side))));
                return;
            }
        }
        if let Err(e) = self.reload(side) {
            tracing::warn!(?side, error = %e, "first read");
            self.emit(HostEvent::Failed(format!(
                "Couldn't read the {} board's settings: {e} Try Read again on the Boards page.",
                side_word(side)
            )));
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
        tracing::debug!(?side, line = %cmd.encode().trim_end(), "send");
        live.link.send(&cmd)?;
        live.inflight = Some(Exchange::new(cmd, Instant::now()));
        loop {
            let now = Instant::now();
            let ex = self.live[i].as_mut().and_then(|l| l.inflight.as_mut()).ok_or_else(|| gone(side))?;
            if let Some(outcome) = ex.poll(now) {
                if let Some(l) = self.live[i].as_mut() {
                    l.inflight = None;
                }
                match &outcome {
                    Outcome::Answered(r) => tracing::debug!(?side, lines = r.len(), "answered"),
                    Outcome::Silent => tracing::debug!(?side, "no answer expected"),
                    Outcome::TimedOut(r) => tracing::warn!(?side, lines = r.len(), "command timed out"),
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
            Outcome::TimedOut(_) => Err(format!("The {} board did not answer `{}`.", side_word(side), text.trim_end())),
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
                Some(Ok(blob)) if blob.side == side => {
                    self.remember(&blob);
                    return Ok(blob);
                }
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

    fn remember(&self, blob: &ConfigBlob) {
        if self.hidden[blob.side.index()] {
            return;
        }
        let Some(file) = self.bus.cache_file() else { return };
        let firmware =
            self.live[blob.side.index()].as_ref().and_then(|l| l.firmware.as_ref()).map(|f| f.version.clone());
        // losing the cache only costs editing while unplugged, so it is logged, not shown
        if let Err(e) = crate::cache::save(&file, blob, firmware.as_deref()) {
            tracing::warn!(error = %e, "could not remember the board's config");
        }
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
        self.skipped = None;
        let result = f(self);
        self.emit(HostEvent::Working(None));
        let skipped = self.skipped.take().map(|s| {
            format!("The {} board isn't connected, so it wasn't changed; do this again once it is.", side_word(s))
        });
        match (result, skipped) {
            (Ok(Some(done)), Some(note)) => self.emit(HostEvent::Done(format!("{done} {note}"))),
            (Ok(None), Some(note)) => self.emit(HostEvent::Done(note)),
            (Ok(Some(done)), None) => self.emit(HostEvent::Done(done)),
            (Ok(None), None) => {}
            (Err(e), _) => {
                tracing::warn!(job = title, error = %e, "job failed");
                self.emit(HostEvent::Failed(e));
            }
        }
    }

    fn effect(&mut self, effect: Effect) {
        tracing::info!(effect = %effect.summary(), "request from the window");
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
            Effect::Apply { side, blob, expected, save_slot } => self
                .job(&format!("Applying to the {} board", side_word(side)), |h| {
                    h.apply(side, &blob, expected.as_ref(), save_slot).map(Some)
                }),
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
            Effect::ProfileFromPreset { slot, name, blobs } => self.job("Filling the profile from the preset", |h| {
                let skipped: Vec<Side> =
                    Side::BOTH.into_iter().filter(|s| !blobs.iter().any(|b| b.side == *s)).collect();
                let mut done = h.profile_from_preset(slot, &name, &blobs)?;
                if let Some(s) = skipped.first() {
                    done.push_str(&format!(
                        " The {} board wasn't connected, so its half of the profile is unchanged.",
                        side_word(*s)
                    ));
                }
                Ok(Some(done))
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
                // `sync` prints nothing, so this can't be confirmed
                Ok(Some(
                    "Sent. The boards should now be pairable for 2 minutes: on the Switch, open Controllers > \
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
                let pinned =
                    r.iter().any(|r| matches!(r, Reply::Peer(PeerReply::Pin(Some((a, _)))) if a.bytes == addr.bytes));
                if pinned { Ok(None) } else { Err(format!("The {} board did not take the device.", side_word(side))) }
            }),
            Effect::Unpin(side) => self.job("Forgetting the device", |h| {
                // `peer none` prints nothing; its follow-up `peer` shows the pin
                let r = h.run_cmd(side, Command::PeerClear)?;
                if r.iter().any(|r| matches!(r, Reply::Peer(PeerReply::Pin(Some(_))))) {
                    return Err(format!("The {} board still has the device.", side_word(side)));
                }
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
        self.skipped = Side::BOTH.into_iter().find(|s| !sides.contains(s));
        for &side in &sides {
            f(self, side).map_err(|e| match (side, sides.first()) {
                // the first board already took it; say so, since nothing undoes that
                (Side::Right, Some(Side::Left)) => format!("{e} The left board was already changed."),
                _ => e,
            })?;
        }
        Ok(())
    }

    /// Read, check nothing changed underneath, write, read back, and save the active profile.
    fn apply(
        &mut self,
        side: Side,
        blob: &ConfigBlob,
        expected: Option<&ConfigBlob>,
        save: Option<Slot>,
    ) -> Job<String> {
        self.may_write(side)?;
        let current = self.read_cfg(side)?;
        if expected.is_some_and(|e| *e != current) {
            return Err(format!(
                "The {} board's mapping changed since it was loaded (a profile switch?). Look it over and apply again.",
                side_word(side)
            ));
        }
        self.write_cfg(side, blob)?;
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

    /// Write a config and read it back.
    fn write_cfg(&mut self, side: Side, blob: &ConfigBlob) -> Job {
        let cmd = Command::cfg_write(blob).map_err(|e| e.to_string())?;
        // An unanswered write may still have landed; the read-back below decides.
        let replies = match self.run_cmd(side, cmd) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(?side, error = %e, "config write unanswered; checking what the board holds");
                Vec::new()
            }
        };
        if let Some(why) = replies.iter().find_map(|r| match r {
            Reply::Cfg(CfgReply::WriteError(why)) => Some(why),
            _ => None,
        }) {
            return Err(format!("The {} board refused it: {why}.", side_word(side)));
        }
        let back = self.read_cfg(side)?;
        if back != *blob {
            return Err(format!("The {} board stored something other than what was sent.", side_word(side)));
        }
        Ok(())
    }

    /// Put a preset into a profile slot on each board it has a config for, then give the board
    /// back what it was running, unless that slot is the active one.
    fn profile_from_preset(&mut self, slot: Slot, name: &str, blobs: &[ConfigBlob]) -> Job<String> {
        // checked up front, so a board that cannot be written does not leave the other half done
        for blob in blobs {
            self.may_write(blob.side)?;
        }
        let mut filled: Vec<Side> = Vec::new();
        for blob in blobs {
            let side = blob.side;
            if let Err(e) = self.fill_one(side, slot, name, blob) {
                return Err(match filled.as_slice() {
                    [] => e,
                    done => {
                        format!("{e} The {} board's profile {} was already filled.", side_word(done[0]), slot.number())
                    }
                });
            }
            filled.push(side);
        }
        Ok(format!("Profile {} now holds the preset \"{name}\".", slot.number()))
    }

    /// Save `blob` into `slot` on one board: write it, `profile save`, then put back what the board
    /// was running unless `slot` is the active profile (whose saved copy the board loads at boot
    /// anyway). The put-back is attempted whatever failed before it.
    fn fill_one(&mut self, side: Side, slot: Slot, name: &str, blob: &ConfigBlob) -> Job {
        let original = self.read_cfg(side)?;
        let table = self.run_cmd(side, Command::Profiles)?;
        let Some(active) = table.iter().find_map(|r| match r {
            Reply::Profile(ProfileReply::Active(n)) => Some(*n),
            _ => None,
        }) else {
            // guessing wrong here would leave the board on the preset after a restart
            return Err(format!("The {} board did not say which profile is active; nothing changed.", side_word(side)));
        };
        let keep = active == slot.number();
        self.hidden[side.index()] = !keep;
        let stored = self.write_cfg(side, blob).and_then(|()| self.run_cmd(side, Command::ProfileSave(slot)));
        let restored = if keep { Ok(()) } else { self.write_cfg(side, &original) };
        self.hidden[side.index()] = false;
        if keep || restored.is_err() {
            // the board now runs a config the window has not seen: show and remember it
            if let Err(e) = self.read_cfg(side) {
                tracing::warn!(?side, error = %e, "read after filling a profile");
            }
        }
        let stuck =
            || format!(" The {} board is now running the preset; Apply or switch profile to fix it.", side_word(side));
        let saved = match (stored, restored) {
            (Ok(r), Ok(())) => slot_saved(&r, slot),
            (Err(e), Ok(())) => return Err(e),
            (Ok(_), Err(e)) => return Err(format!("{e}{}", stuck())),
            (Err(e), Err(_)) => return Err(format!("{e}{}", stuck())),
        };
        if !saved {
            return Err(format!("The {} board did not save profile {}.", side_word(side), slot.number()));
        }
        self.run_cmd(side, Command::ProfileName(slot, name.to_owned()))?;
        Ok(())
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
        let table = self.run_cmd(side, Command::Profiles)?;
        let mut done = format!("Restored the {} board's built-in mapping and unpair key.", side_word(side));
        // The active profile is loaded at boot over the restored mapping. Overwriting it here would
        // destroy a profile the person may want, so say it instead.
        if let Some(n) = active_saved(&table) {
            done.push_str(&format!(
                " Profile {n} is active and saved, so the board goes back to it when restarted. To keep the \
                 defaults, save them over profile {n} or clear it."
            ));
        }
        Ok(done)
    }

    fn profile_switch(&mut self, slot: Slot) -> Job<String> {
        // the left board tells the right one; either works, the left is preferred
        let side = self.live_sides().first().copied().ok_or("Connect a board first.")?;
        let replies = self.run_cmd(side, Command::ProfileLoad(slot))?;
        let switched = replies
            .iter()
            .any(|r| matches!(r, Reply::Profile(ProfileReply::NowOn { slot: n, .. }) if *n == slot.number()));
        if !switched {
            return Err(format!("Profile {} is empty on the {} board.", slot.number(), side_word(side)));
        }
        self.pump(LINK_SETTLE);
        let mut empty_on = None;
        for s in self.live_sides() {
            self.read_cfg(s)?;
            let table = self.run_cmd(s, Command::Profiles)?;
            if !slot_saved(&table, slot) {
                empty_on = Some(s);
            }
        }
        match empty_on {
            // a board told to load a slot it never saved keeps its mapping and says nothing
            Some(s) => Ok(format!(
                "Switched to profile {}, but it is empty on the {} board, which kept its mapping. Save the profile \
                 again with both boards connected.",
                slot.number(),
                side_word(s)
            )),
            None => Ok(format!("Switched to profile {}.", slot.number())),
        }
    }

    /// Check the image, get the board into its bootloader, copy, and wait for it to come back.
    /// Errors are sentences for the person.
    fn flash(&mut self, side: Side, file: &Path) -> Job {
        let bytes = std::fs::read(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
        let image = Uf2Image::parse(bytes).map_err(|e| format!("{} is not a usable UF2 file: {e}", file.display()))?;
        image.require_family(FAMILY_NRF52840).map_err(|e| e.to_string())?;
        // The image carries no side marker; release files are named xd2joy_LEFT.uf2 / _RIGHT.uf2.
        let name = file.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
        let wrong = match side {
            Side::Left => name.contains("RIGHT"),
            Side::Right => name.contains("LEFT"),
        };
        if wrong {
            return Err(format!(
                "{} looks like firmware for the other board. Pick the {} file.",
                file.display(),
                side_word(side)
            ));
        }

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
                tracing::debug!(?side, line = %Command::Bootloader.encode().trim_end(), "send");
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
                    "the board did not show up as a bootloader drive (NICENANO or XIAO-SENSE). If your system \
                     does not mount USB drives by itself, mount it and try again."
                        .into()
                } else {
                    "no bootloader drive appeared. Double-press the board's reset button (on a Pro Micro, short RST to \
                     GND twice), quickly."
                        .into()
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
        tracing::info!(
            mount = %drive.mount.display(),
            model = ?drive.info.model,
            board_id = ?drive.info.board_id,
            softdevice = ?drive.info.softdevice,
            image_start = format_args!("{:#x}", image.span().0),
            "bootloader drive"
        );
        if !drive.info.looks_like_nrf52840() {
            tracing::warn!(mount = %drive.mount.display(), board_id = ?drive.info.board_id, "unexpected bootloader id");
        }
        if let Err(e) = check_layout(&drive.info, &image) {
            return Err(format!(
                "{e}. Nothing was copied. Unplug the {} board and plug it back in to leave its bootloader; it \
                 still has its old firmware. For a Seeed XIAO, build with XD2JOY_BOARD=xiao.",
                side_word(side)
            ));
        }

        self.emit(HostEvent::Flash(FlashStage::Copying { side, drive: drive.mount.clone() }));
        self.bus.copy(&drive.mount, &image).map_err(|e| {
            format!(
                "{e}. The board is still in its bootloader with the drive at {}: copy the file onto that drive \
                 by hand, or unplug and replug the board to try again.",
                drive.mount.display()
            )
        })?;

        self.emit(HostEvent::Flash(FlashStage::WaitingForBoard { side }));
        let deadline = Instant::now() + BOARD_WAIT;
        loop {
            std::thread::sleep(POLL);
            if self.bus.list().is_ok_and(|ports| ports.iter().any(|p| p.side == side)) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("the firmware was copied but the board did not come back as an XD2Joy board. \
                            Unplug it and plug it back in. If its bootloader drive shows up again, copy the \
                            file onto it by hand; if nothing shows up, check the file was built for this \
                            side and this kind of board."
                    .into());
            }
        }
    }
}

/// One log line per event, at a level that matches how much it matters later.
fn log_event(event: &HostEvent) {
    match event {
        HostEvent::Board { side, event: ConnectionEvent::Line { raw, reply } } => {
            if matches!(reply, Reply::Raw(_)) {
                tracing::trace!(?side, line = %raw, "recv");
            } else {
                tracing::debug!(?side, line = %logs::mask_passkeys(raw), "recv");
            }
        }
        HostEvent::Board { side, event: ConnectionEvent::Unreadable { raw, error } } => {
            tracing::warn!(?side, line = %logs::mask_passkeys(raw), %error, "unreadable line");
        }
        HostEvent::Board { side, event: ConnectionEvent::Lost(why) } => tracing::warn!(?side, %why, "connection lost"),
        HostEvent::Connected { side, port } => tracing::info!(?side, %port, "board connected"),
        HostEvent::ConnectFailed { side, port, error } => tracing::warn!(?side, %port, %error, "could not open board"),
        HostEvent::Disconnected { side } => tracing::info!(?side, "board disconnected"),
        HostEvent::NoVersion { side } => tracing::warn!(?side, "no answer to ver: firmware older than this app"),
        HostEvent::Duplicate { side, ignored } => tracing::warn!(?side, %ignored, "second board for one side ignored"),
        HostEvent::Flash(stage) => match stage {
            FlashStage::Failed { .. } => tracing::warn!(side = ?stage.side(), stage = %stage.describe(), "update"),
            _ => tracing::info!(side = ?stage.side(), stage = %stage.describe(), "update"),
        },
        HostEvent::Working(Some(job)) => tracing::info!(%job, "job started"),
        HostEvent::Working(None) => tracing::debug!("job finished"),
        HostEvent::Done(text) => tracing::info!(%text, "job done"),
        HostEvent::Failed(text) => tracing::warn!(%text, "job failed"),
        HostEvent::Scanning { side, on } => tracing::info!(?side, on, "bluetooth search"),
        HostEvent::CloseRefused(_) => {}
    }
}

/// Extra `ver` attempts after the first, and the wait between them.
const VER_RETRIES: usize = 2;
const VER_RETRY_GAP: Duration = Duration::from_millis(800);

/// The active slot from a `profile` table, if it holds a saved mapping.
fn active_saved(replies: &[Reply]) -> Option<u8> {
    let active = replies.iter().find_map(|r| match r {
        Reply::Profile(ProfileReply::Active(n)) => Some(*n),
        _ => None,
    })?;
    Slot::new(active).filter(|s| slot_saved(replies, *s)).map(Slot::number)
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
            let until = Instant::now() + Duration::from_secs(10);
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
        rig.host.send(Effect::Apply { side: Side::Right, blob: stale.clone(), expected: Some(stale), save_slot: None });
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
    fn a_preset_fills_a_slot_without_changing_what_the_board_runs() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let running = rig.bus.world().boards[0].live.clone();
        let mut preset = ConfigBlob::firmware_default(Side::Left);
        preset.colours.body = xd2joy_proto::Rgb([1, 2, 3]);
        let slot = Slot::new(3).expect("slot");
        rig.host.send(Effect::ProfileFromPreset { slot, name: "Racing".into(), blobs: vec![preset.clone()] });
        let e = rig.wait("fill result", done);
        assert!(
            matches!(&e, HostEvent::Done(t) if t.contains("Profile 3") && t.contains("right board wasn't")),
            "{e:?}"
        );
        let world = rig.bus.world();
        assert_eq!(world.boards[0].live, running, "the board runs what it ran before");
        assert_eq!(world.boards[0].slots[2].as_ref(), Some(&preset));
        assert_eq!(world.boards[0].names[2], "Racing");
    }

    #[test]
    fn a_lost_profile_save_still_puts_back_what_the_board_ran() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let running = rig.bus.world().boards[0].live.clone();
        rig.bus.world().boards[0].deaf.push("profile save 3");
        let mut preset = ConfigBlob::firmware_default(Side::Left);
        preset.colours.body = xd2joy_proto::Rgb([1, 2, 3]);
        let slot = Slot::new(3).expect("slot");
        rig.host.send(Effect::ProfileFromPreset { slot, name: "Racing".into(), blobs: vec![preset] });
        let e = rig.wait("fill result", done);
        assert!(matches!(e, HostEvent::Failed(_)), "{e:?}");
        let world = rig.bus.world();
        assert_eq!(world.boards[0].live, running);
        assert_eq!(world.boards[0].slots[2], None);
    }

    #[test]
    fn filling_a_profile_needs_to_know_the_active_one() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let running = rig.bus.world().boards[0].live.clone();
        rig.bus.world().boards[0].deaf.push("profile");
        let slot = Slot::new(3).expect("slot");
        let preset = ConfigBlob::firmware_default(Side::Left);
        rig.host.send(Effect::ProfileFromPreset { slot, name: "Racing".into(), blobs: vec![preset] });
        let e = rig.wait("fill result", done);
        assert!(matches!(e, HostEvent::Failed(_)), "{e:?}");
        assert_eq!(rig.bus.world().boards[0].live, running);
        assert_eq!(rig.bus.world().boards[0].slots[2], None);
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
        assert!(
            !rig.events.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|e| matches!(e, HostEvent::Failed(_)))
        );
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

    #[test]
    fn firmware_for_another_flash_layout_is_not_copied() {
        let bus = FakeBus::new();
        bus.world().boot_info = crate::fake::BOOT_XIAO;
        let rig = Rig::new(bus);
        rig.both_read();
        let before = rig.bus.world().boards[1].version.clone();
        let dir = std::env::temp_dir().join(format!("xd2joy-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("xd2joy_RIGHT.uf2");
        // linked at 0x26000 (Pro Micro); the XIAO's bootloader starts firmware at 0x27000
        std::fs::write(&file, uf2(4)).expect("write image");
        rig.host.send(Effect::Flash { side: Side::Right, file });
        let e = rig.wait("flash end", |e| matches!(e, HostEvent::Flash(s) if s.is_finished()));
        assert!(matches!(&e, HostEvent::Flash(FlashStage::Failed { error, .. }) if error.contains("0x27000")), "{e:?}");
        assert_eq!(rig.bus.world().boards[1].version, before, "nothing was copied");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn firmware_named_for_the_other_board_is_refused() {
        let rig = Rig::new(FakeBus::new());
        rig.both_read();
        let dir = std::env::temp_dir().join(format!("xd2joy-side-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("xd2joy_LEFT.uf2");
        std::fs::write(&file, uf2(4)).expect("write image");
        rig.host.send(Effect::Flash { side: Side::Right, file });
        let e = rig.wait("flash end", |e| matches!(e, HostEvent::Flash(s) if s.is_finished()));
        assert!(matches!(e, HostEvent::Flash(FlashStage::Failed { .. })));
        assert!(!rig.received(Side::Right).iter().any(|l| l == "bootloader"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_on_both_boards_names_the_missing_one() {
        let bus = FakeBus::new();
        bus.world().boards[1].present = false;
        let rig = Rig::new(bus);
        rig.wait("left read", |e| {
            matches!(
                e,
                HostEvent::Board {
                    side: Side::Left,
                    event: ConnectionEvent::Line { reply: Reply::Peer(PeerReply::Attached(_)), .. }
                }
            )
        });
        rig.host.send(Effect::Grip(false));
        let e = rig.wait("grip result", done);
        assert!(matches!(&e, HostEvent::Done(t) if t.contains("right board isn't connected")), "{e:?}");
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
