//! One open board. Reading happens on its own thread, which parses lines and hands them to a
//! callback; the callback decides how they reach the UI, so this crate knows nothing about async.

use std::{
    io::{ErrorKind, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use serialport::SerialPort;
use xd2joy_proto::{BAUD, Command, LineSplitter, Reply, parse_line};

use crate::{BoardPort, DeviceError};

/// How long a read waits before checking whether the connection was closed.
const READ_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// A line the firmware printed, and what it means.
    Line { raw: String, reply: Reply },
    /// A tagged line whose fields did not parse: the firmware and the tool disagree.
    Unreadable { raw: String, error: String },
    /// The port closed on its own (unplugged, or the board restarted). Not sent after `close`.
    Lost(String),
}

pub struct Connection {
    board: BoardPort,
    writer: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Connection {
    /// Open the board's port and start reading. `on_event` runs on the reader thread.
    pub fn open(
        board: BoardPort,
        on_event: impl Fn(ConnectionEvent) + Send + 'static,
    ) -> Result<Connection, DeviceError> {
        let open_err = |source| DeviceError::Open { port: board.port.clone(), source };
        let mut writer = serialport::new(&board.port, BAUD).timeout(READ_POLL).open().map_err(open_err)?;
        // Zephyr's CDC ACM holds output until the host raises DTR, as browsers do on open.
        writer.write_data_terminal_ready(true).map_err(open_err)?;
        let reader = writer.try_clone().map_err(|source| DeviceError::Clone { port: board.port.clone(), source })?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            let port = board.port.clone();
            std::thread::Builder::new()
                .name(format!("xd2joy-read {}", board.port))
                .spawn(move || read_loop(reader, &stop, &port, on_event))
                .map_err(|source| DeviceError::Thread { port: board.port.clone(), source })?
        };
        Ok(Connection { board, writer, stop, reader: Some(thread) })
    }

    pub fn board(&self) -> &BoardPort {
        &self.board
    }

    pub fn send(&mut self, command: &Command) -> Result<(), DeviceError> {
        let line = command.encode();
        tracing::debug!(port = %self.board.port, line = line.trim_end(), "send");
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|source| DeviceError::Write { port: self.board.port.clone(), source })
    }

    /// Stop reading and close the port. Dropping does the same.
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.reader.take()
            && thread.join().is_err()
        {
            tracing::error!(port = %self.board.port, "the reader thread panicked");
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_loop(mut port: Box<dyn SerialPort>, stop: &AtomicBool, name: &str, on_event: impl Fn(ConnectionEvent)) {
    let mut lines = LineSplitter::new();
    let mut buf = [0u8; 512];
    while !stop.load(Ordering::Relaxed) {
        match port.read(&mut buf) {
            // a timed-out read is an error, so zero bytes means end of file: the device went away
            Ok(0) => {
                if !stop.load(Ordering::Relaxed) {
                    tracing::info!(port = name, "port reached end of file");
                    on_event(ConnectionEvent::Lost("the board went away".to_owned()));
                }
                return;
            }
            Ok(n) => {
                for raw in lines.push(&buf[..n]) {
                    on_event(match parse_line(&raw) {
                        Ok(reply) => ConnectionEvent::Line { raw, reply },
                        Err(e) => {
                            tracing::warn!(port = name, line = %raw, error = %e, "unreadable reply");
                            ConnectionEvent::Unreadable { raw, error: e.to_string() }
                        }
                    });
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::Interrupted => {}
            Err(e) => {
                if !stop.load(Ordering::Relaxed) {
                    tracing::info!(port = name, error = %e, "port closed");
                    on_event(ConnectionEvent::Lost(e.to_string()));
                }
                return;
            }
        }
    }
}
