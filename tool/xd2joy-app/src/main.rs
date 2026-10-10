//! `xd2joy`: the desktop configuration and firmware update tool. Starts logging, the host thread
//! (USB, serial, flashing), and the window.
//!
//! Files: `model.rs` (what the window shows, pure), `host.rs` (the thread that talks to boards),
//! `ui.rs` (the window), `theme.rs` (colours and spacing), `demo.rs` (a pretend host, `XD2JOY_DEMO=1`).

mod demo;
mod host;
mod model;
mod theme;
mod ui;

use std::sync::Arc;

use freya::prelude::*;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("XD2JOY_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .init();

    let (tx, rx) = futures_channel::mpsc::unbounded();
    let sink: host::Sink = Arc::new(move |event| {
        // the window closed; the host stops when its handle is dropped
        let _ = tx.unbounded_send(event);
    });
    let started = if std::env::var_os("XD2JOY_DEMO").is_some() { demo::start(sink) } else { host::start(sink) };
    let host = match started {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "could not start the host thread");
            std::process::exit(1);
        }
    };

    let root = ui::Root { host, inbox: ui::Handoff::new(rx) };
    launch(LaunchConfig::new().with_window(WindowConfig::new_app(root).with_title("XD2Joy").with_size(900., 680.)));
}
