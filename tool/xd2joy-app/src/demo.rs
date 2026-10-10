//! A pretend host for working on the window without boards: `XD2JOY_DEMO=1 cargo run -p xd2joy`.
//! It plays both boards connecting, answers mode and input changes, and walks through a firmware
//! update without touching any drive.

use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use xd2joy_device::ConnectionEvent;
use xd2joy_proto::{InputSource, Mode, Side, parse_line};

use crate::{
    host::{HostHandle, Msg, Sink},
    model::{Effect, FlashStage, HostEvent},
};

pub fn start(sink: Sink) -> std::io::Result<HostHandle> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new().name("xd2joy-demo".into()).spawn(move || run(&sink, &rx))?;
    Ok(HostHandle::from_sender(tx))
}

fn say(sink: &Sink, side: Side, raw: &str) {
    match parse_line(raw) {
        Ok(reply) => sink(HostEvent::Board { side, event: ConnectionEvent::Line { raw: raw.to_owned(), reply } }),
        Err(e) => tracing::error!(raw, error = %e, "the demo script has a line the parser refuses"),
    }
}

fn mode_word(mode: Mode) -> &'static str {
    match mode {
        Mode::Joycon => "JOYCON",
        Mode::Stick => "STICK",
        Mode::Keyboard => "KEYBOARD",
    }
}

fn source_word(source: InputSource) -> &'static str {
    match source {
        InputSource::Auto => "auto",
        InputSource::Bluetooth => "bt",
        InputSource::Usb => "usb",
    }
}

fn run(sink: &Sink, rx: &Receiver<Msg>) {
    sink(HostEvent::Connected { side: Side::Left, port: "/dev/ttyACM0".into() });
    say(sink, Side::Left, "[VER] fw v0.3.0-demo proto 1 side L");
    say(sink, Side::Left, "[MODE] JOYCON");
    say(sink, Side::Left, "[INPUT] src=auto wired=active bridge=0x01 frames=5120 bad=0");
    sink(HostEvent::Connected { side: Side::Right, port: "/dev/ttyACM1".into() });
    sink(HostEvent::NoVersion { side: Side::Right });
    say(sink, Side::Right, "[MODE] JOYCON");

    while let Ok(msg) = rx.recv() {
        let Msg::Effect(effect) = msg else { continue };
        match effect {
            Effect::SetMode(side, mode) => {
                say(sink, side, &format!("[MODE] now {}, joy-con input on", mode_word(mode)))
            }
            Effect::SetInput(side, source) => {
                say(sink, side, &format!("[INPUT] src={} wired=none bridge=0x00 frames=0 bad=0", source_word(source)))
            }
            Effect::Flash { side, file } => {
                for stage in [
                    FlashStage::Checking { side, file },
                    FlashStage::Restarting { side },
                    FlashStage::Copying { side, drive: "/media/demo/NICENANO".into() },
                    FlashStage::WaitingForBoard { side },
                    FlashStage::Done { side },
                ] {
                    sink(HostEvent::Flash(stage));
                    thread::sleep(Duration::from_millis(700));
                }
            }
        }
    }
}
