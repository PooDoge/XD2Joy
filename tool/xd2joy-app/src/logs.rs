//! Logging: to the terminal, and to a daily file kept for troubleshooting.
//!
//! The file gets everything the app's crates log at debug level and above: every line sent to and
//! received from a board, every job and its outcome, every notice the window showed, every edit to
//! the mapping and every panic. `[RAW]` reports are only logged at trace level, because a mouse
//! prints one per movement. `XD2JOY_LOG` overrides the file's filter (for example `trace`) and
//! `XD2JOY_LOG_DIR` its folder.
//!
//! The file is written synchronously, not from a background thread, so the lines just before a
//! crash are not lost.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// Days of log files kept.
pub const KEEP_DAYS: usize = 14;
const PREFIX: &str = "xd2joy";
const SUFFIX: &str = "log";
/// What the file records unless `XD2JOY_LOG` says otherwise. Other crates only at warn: Freya's
/// renderer logs its cache sizes at info several times a second.
const FILE_FILTER: &str = "warn,xd2joy=debug,xd2joy_device=debug,xd2joy_flash=debug,xd2joy_proto=debug";
const TERMINAL_FILTER: &str = "warn,xd2joy=info,xd2joy_device=info,xd2joy_flash=info,xd2joy_proto=info";

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// The folder log files go to, once logging started with one.
pub fn dir() -> Option<&'static Path> {
    DIR.get().map(PathBuf::as_path)
}

fn default_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XD2JOY_LOG_DIR") {
        return Some(PathBuf::from(d));
    }
    // ~/.local/share/xd2joy/logs, ~/Library/Application Support/xd2joy/logs, %LOCALAPPDATA%\xd2joy\logs
    dirs::data_local_dir().map(|d| d.join("xd2joy").join("logs"))
}

/// Start logging. Without a usable log folder the app still runs, logging to the terminal only,
/// and says why there.
pub fn init() {
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(EnvFilter::try_from_env("XD2JOY_LOG").unwrap_or_else(|_| EnvFilter::new(TERMINAL_FILTER)));

    let mut problem = None;
    let file = match default_dir() {
        None => {
            problem = Some("no folder for application data on this system".to_owned());
            None
        }
        Some(dir) => match open(&dir) {
            Ok(appender) => {
                let _ = DIR.set(dir);
                let filter = EnvFilter::try_from_env("XD2JOY_LOG").unwrap_or_else(|_| EnvFilter::new(FILE_FILTER));
                Some(tracing_subscriber::fmt::layer().with_writer(appender).with_ansi(false).with_filter(filter))
            }
            Err(e) => {
                problem = Some(format!("{}: {e}", dir.display()));
                None
            }
        },
    };
    tracing_subscriber::registry().with(stderr).with(file).init();
    if let Some(p) = problem {
        tracing::warn!(problem = %p, "no log file; logging to the terminal only");
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(%info, %backtrace, "panic");
        default_hook(info);
    }));

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        log_dir = ?dir(),
        demo = std::env::var_os("XD2JOY_DEMO").is_some(),
        "xd2joy starting"
    );
}

fn open(dir: &Path) -> Result<RollingFileAppender, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(PREFIX)
        .filename_suffix(SUFFIX)
        .max_log_files(KEEP_DAYS)
        .build(dir)
        .map_err(|e| e.to_string())
}

/// The newest log file in `dir`. Names carry the date (`xd2joy.2026-10-11.log`), so the greatest
/// name is the newest.
pub fn newest_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
        })
        .max()
}

/// Bytes read from the end of the log file for `tail`; a busy day's file can be many megabytes.
const TAIL_BYTES: u64 = 256 * 1024;

/// The last `lines` lines of the newest log file, for the diagnostics file.
pub fn tail(lines: usize) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let dir = dir().ok_or("there is no log file")?;
    let file = newest_file(dir).ok_or("the log folder is empty")?;
    let fail = |e: std::io::Error| format!("{}: {e}", file.display());
    let mut f = std::fs::File::open(&file).map_err(fail)?;
    let len = f.metadata().map_err(fail)?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).map_err(fail)?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(fail)?;
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    // the first line may have been cut by the seek
    let first = usize::from(len > TAIL_BYTES);
    let from = all.len().saturating_sub(lines).max(first);
    Ok(all.get(from..).unwrap_or_default().join("\n"))
}

/// A pairing code is only good once, but there is no reason to keep it in a file someone may
/// attach to a bug report: digits after the word "passkey" become `*` (the timestamp before it stays).
pub fn mask_passkeys(text: &str) -> String {
    text.lines()
        .map(|line| match line.to_ascii_lowercase().find("passkey") {
            Some(at) => {
                let (head, rest) = line.split_at(at);
                let masked: String = rest.chars().map(|c| if c.is_ascii_digit() { '*' } else { c }).collect();
                format!("{head}{masked}")
            }
            None => line.to_owned(),
        })
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passkeys_are_masked() {
        assert_eq!(mask_passkeys("12:00 L: PASSKEY 123456\nL: h=0x0012"), "12:00 L: PASSKEY ******\nL: h=0x0012");
    }

    #[test]
    fn the_newest_dated_file_wins_and_others_are_ignored() -> Result<(), std::io::Error> {
        let dir = std::env::temp_dir().join(format!("xd2joy-logs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        for name in ["xd2joy.2026-10-09.log", "xd2joy.2026-10-11.log", "xd2joy.2026-10-10.log", "zzz.txt"] {
            std::fs::write(dir.join(name), "x")?;
        }
        let newest = newest_file(&dir).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        assert_eq!(newest.as_deref(), Some("xd2joy.2026-10-11.log"));
        std::fs::remove_dir_all(&dir)
    }
}
