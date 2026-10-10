//! What the window shows, as plain data, and how host events change it. No Freya types here, so
//! every rule is a unit test (`tests` below).

use std::{collections::VecDeque, path::PathBuf};

use xd2joy_device::ConnectionEvent;
use xd2joy_proto::{FirmwareInfo, InputSource, InputStatus, Mode, PROTO_VERSION, Reply, Side};

/// Lines kept in the console pane.
pub const LOG_LINES: usize = 300;

/// Something the window asks the host to do. Components only produce these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    SetMode(Side, Mode),
    SetInput(Side, InputSource),
    Flash { side: Side, file: PathBuf },
}

/// Everything the host tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Connected {
        side: Side,
        port: String,
    },
    ConnectFailed {
        side: Side,
        port: String,
        error: String,
    },
    Disconnected {
        side: Side,
    },
    Board {
        side: Side,
        event: ConnectionEvent,
    },
    /// The board answered nothing to `ver`: firmware from before this tool.
    NoVersion {
        side: Side,
    },
    /// More than one board claims the same side; the tool uses the first and says so.
    Duplicate {
        side: Side,
        ignored: String,
    },
    Flash(FlashStage),
    /// A command could not be written.
    SendFailed {
        side: Side,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashStage {
    Checking {
        side: Side,
        file: PathBuf,
    },
    /// The board was told to restart into its bootloader.
    Restarting {
        side: Side,
    },
    /// The board's firmware cannot restart itself; the person has to double-tap reset.
    NeedsManualReset {
        side: Side,
    },
    Copying {
        side: Side,
        drive: PathBuf,
    },
    WaitingForBoard {
        side: Side,
    },
    Done {
        side: Side,
    },
    Failed {
        side: Side,
        error: String,
    },
}

impl FlashStage {
    pub fn side(&self) -> Side {
        match self {
            FlashStage::Checking { side, .. }
            | FlashStage::Restarting { side }
            | FlashStage::NeedsManualReset { side }
            | FlashStage::Copying { side, .. }
            | FlashStage::WaitingForBoard { side }
            | FlashStage::Done { side }
            | FlashStage::Failed { side, .. } => *side,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, FlashStage::Done { .. } | FlashStage::Failed { .. })
    }

    /// One sentence for the progress line.
    pub fn describe(&self) -> String {
        match self {
            FlashStage::Checking { file, .. } => format!("Checking {}", file.display()),
            FlashStage::Restarting { .. } => "Restarting the board into its bootloader".to_owned(),
            FlashStage::NeedsManualReset { .. } => {
                "This firmware can't restart itself. Short RST to GND twice quickly on this board.".to_owned()
            }
            FlashStage::Copying { drive, .. } => format!("Copying the firmware to {}", drive.display()),
            FlashStage::WaitingForBoard { .. } => "Waiting for the board to come back".to_owned(),
            FlashStage::Done { .. } => "Updated".to_owned(),
            FlashStage::Failed { error, .. } => format!("Update failed: {error}"),
        }
    }
}

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

    /// A board newer than the tool: commands may have changed shape.
    pub fn newer_than_tool(&self) -> bool {
        matches!(self, Firmware::Known(info) if info.proto > PROTO_VERSION)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardView {
    pub link: Link,
    pub firmware: Firmware,
    pub mode: Option<Mode>,
    pub input: Option<InputStatus>,
    /// A problem worth showing on the card (a duplicate board, a failed write, a side mismatch).
    pub warning: Option<String>,
}

impl BoardView {
    pub fn connected(&self) -> bool {
        matches!(self.link, Link::Connected { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub boards: [BoardView; 2],
    pub flash: Option<FlashStage>,
    pub log: VecDeque<String>,
}

impl Model {
    pub fn board(&self, side: Side) -> &BoardView {
        &self.boards[side.index()]
    }

    fn board_mut(&mut self, side: Side) -> &mut BoardView {
        &mut self.boards[side.index()]
    }

    /// A flash is running: other actions on boards wait.
    pub fn busy(&self) -> bool {
        self.flash.as_ref().is_some_and(|s| !s.is_finished())
    }

    fn log(&mut self, side: Option<Side>, text: impl AsRef<str>) {
        let prefix = side.map_or_else(String::new, |s| format!("{} ", s.letter()));
        self.log.push_back(format!("{prefix}{}", text.as_ref()));
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn apply(&mut self, event: HostEvent) {
        match event {
            HostEvent::Connected { side, port } => {
                self.log(Some(side), format!("connected on {port}"));
                *self.board_mut(side) = BoardView { link: Link::Connected { port }, ..BoardView::default() };
            }
            HostEvent::ConnectFailed { side, port, error } => {
                self.log(Some(side), format!("could not open {port}: {error}"));
                self.board_mut(side).link = Link::Failed { port, error };
            }
            HostEvent::Disconnected { side } => {
                self.log(Some(side), "disconnected");
                *self.board_mut(side) = BoardView::default();
            }
            HostEvent::Board { side, event } => self.board_event(side, event),
            HostEvent::NoVersion { side } => {
                let board = self.board_mut(side);
                if board.firmware == Firmware::Unknown {
                    board.firmware = Firmware::Old;
                }
            }
            HostEvent::Duplicate { side, ignored } => {
                self.board_mut(side).warning =
                    Some(format!("Another {} board is plugged in on {ignored}; it is ignored.", side_word(side)));
            }
            HostEvent::Flash(stage) => {
                self.log(Some(stage.side()), stage.describe());
                self.flash = Some(stage);
            }
            HostEvent::SendFailed { side, error } => {
                self.log(Some(side), &error);
                self.board_mut(side).warning = Some(error);
            }
        }
    }

    fn board_event(&mut self, side: Side, event: ConnectionEvent) {
        match event {
            ConnectionEvent::Line { raw, reply } => {
                self.log(Some(side), &raw);
                let board = self.board_mut(side);
                match reply {
                    Reply::Version(info) => {
                        board.warning = (info.side != side).then(|| {
                            format!(
                                "This board's USB id says {}, but its firmware says {}. Flash the right file.",
                                side_word(side),
                                side_word(info.side)
                            )
                        });
                        board.firmware = Firmware::Known(info);
                    }
                    Reply::Mode { mode, .. } => board.mode = Some(mode),
                    Reply::Input(status) => board.input = Some(status),
                    Reply::Bootloader | Reply::Other(_) => {}
                }
            }
            ConnectionEvent::Unreadable { raw, error } => self.log(Some(side), format!("{raw}  ({error})")),
            ConnectionEvent::Lost(why) => self.log(Some(side), format!("connection lost: {why}")),
        }
    }
}

fn side_word(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(raw: &str) -> HostEvent {
        let reply = xd2joy_proto::parse_line(raw).expect("test lines parse");
        HostEvent::Board { side: Side::Left, event: ConnectionEvent::Line { raw: raw.into(), reply } }
    }

    fn connected() -> Model {
        let mut m = Model::default();
        m.apply(HostEvent::Connected { side: Side::Left, port: "/dev/ttyACM0".into() });
        m
    }

    #[test]
    fn replies_fill_the_board_card() {
        let mut m = connected();
        m.apply(line("[VER] fw v2 proto 1 side L"));
        m.apply(line("[MODE] now STICK, joy-con input on"));
        m.apply(line("[INPUT] src=auto wired=none bridge=0x00 frames=0 bad=0"));
        let b = m.board(Side::Left);
        assert!(b.connected());
        assert!(b.firmware.can_restart_itself());
        assert_eq!(b.mode, Some(Mode::Stick));
        assert_eq!(b.input.map(|i| i.source), Some(InputSource::Auto));
        assert_eq!(b.warning, None);
        assert_eq!(m.log.len(), 4);
    }

    #[test]
    fn firmware_without_ver_is_marked_old_and_cannot_restart_itself() {
        let mut m = connected();
        m.apply(HostEvent::NoVersion { side: Side::Left });
        assert_eq!(m.board(Side::Left).firmware, Firmware::Old);
        assert!(!m.board(Side::Left).firmware.can_restart_itself());
        // a late answer still wins
        m.apply(line("[VER] fw v2 proto 1 side L"));
        assert!(m.board(Side::Left).firmware.can_restart_itself());
        // and a timeout after a real answer changes nothing
        m.apply(HostEvent::NoVersion { side: Side::Left });
        assert!(m.board(Side::Left).firmware.can_restart_itself());
    }

    #[test]
    fn side_mismatch_is_flagged() {
        let mut m = connected();
        m.apply(line("[VER] fw v2 proto 1 side R"));
        assert!(m.board(Side::Left).warning.as_deref().is_some_and(|w| w.contains("firmware says right")));
    }

    #[test]
    fn newer_protocol_is_noticed() {
        let mut m = connected();
        m.apply(line(&format!("[VER] fw v9 proto {} side L", PROTO_VERSION + 1)));
        assert!(m.board(Side::Left).firmware.newer_than_tool());
    }

    #[test]
    fn disconnect_forgets_the_board() {
        let mut m = connected();
        m.apply(line("[MODE] KEYBOARD"));
        m.apply(HostEvent::Disconnected { side: Side::Left });
        assert_eq!(*m.board(Side::Left), BoardView::default());
    }

    #[test]
    fn flash_progress_makes_the_model_busy_until_it_finishes() {
        let mut m = Model::default();
        assert!(!m.busy());
        m.apply(HostEvent::Flash(FlashStage::Restarting { side: Side::Right }));
        assert!(m.busy());
        m.apply(HostEvent::Flash(FlashStage::Done { side: Side::Right }));
        assert!(!m.busy());
    }

    #[test]
    fn the_log_is_capped() {
        let mut m = connected();
        for _ in 0..LOG_LINES + 50 {
            m.apply(line("[MODE] KEYBOARD"));
        }
        assert_eq!(m.log.len(), LOG_LINES);
    }
}
