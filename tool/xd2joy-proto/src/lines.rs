//! Splitting the serial byte stream into text lines.

/// Lines longer than this are cut: the firmware's longest line is the config blob (a few hundred
/// hex characters), so anything near this is noise, not a reply.
const MAX_LINE: usize = 4096;

/// Collects bytes and hands back complete lines without their `\r`/`\n`. Bytes that are not UTF-8
/// are replaced rather than dropped, so a garbled line still shows up in the log.
#[derive(Debug, Default)]
pub struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns the lines they completed, in order. Empty lines are skipped.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                if !self.buf.is_empty() {
                    let line = String::from_utf8_lossy(&self.buf).into_owned();
                    out.extend(split_glued(&line).into_iter().map(str::to_owned));
                    self.buf.clear();
                }
            } else if self.buf.len() < MAX_LINE {
                self.buf.push(b);
            }
        }
        out
    }
}

/// Every tag the firmware prints. Only these start a new record inside a line, so a device name
/// with brackets in it is not cut up.
const TAGS: &[&str] = &[
    "ADDR", "ADV", "BATT", "BOOT", "BTN", "CFG", "CLOCK", "CMD", "CTLRQ", "FAULT", "GATT", "GRIP", "HID", "HIDLNK",
    "HIDMAP", "HOGP", "INPUT", "JC", "KBD", "LED", "LINK", "MAP", "MODE", "MOUSE", "MREC", "MSTAT", "PEER", "PROF",
    "QGATE", "QUIET", "RATE", "RAW", "RPT", "SCAN", "SMOOTH", "STICK", "STORE", "SYNC", "USBKBD", "VER", "WAKE",
];

/// `printk` is not line-atomic, so two records can share a line (`[CFG] blob 55..[RAW] h=..`).
/// Splits before every known `[TAG]` after the start. A record cut in the middle stays broken;
/// the parser rejects it (a blob fails its length check) and the caller asks again.
pub fn split_glued(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, _) in line.match_indices('[') {
        if i > start && starts_with_tag(&line[i..]) {
            out.push(line[start..i].trim_end());
            start = i;
        }
    }
    out.push(&line[start..]);
    out.retain(|s| !s.is_empty());
    out
}

fn starts_with_tag(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('[') else { return false };
    let Some((tag, _)) = rest.split_once(']') else { return false };
    TAGS.contains(&tag.trim_end_matches(' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_across_chunks_and_line_endings() {
        let mut s = LineSplitter::new();
        assert!(s.push(b"[VER] fw 1").is_empty());
        assert_eq!(s.push(b".0\r\n[MODE] STICK\n\npartial"), vec!["[VER] fw 1.0", "[MODE] STICK"]);
        assert_eq!(s.push(b"\r"), vec!["partial"]);
    }

    #[test]
    fn caps_runaway_lines() {
        let mut s = LineSplitter::new();
        s.push(&vec![b'x'; MAX_LINE * 2]);
        let lines = s.push(b"\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), MAX_LINE);
    }

    #[test]
    fn splits_glued_records() {
        let mut s = LineSplitter::new();
        assert_eq!(
            s.push(b"[CFG] blob 554A[RAW] h=0x0012 len=1: 00[MAP   ] end\n"),
            vec!["[CFG] blob 554A", "[RAW] h=0x0012 len=1: 00", "[MAP   ] end"]
        );
        // brackets that are not a tag stay put
        assert_eq!(split_glued("[SCAN] dev x name='[TV] [x'"), vec!["[SCAN] dev x name='[TV] [x'"]);
        assert_eq!(split_glued("text [MODE] STICK"), vec!["text", "[MODE] STICK"]);
    }

    #[test]
    fn keeps_invalid_utf8_visible() {
        let mut s = LineSplitter::new();
        assert_eq!(s.push(b"a\xFFb\n"), vec!["a\u{FFFD}b"]);
    }
}
