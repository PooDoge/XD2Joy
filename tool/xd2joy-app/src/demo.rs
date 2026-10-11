//! A pretend pair of boards for working on the window without hardware:
//! `XD2JOY_DEMO=1 cargo run -p xd2joy`. The real host drives simulated firmware (`fake.rs`), and a
//! small thread "types" on the Bluetooth keyboard and clicks the mouse so the live view moves.

use std::{thread, time::Duration};

use xd2joy_proto::Side;

use crate::{
    fake::FakeBus,
    host::{self, HostHandle, Sink},
};

pub fn start(sink: Sink) -> std::io::Result<HostHandle> {
    let bus = FakeBus::new();
    let typist = bus.clone();
    thread::Builder::new().name("xd2joy-demo".into()).spawn(move || type_forever(&typist))?;
    host::start(bus, sink)
}

fn type_forever(bus: &FakeBus) {
    // W, then W+D, then nothing; left click, then the back button, then nothing
    let keys = ["00 00 1A 00 00 00 00 00", "00 00 1A 07 00 00 00 00", "00 00 00 00 00 00 00 00"];
    let mouse = ["01 00 00 00", "08 00 00 00", "00 00 00 00"];
    let mut i = 0usize;
    loop {
        thread::sleep(Duration::from_millis(900));
        bus.say(Side::Left, &[format!("[RAW] h=0x0012 len=8: {}", keys[i % keys.len()])]);
        bus.say(Side::Right, &[format!("[RAW] h=0x0020 len=4: {}", mouse[i % mouse.len()])]);
        i += 1;
    }
}
