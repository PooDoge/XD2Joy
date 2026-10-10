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
                    out.push(String::from_utf8_lossy(&self.buf).into_owned());
                    self.buf.clear();
                }
            } else if self.buf.len() < MAX_LINE {
                self.buf.push(b);
            }
        }
        out
    }
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
    fn keeps_invalid_utf8_visible() {
        let mut s = LineSplitter::new();
        assert_eq!(s.push(b"a\xFFb\n"), vec!["a\u{FFFD}b"]);
    }
}
