//! The host: one thread that owns the serial connections and does everything the window asks
//! (`Effect`s), reporting back through `HostEvent`s. It rescans USB every half second, connects
//! boards as they appear, and runs firmware updates.
//!
//! A firmware update blocks the thread on purpose: nothing else should talk to the boards while
//! one is restarting, and the window disables its buttons until the update finishes.

use std::{
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

use xd2joy_device::{BoardPort, Connection, ConnectionEvent, list_boards};
use xd2joy_flash::{FAMILY_NRF52840, Uf2Image, copy_image, list_boot_drives, new_drive};
use xd2joy_proto::{Command, FirmwareInfo, Reply, Side};

use crate::model::{Effect, FlashStage, HostEvent};

const SCAN_EVERY: Duration = Duration::from_millis(500);
/// How long a board gets to answer `ver` before it counts as old firmware.
const VERSION_WAIT: Duration = Duration::from_secs(2);
/// A port that would not open is retried after this long, not on every scan.
const RETRY_OPEN: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(250);
const DRIVE_WAIT_AUTO: Duration = Duration::from_secs(15);
/// Long enough for someone to find a wire and short RST twice.
const DRIVE_WAIT_MANUAL: Duration = Duration::from_secs(90);
const BOARD_WAIT: Duration = Duration::from_secs(20);

pub type Sink = Arc<dyn Fn(HostEvent) + Send + Sync>;

pub(crate) enum Msg {
    Effect(Effect),
    /// From a connection's reader thread; `port` drops events from a connection already closed.
    Board {
        side: Side,
        port: String,
        event: ConnectionEvent,
    },
}

/// The window's handle to the host. Dropping every handle stops the host thread.
#[derive(Clone)]
pub struct HostHandle {
    tx: Sender<Msg>,
}

impl HostHandle {
    /// A handle whose requests go to `tx` (for the demo host).
    pub(crate) fn from_sender(tx: Sender<Msg>) -> HostHandle {
        HostHandle { tx }
    }

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

pub fn start(sink: Sink) -> std::io::Result<HostHandle> {
    let (tx, rx) = mpsc::channel();
    let host = Host { tx: tx.clone(), sink, live: [None, None], failed: Vec::new(), warned_duplicate: [false, false] };
    std::thread::Builder::new().name("xd2joy-host".into()).spawn(move || host.run(rx))?;
    Ok(HostHandle { tx })
}

struct Live {
    conn: Connection,
    opened: Instant,
    firmware: Option<FirmwareInfo>,
    version_timed_out: bool,
}

struct Host {
    tx: Sender<Msg>,
    sink: Sink,
    live: [Option<Live>; 2],
    /// Ports that would not open, and when.
    failed: Vec<(String, Instant)>,
    warned_duplicate: [bool; 2],
}

impl Host {
    fn emit(&self, event: HostEvent) {
        (self.sink)(event);
    }

    fn run(mut self, rx: Receiver<Msg>) {
        let mut last_scan: Option<Instant> = None;
        loop {
            match rx.recv_timeout(POLL) {
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
            if last_scan.is_none_or(|t| t.elapsed() >= SCAN_EVERY) {
                self.scan();
                last_scan = Some(Instant::now());
            }
            self.check_versions();
        }
    }

    fn scan(&mut self) {
        let ports = match list_boards() {
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
                && !mine.iter().any(|p| p.port == live.conn.board().port)
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
        let opened = Connection::open(board, move |event| {
            // the host may have stopped; nothing to do with the line then
            let _ = tx.send(Msg::Board { side, port: reader_port.clone(), event });
        });
        match opened {
            Ok(mut conn) => {
                self.emit(HostEvent::Connected { side, port: port.clone() });
                for cmd in [Command::Version, Command::Mode(None), Command::Input(None)] {
                    if let Err(e) = conn.send(cmd) {
                        self.emit(HostEvent::SendFailed { side, error: e.to_string() });
                        break;
                    }
                }
                self.live[side.index()] =
                    Some(Live { conn, opened: Instant::now(), firmware: None, version_timed_out: false });
            }
            Err(e) => {
                tracing::warn!(%port, error = %e, "open failed");
                self.failed.push((port.clone(), Instant::now()));
                self.emit(HostEvent::ConnectFailed { side, port, error: e.to_string() });
            }
        }
    }

    fn drop_connection(&mut self, side: Side) {
        if let Some(live) = self.live[side.index()].take() {
            live.conn.close();
            self.emit(HostEvent::Disconnected { side });
        }
    }

    fn board_event(&mut self, side: Side, port: &str, event: ConnectionEvent) {
        let Some(live) = &mut self.live[side.index()] else { return };
        if live.conn.board().port != port {
            return;
        }
        if let ConnectionEvent::Line { reply: Reply::Version(info), .. } = &event {
            live.firmware = Some(info.clone());
        }
        let lost = matches!(event, ConnectionEvent::Lost(_));
        self.emit(HostEvent::Board { side, event });
        if lost {
            self.drop_connection(side);
        }
    }

    fn check_versions(&mut self) {
        let mut late = Vec::new();
        for (i, live) in self.live.iter_mut().enumerate() {
            if let Some(live) = live
                && live.firmware.is_none()
                && !live.version_timed_out
                && live.opened.elapsed() >= VERSION_WAIT
            {
                live.version_timed_out = true;
                late.push(Side::BOTH[i]);
            }
        }
        for side in late {
            self.emit(HostEvent::NoVersion { side });
        }
    }

    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::SetMode(side, mode) => self.send(side, Command::Mode(Some(mode))),
            Effect::SetInput(side, source) => self.send(side, Command::Input(Some(source))),
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

    fn send(&mut self, side: Side, command: Command) {
        let Some(live) = &mut self.live[side.index()] else {
            self.emit(HostEvent::SendFailed { side, error: "That board is not connected.".into() });
            return;
        };
        if let Err(e) = live.conn.send(command) {
            self.emit(HostEvent::SendFailed { side, error: e.to_string() });
        }
    }

    /// Check the image, get the board into its bootloader, copy, and wait for it to come back.
    /// Errors are sentences for the person.
    fn flash(&mut self, side: Side, file: &Path) -> Result<(), String> {
        let bytes = std::fs::read(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
        let image = Uf2Image::parse(bytes).map_err(|e| format!("{} is not a usable UF2 file: {e}", file.display()))?;
        image.require_family(FAMILY_NRF52840).map_err(|e| e.to_string())?;

        let before = list_boot_drives();
        let other = match side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        let other_port = self.live[other.index()].as_ref().map(|l| l.conn.board().port.clone());
        let can_restart =
            self.live[side.index()].as_ref().and_then(|l| l.firmware.as_ref()).is_some_and(|f| f.proto >= 1);

        let wait = if can_restart {
            self.send(side, Command::Bootloader);
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
            if let Some(drive) = new_drive(&before, &list_boot_drives()) {
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
            && !list_boards().is_ok_and(|ports| ports.iter().any(|p| p.port == port))
        {
            return Err(format!(
                "the {} board restarted instead, so nothing was copied. Unplug it and plug it back in, then try again.",
                match other {
                    Side::Left => "left",
                    Side::Right => "right",
                }
            ));
        }
        if !drive.info.looks_like_nrf52840() {
            tracing::warn!(mount = %drive.mount.display(), board_id = ?drive.info.board_id, "unexpected bootloader id");
        }

        self.emit(HostEvent::Flash(FlashStage::Copying { side, drive: drive.mount.clone() }));
        copy_image(&drive.mount, &image).map_err(|e| e.to_string())?;

        self.emit(HostEvent::Flash(FlashStage::WaitingForBoard { side }));
        let deadline = Instant::now() + BOARD_WAIT;
        loop {
            std::thread::sleep(POLL);
            if list_boards().is_ok_and(|ports| ports.iter().any(|p| p.side == side)) {
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
