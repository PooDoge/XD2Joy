//! One command in flight on one board, and the lines that answer it.
//!
//! The firmware ignores every byte that arrives while it is still running a command, so a second
//! command written too soon is lost without a trace. An `Exchange` is how the caller knows when
//! the board is ready again: it finishes on the command's last reply line, after which a short
//! `SETTLE` lets the console handler return, or it expires.

use std::time::{Duration, Instant};

use xd2joy_proto::{Command, Reply};

/// After the last reply line the firmware still has to return from its handler before it reads
/// again. A few milliseconds is plenty; this leaves margin for a busy USB stack.
pub const SETTLE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The board answered; these are the lines that belong to the answer, in order.
    Answered(Vec<Reply>),
    /// A command that prints nothing ran its guard time; assume it was taken.
    Silent,
    /// No (complete) answer in time. The lines that did arrive are kept.
    TimedOut(Vec<Reply>),
}

#[derive(Debug)]
pub struct Exchange {
    command: Command,
    deadline: Instant,
    replies: Vec<Reply>,
    done_at: Option<Instant>,
}

impl Exchange {
    /// Starts the clock; call right after writing the command.
    pub fn new(command: Command, now: Instant) -> Exchange {
        let deadline = now + command.timeout();
        Exchange { command, deadline, replies: Vec::new(), done_at: None }
    }

    pub fn command(&self) -> &Command {
        &self.command
    }

    /// Offer a line the board printed while this command was in flight.
    pub fn offer(&mut self, reply: &Reply, now: Instant) {
        if self.done_at.is_some() {
            return;
        }
        if self.command.concerns(reply) {
            self.replies.push(reply.clone());
        }
        if self.command.is_done_by(reply) {
            self.done_at = Some(now);
        }
    }

    /// `Some` once the exchange is over and the board can take the next command.
    pub fn poll(&mut self, now: Instant) -> Option<Outcome> {
        if let Some(at) = self.done_at {
            return (now >= at + SETTLE).then(|| Outcome::Answered(std::mem::take(&mut self.replies)));
        }
        if now < self.deadline {
            return None;
        }
        Some(if self.command.is_silent() {
            Outcome::Silent
        } else {
            Outcome::TimedOut(std::mem::take(&mut self.replies))
        })
    }

    /// When `poll` should be called next for a timely answer.
    pub fn wake_at(&self) -> Instant {
        self.done_at.map_or(self.deadline, |at| at + SETTLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xd2joy_proto::{ProfileReply, ProfileSlot, parse_line};

    fn line(s: &str) -> Reply {
        match parse_line(s) {
            Ok(r) => r,
            Err(e) => panic!("{s}: {e}"),
        }
    }

    #[test]
    fn finishes_on_the_last_line_then_settles() {
        let t0 = Instant::now();
        let mut ex = Exchange::new(Command::Profiles, t0);
        ex.offer(&line("[PROF] active 1 of 4"), t0);
        ex.offer(&line("[RAW] h=0x0012 len=1: 00"), t0);
        for n in 1..=3 {
            ex.offer(&line(&format!("[PROF] {n} empty  key 0x00 mods 0x00 ")), t0);
        }
        assert_eq!(ex.poll(t0), None);
        ex.offer(&line("[PROF] 4 saved  key 0x00 mods 0x00 x"), t0);
        assert_eq!(ex.poll(t0), None, "waits for the handler to return");
        let Some(Outcome::Answered(r)) = ex.poll(t0 + SETTLE) else { panic!("should be answered") };
        // chatter is left out
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], Reply::Profile(ProfileReply::Active(1)));
        assert!(matches!(&r[4], Reply::Profile(ProfileReply::Slot(ProfileSlot { slot: 4, saved: true, .. }))));
    }

    #[test]
    fn times_out_with_what_arrived() {
        let t0 = Instant::now();
        let mut ex = Exchange::new(Command::PeerQuery, t0);
        ex.offer(&line("[PEER] pin=none"), t0);
        assert_eq!(ex.poll(t0 + Duration::from_secs(1)), None);
        assert_eq!(ex.poll(t0 + Duration::from_secs(3)), Some(Outcome::TimedOut(vec![line("[PEER] pin=none")])));
    }

    #[test]
    fn silent_commands_just_wait() {
        let t0 = Instant::now();
        let mut ex = Exchange::new(Command::PeerClear, t0);
        assert_eq!(ex.poll(t0), None);
        assert_eq!(ex.poll(ex.wake_at()), Some(Outcome::Silent));
    }
}
