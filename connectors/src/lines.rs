use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// The complete lines of `path` from byte `offset` on, and the offset after the last of
/// them. A last line without its newline is still being written (or was torn by a crash):
/// it is left for the next call. A file shorter than `offset` was replaced and is read
/// from the start. Bytes that are not UTF-8 are replaced.
///
/// # Errors
/// Fails when the file cannot be opened or read.
pub fn read_new_lines(path: &Path, offset: u64) -> io::Result<(Vec<String>, u64)> {
    let mut file = File::open(path)?;
    let start = if file.metadata()?.len() < offset {
        0
    } else {
        offset
    };
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else {
        return Ok((Vec::new(), start));
    };
    let lines = bytes[..end]
        .split(|b| *b == b'\n')
        .map(|l| String::from_utf8_lossy(l).trim_end_matches('\r').to_owned())
        .collect();
    Ok((lines, start + end as u64 + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Scratch;
    use std::fs;

    #[test]
    fn torn_last_line_is_left_for_later() {
        let dir = Scratch::new("torn");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "one\ntwo\nthr").unwrap();
        let (lines, offset) = read_new_lines(&path, 0).unwrap();
        assert_eq!(lines, ["one", "two"]);
        assert_eq!(offset, 8);
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let (lines, offset) = read_new_lines(&path, offset).unwrap();
        assert_eq!(lines, ["three"]);
        assert_eq!(offset, 14);
    }

    #[test]
    fn replaced_file_is_read_from_start() {
        let dir = Scratch::new("replaced");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\n").unwrap();
        let (lines, _) = read_new_lines(&path, 100).unwrap();
        assert_eq!(lines, ["a"]);
    }

    #[test]
    fn carriage_returns_are_trimmed() {
        let dir = Scratch::new("crlf");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\r\nb\r\n").unwrap();
        let (lines, offset) = read_new_lines(&path, 0).unwrap();
        assert_eq!(lines, ["a", "b"]);
        assert_eq!(offset, 6);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_replaced() {
        let dir = Scratch::new("lossy");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, b"a\xffb\n").unwrap();
        let (lines, _) = read_new_lines(&path, 0).unwrap();
        assert_eq!(lines, ["a\u{fffd}b"]);
    }

    #[test]
    fn a_file_without_a_newline_gives_nothing_yet() {
        let dir = Scratch::new("nonl");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "only").unwrap();
        assert_eq!(read_new_lines(&path, 0).unwrap(), (Vec::new(), 0));
    }

    #[test]
    fn an_offset_at_the_end_gives_nothing_and_stays() {
        let dir = Scratch::new("end");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "a\nb\n").unwrap();
        assert_eq!(read_new_lines(&path, 4).unwrap(), (Vec::new(), 4));
    }

    #[test]
    fn a_replaced_shorter_file_is_read_from_the_start() {
        let dir = Scratch::new("shorter");
        let path = dir.path().join("log.jsonl");
        fs::write(&path, "one\ntwo\n").unwrap();
        let (_, offset) = read_new_lines(&path, 0).unwrap();
        fs::write(&path, "x\n").unwrap();
        assert_eq!(
            read_new_lines(&path, offset).unwrap(),
            (vec!["x".to_owned()], 2)
        );
    }
}
