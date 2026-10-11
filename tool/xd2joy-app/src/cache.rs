//! The last config read from each board, kept on disk so a board's half of the mapping can be
//! looked at and edited while it is unplugged. The edits are applied when it is plugged in again;
//! if the board holds something else by then, the editor keeps the edits and says so.
//!
//! One small JSON file: `{"L": {"blob": "<hex>", "read_at": <unix seconds>, "firmware": "v2"}, "R": ...}`.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value, json};
use xd2joy_proto::{ConfigBlob, Side};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached {
    pub blob: ConfigBlob,
    /// When it was read, in seconds since 1970.
    pub read_at: u64,
    pub firmware: Option<String>,
}

/// `<config dir>/xd2joy/boards.json`, or `XD2JOY_CACHE` when set.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("XD2JOY_CACHE") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("xd2joy").join("boards.json"))
}

fn key(side: Side) -> &'static str {
    match side {
        Side::Left => "L",
        Side::Right => "R",
    }
}

/// Both boards' cached configs. A missing or damaged file is no cache, logged, never an error:
/// the boards are read again on connect anyway.
pub fn load(file: &Path) -> [Option<Cached>; 2] {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return [None, None],
        Err(e) => {
            tracing::warn!(file = %file.display(), error = %e, "board cache unreadable");
            return [None, None];
        }
    };
    let root: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(file = %file.display(), error = %e, "board cache is not JSON; ignored");
            return [None, None];
        }
    };
    Side::BOTH.map(|side| {
        let entry = root.get(key(side))?;
        let hex = entry.get("blob")?.as_str()?;
        let blob = match ConfigBlob::from_hex(hex) {
            Ok(b) if b.side == side => b,
            Ok(_) => {
                tracing::warn!(?side, "board cache holds the other side's config; ignored");
                return None;
            }
            Err(e) => {
                tracing::warn!(?side, error = %e, "board cache config unreadable; ignored");
                return None;
            }
        };
        Some(Cached {
            blob,
            read_at: entry.get("read_at").and_then(Value::as_u64).unwrap_or(0),
            firmware: entry.get("firmware").and_then(Value::as_str).map(str::to_owned),
        })
    })
}

/// Store one board's config, keeping the other board's entry.
pub fn save(file: &Path, blob: &ConfigBlob, firmware: Option<&str>) -> Result<(), String> {
    let hex = blob.to_hex().map_err(|e| e.to_string())?;
    let mut root = match std::fs::read_to_string(file) {
        Ok(t) => match serde_json::from_str::<Value>(&t).ok().and_then(|v| v.as_object().cloned()) {
            Some(map) => map,
            None => {
                // keep the damaged file for a bug report instead of silently writing over it
                let aside = file.with_extension("json.bad");
                tracing::warn!(file = %file.display(), aside = %aside.display(), "board cache damaged; moved aside");
                if let Err(e) = std::fs::rename(file, &aside) {
                    tracing::warn!(error = %e, "could not move the damaged board cache aside");
                }
                Map::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
        // writing now would drop the other board's entry, which may only be unreadable for a moment
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    root.insert(key(blob.side).to_owned(), json!({"blob": hex, "read_at": now, "firmware": firmware}));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    // write, flush to disk, then rename, so a crash or power cut leaves the old file rather than
    // half a new one; the process id keeps two running copies of the app off each other's file
    let tmp = file.with_extension(format!("json.{}.tmp", std::process::id()));
    let fail = |e: std::io::Error| format!("{}: {e}", tmp.display());
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(fail)?;
        f.write_all(text.as_bytes()).map_err(fail)?;
        f.sync_all().map_err(fail)?;
    }
    std::fs::rename(&tmp, file).map_err(|e| {
        if let Err(cleanup) = std::fs::remove_file(&tmp) {
            tracing::debug!(error = %cleanup, "could not remove the temporary board cache");
        }
        format!("{}: {e}", file.display())
    })
}

/// `2026-10-11` for a time in seconds since 1970 (UTC; a day either way does not matter here).
pub fn date(secs: u64) -> String {
    // Howard Hinnant's days-to-civil
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_one_board_without_losing_the_other() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("xd2joy-cache-{}", std::process::id()));
        let file = dir.join("boards.json");
        let left = ConfigBlob::firmware_default(Side::Left);
        let right = ConfigBlob::firmware_default(Side::Right);
        save(&file, &left, Some("v2"))?;
        save(&file, &right, None)?;
        let [l, r] = load(&file);
        assert_eq!(l.as_ref().map(|c| &c.blob), Some(&left));
        assert_eq!(l.and_then(|c| c.firmware).as_deref(), Some("v2"));
        assert_eq!(r.map(|c| c.blob), Some(right));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_damaged_or_missing_file_is_no_cache() -> Result<(), std::io::Error> {
        let dir = std::env::temp_dir().join(format!("xd2joy-cache-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        assert_eq!(load(&dir.join("missing.json")), [None, None]);
        let file = dir.join("boards.json");
        std::fs::write(&file, r#"{"L": {"blob": "zz"}, "R": 5}"#)?;
        assert_eq!(load(&file), [None, None]);
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn a_damaged_file_is_moved_aside_when_saving() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("xd2joy-cache-aside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join("boards.json");
        std::fs::write(&file, "not json").map_err(|e| e.to_string())?;
        save(&file, &ConfigBlob::firmware_default(Side::Left), None)?;
        assert!(dir.join("boards.json.bad").exists());
        assert!(load(&file)[0].is_some());
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
    }

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_791_676_800), "2026-10-11");
        assert_eq!(date(951_782_400), "2000-02-29");
    }
}
