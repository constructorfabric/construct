use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

/// The longest line that is held in memory. A longer one (a huge tool result) is skipped
/// to its newline without being kept.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// One complete line of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// The text without its newline. Bytes that are not UTF-8 are replaced.
    Text(String),
    /// A line longer than [`MAX_LINE_BYTES`]; its text was dropped.
    TooLong,
}

/// Hands every complete line of `path` from byte `offset` on to `each`, one at a time, with
/// the offset after it, and returns the offset after the last complete line. Memory stays
/// bounded by [`MAX_LINE_BYTES`] whatever the file holds. A last line without its newline is
/// still being written (or was torn by a crash): it is left for the next call. A file shorter
/// than `offset` was replaced and is read from the start. `each` returns `false` to stop
/// early; the returned offset is then the one after the last line it was given.
///
/// # Errors
/// Fails when the file cannot be opened or read.
pub fn for_each_line(
    path: &Path,
    offset: u64,
    mut each: impl FnMut(Line, u64) -> bool,
) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut pos = if file.metadata()?.len() < offset {
        0
    } else {
        offset
    };
    file.seek(SeekFrom::Start(pos))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let mut too_long = false;
        let mut total = 0_u64;
        let complete = loop {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                break false;
            }
            let (take, found) = match chunk.iter().position(|b| *b == b'\n') {
                Some(i) => (i + 1, true),
                None => (chunk.len(), false),
            };
            if !too_long {
                buf.extend_from_slice(&chunk[..take]);
                if buf.len() > MAX_LINE_BYTES + 1 {
                    too_long = true;
                    buf = Vec::new();
                }
            }
            total += take as u64;
            reader.consume(take);
            if found {
                break true;
            }
        };
        if !complete {
            return Ok(pos);
        }
        pos += total;
        let line = if too_long {
            Line::TooLong
        } else {
            buf.pop();
            let text = String::from_utf8_lossy(&buf);
            Line::Text(text.trim_end_matches('\r').to_owned())
        };
        if !each(line, pos) {
            return Ok(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Scratch;
    use std::fs;

    fn read(path: &Path, offset: u64) -> (Vec<Line>, u64) {
        let mut lines = Vec::new();
        let end = for_each_line(path, offset, |l, _| {
            lines.push(l);
            true
        })
        .unwrap();
        (lines, end)
    }

    fn text(l: &str) -> Line {
        Line::Text(l.to_owned())
    }

    #[test]
    fn torn_last_line_is_left_for_later() {
        let dir = Scratch::new("torn");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "one\ntwo\nthr").unwrap();
        let (lines, offset) = read(&path, 0);
        assert_eq!(lines, [text("one"), text("two")]);
        assert_eq!(offset, 8);
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let (lines, offset) = read(&path, offset);
        assert_eq!(lines, [text("three")]);
        assert_eq!(offset, 14);
    }

    #[test]
    fn replaced_file_is_read_from_start() {
        let dir = Scratch::new("replaced");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\n").unwrap();
        let (lines, offset) = read(&path, 100);
        assert_eq!(lines, [text("a")]);
        assert_eq!(offset, 2);
    }

    #[test]
    fn carriage_returns_are_trimmed() {
        let dir = Scratch::new("crlf");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\r\nb\r\n").unwrap();
        let (lines, offset) = read(&path, 0);
        assert_eq!(lines, [text("a"), text("b")]);
        assert_eq!(offset, 6);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_replaced() {
        let dir = Scratch::new("lossy");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, b"a\xffb\n").unwrap();
        assert_eq!(read(&path, 0).0, [text("a\u{fffd}b")]);
    }

    #[test]
    fn a_file_without_a_newline_gives_nothing_yet() {
        let dir = Scratch::new("nonl");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "only").unwrap();
        assert_eq!(read(&path, 0), (Vec::new(), 0));
    }

    #[test]
    fn an_offset_at_the_end_gives_nothing_and_stays() {
        let dir = Scratch::new("end");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\nb\n").unwrap();
        assert_eq!(read(&path, 4), (Vec::new(), 4));
    }

    #[test]
    fn a_replaced_shorter_file_is_read_from_the_start() {
        let dir = Scratch::new("shorter");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "one\ntwo\n").unwrap();
        let (_, offset) = read(&path, 0);
        fs::write(&path, "x\n").unwrap();
        assert_eq!(read(&path, offset), (vec![text("x")], 2));
    }

    #[test]
    fn a_huge_line_is_dropped_and_the_next_one_is_read() {
        let dir = Scratch::new("huge");
        let path = dir.path().join("log.jsonl");
        let mut data = vec![b'x'; MAX_LINE_BYTES + 10];
        data.extend_from_slice(b"\nafter\r\n");
        fs::write(&path, &data).unwrap();
        let (lines, offset) = read(&path, 0);
        assert_eq!(lines, [Line::TooLong, text("after")]);
        assert_eq!(offset, data.len() as u64);
    }

    #[test]
    fn stopping_early_returns_the_offset_after_the_last_line_given() {
        let dir = Scratch::new("stop");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\nb\nc\n").unwrap();
        let end = for_each_line(&path, 0, |_, _| false).unwrap();
        assert_eq!(end, 2);
    }

    #[test]
    fn a_line_of_the_longest_size_is_read_and_one_byte_more_is_dropped() {
        let dir = Scratch::new("limit");
        let path = dir.path().join("log.jsonl");
        let mut data = vec![b'x'; MAX_LINE_BYTES];
        data.push(b'\n');
        data.extend(vec![b'y'; MAX_LINE_BYTES + 1]);
        data.extend_from_slice(b"\nend\n");
        fs::write(&path, &data).unwrap();
        let (lines, _) = read(&path, 0);
        assert_eq!(
            lines,
            [
                Line::Text("x".repeat(MAX_LINE_BYTES)),
                Line::TooLong,
                text("end")
            ]
        );
    }
}
