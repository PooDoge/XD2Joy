//! `xd2joy`: the desktop configuration and firmware update tool. Starts logging, the host thread
//! (USB, serial, flashing), and the window.
//!
//! Files: `model/` (what the window shows, pure), `host.rs` (the thread that talks to boards),
//! `ui/` (the window), `logs.rs` (the log file), `theme.rs` (colours and spacing), `fake.rs` (simulated boards) and `demo.rs`
//! (the window on simulated boards, `XD2JOY_DEMO=1`; `XD2JOY_PAGE=<page>` opens on a page).

mod cache;
mod close;
mod demo;
mod fake;
mod host;
mod logs;
mod model;
mod theme;
mod ui;

use std::sync::Arc;

use freya::prelude::*;

fn main() {
    logs::init();

    let (tx, rx) = futures_channel::mpsc::unbounded();
    let close_tx = tx.clone();
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
    // the demo's simulated boards must not mix with the real boards' remembered configs
    let cached = match cache::path() {
        Some(file) if std::env::var_os("XD2JOY_DEMO").is_none() => cache::load(&file),
        _ => [None, None],
    };
    let close = close::CloseGuard::default();
    let guard = close.clone();
    let root = ui::Root { host, inbox: ui::Handoff::new(rx), start_page, cached: ui::Handoff::new(cached), close };
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new_app(root)
                .with_title("XD2Joy")
                .with_size(1440., 940.)
                .with_min_size(960., 640.)
                .with_on_close(move |_, _| match guard.decide(std::time::Instant::now()) {
                    Ok(()) => CloseDecision::Close,
                    Err(reason) => {
                        // the window shows why through the same channel as host events
                        let _ = close_tx.unbounded_send(model::HostEvent::CloseRefused(reason));
                        CloseDecision::KeepOpen
                    }
                }),
        ),
    );
    tracing::info!("window closed");
}
