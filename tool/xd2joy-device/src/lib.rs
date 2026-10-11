//! Finding XD2Joy boards on USB serial and talking to them.
//!
//! Files: `lib.rs` (discovery and the plug/unplug diff), `connection.rs` (one open board: a reader
//! thread that parses lines, and a writer), `exchange.rs` (one command in flight and its answer).

mod connection;
mod exchange;

pub use connection::{Connection, ConnectionEvent};
pub use exchange::{Exchange, Outcome, SETTLE};
use xd2joy_proto::Side;

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("could not list serial ports: {0}")]
    List(#[source] serialport::Error),
    #[error("could not open {port}: {source}")]
    Open {
        port: String,
        #[source]
        source: serialport::Error,
    },
    #[error("could not start reading {port}: {source}")]
    Clone {
        port: String,
        #[source]
        source: serialport::Error,
    },
    #[error("could not write to {port}: {source}")]
    Write {
        port: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start the reader thread for {port}: {source}")]
    Thread {
        port: String,
        #[source]
        source: std::io::Error,
    },
}

/// A board found on USB: its serial port and which side it says it is (from the USB product id).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoardPort {
    pub port: String,
    pub side: Side,
    pub serial_number: Option<String>,
}

/// The XD2Joy boards plugged in now, sorted by side then port. On macOS each CDC device shows up as
/// both `/dev/cu.*` and `/dev/tty.*`; only `cu.*` is kept, because opening `tty.*` waits for carrier.
pub fn list_boards() -> Result<Vec<BoardPort>, DeviceError> {
    let ports = serialport::available_ports().map_err(DeviceError::List)?;
    let mut boards: Vec<BoardPort> = ports
        .into_iter()
        .filter(|p| !p.port_name.starts_with("/dev/tty."))
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(usb) => Side::from_usb(usb.vid, usb.pid).map(|side| BoardPort {
                port: p.port_name,
                side,
                serial_number: usb.serial_number,
            }),
            _ => None,
        })
        .collect();
    boards.sort_by(|a, b| (a.side, &a.port).cmp(&(b.side, &b.port)));
    Ok(boards)
}

/// What changed between two scans.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PortChanges {
    pub added: Vec<BoardPort>,
    pub removed: Vec<BoardPort>,
}

pub fn diff_ports(before: &[BoardPort], now: &[BoardPort]) -> PortChanges {
    PortChanges {
        added: now.iter().filter(|p| !before.contains(p)).cloned().collect(),
        removed: before.iter().filter(|p| !now.contains(p)).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, side: Side) -> BoardPort {
        BoardPort { port: name.into(), side, serial_number: None }
    }

    #[test]
    fn diff_reports_plugs_and_unplugs() {
        let a = port("/dev/ttyACM0", Side::Left);
        let b = port("/dev/ttyACM1", Side::Right);
        let c = port("/dev/ttyACM2", Side::Right);
        let changes = diff_ports(&[a.clone(), b.clone()], &[a, c.clone()]);
        assert_eq!(changes, PortChanges { added: vec![c], removed: vec![b] });
        assert_eq!(diff_ports(&[], &[]), PortChanges::default());
    }
}
