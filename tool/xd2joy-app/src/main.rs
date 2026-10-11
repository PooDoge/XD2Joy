//! `xd2joy`: the desktop configuration and firmware update tool. Starts logging, the host thread
//! (USB, serial, flashing), and the window.
//!
//! Files: `model/` (what the window shows, pure), `host.rs` (the thread that talks to boards),
//! `ui/` (the window), `theme.rs` (colours and spacing), `fake.rs` (simulated boards) and `demo.rs`
//! (the window on simulated boards, `XD2JOY_DEMO=1`; `XD2JOY_PAGE=<page>` opens on a page).

mod demo;
mod fake;
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
    let started =
        if std::env::var_os("XD2JOY_DEMO").is_some() { demo::start(sink) } else { host::start(host::SerialBus, sink) };
    let host = match started {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "could not start the host thread");
            std::process::exit(1);
        }
    };

    let start_page = std::env::var("XD2JOY_PAGE")
        .ok()
        .and_then(|want| model::Page::ALL.into_iter().find(|p| p.label().eq_ignore_ascii_case(&want)))
        .unwrap_or_default();
    let root = ui::Root { host, inbox: ui::Handoff::new(rx), start_page };
    launch(LaunchConfig::new().with_window(
        WindowConfig::new_app(root).with_title("XD2Joy").with_size(1440., 940.).with_min_size(960., 640.),
    ));
}
