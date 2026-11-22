//! The answer of the provider as server-sent events: plain text, events separated by a blank line,
//! the `data:` lines of an event joined by newlines. Only what the chat reads is kept: the data.
//!
//! The reading follows the W3C text: the three line endings, a byte order mark at the start,
//! comments, a field with no colon, one space dropped after the colon, and an unknown field
//! ignored. An event that has no field at all is not an event.

use std::collections::VecDeque;

use reqwest::header;

/// Why the stream of events broke.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StreamError {
    /// The answer went past the size cap given to [`Events::new`].
    Cap,
    /// The connection broke, or the bytes are not text.
    Broken,
}

/// The `data` of the events of a response, one by one.
pub(crate) struct Events {
    response: reqwest::Response,
    parser: Parser,
    pending: VecDeque<String>,
    limit: usize,
    total: usize,
    done: bool,
}

/// Whether `response` says it is an event stream.
pub(crate) fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.starts_with("text/event-stream"))
}

impl Events {
    /// The events of `response`. The stream ends with [`StreamError::Cap`] at `limit` bytes: the
    /// parser does not limit its buffer.
    pub(crate) fn new(response: reqwest::Response, limit: usize) -> Self {
        Self {
            response,
            parser: Parser::default(),
            pending: VecDeque::new(),
            limit,
            total: 0,
            done: false,
        }
    }

    /// The data of the next event; `None` at the end of the stream.
    pub(crate) async fn next(&mut self) -> Option<Result<String, StreamError>> {
        loop {
            if let Some(data) = self.pending.pop_front() {
                return Some(Ok(data));
            }
            if self.done {
                return None;
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => {
                    self.total += chunk.len();
                    if self.total > self.limit {
                        self.done = true;
                        return Some(Err(StreamError::Cap));
                    }
                    match self.parser.push(&chunk) {
                        Ok(events) => self.pending = events,
                        Err(error) => {
                            self.done = true;
                            return Some(Err(error));
                        }
                    }
                }
                Ok(None) => {
                    self.done = true;
                    self.pending = self.parser.finish();
                }
                Err(_) => {
                    self.done = true;
                    return Some(Err(StreamError::Broken));
                }
            }
        }
    }
}

/// The event being read: its `data`, and whether any field was seen.
#[derive(Default)]
struct Event {
    data: String,
    seen: bool,
}

impl Event {
    fn line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => {
                self.seen = true;
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
            "event" => self.seen = true,
            "id" => self.seen |= !value.contains('\0'),
            "retry" => self.seen |= value.parse::<u64>().is_ok(),
            _ => {}
        }
    }

    fn block(text: &str) -> Option<String> {
        let mut event = Self::default();
        for line in text.lines() {
            event.line(line);
        }
        event.seen.then_some(event.data)
    }
}

/// Bytes in, the data of the whole events out.
#[derive(Default)]
struct Parser {
    buffer: String,
    /// The end of the last chunk when it cut a character in two.
    tail: Vec<u8>,
    started: bool,
}

impl Parser {
    /// The data of the events that `chunk` completes.
    fn push(&mut self, chunk: &[u8]) -> Result<VecDeque<String>, StreamError> {
        let mut bytes = std::mem::take(&mut self.tail);
        bytes.extend_from_slice(chunk);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(e) if e.error_len().is_none() => {
                let (whole, rest) = bytes.split_at(e.valid_up_to());
                self.tail = rest.to_vec();
                std::str::from_utf8(whole).map_err(|_| StreamError::Broken)?
            }
            Err(_) => return Err(StreamError::Broken),
        };
        if text.is_empty() {
            return Ok(VecDeque::new());
        }
        let text = if self.started {
            text
        } else {
            self.started = true;
            text.strip_prefix('\u{FEFF}').unwrap_or(text)
        };
        self.buffer
            .push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
        Ok(self.events())
    }

    /// The whole events in the buffer; what follows the last blank line stays.
    fn events(&mut self) -> VecDeque<String> {
        let mut events = VecDeque::new();
        while let Some(end) = self.buffer.find("\n\n") {
            if end > 0 {
                events.extend(Event::block(&self.buffer[..end]));
            }
            let rest = self.buffer[end + 2..].trim_start_matches('\n').to_owned();
            self.buffer = rest;
        }
        events
    }

    /// The event the stream ended in the middle of, if any.
    fn finish(&mut self) -> VecDeque<String> {
        let rest = std::mem::take(&mut self.buffer);
        if rest.trim().is_empty() {
            return VecDeque::new();
        }
        Event::block(&rest).into_iter().collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn read(chunks: &[&[u8]]) -> Vec<String> {
        let mut parser = Parser::default();
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(parser.push(chunk).unwrap());
        }
        out.extend(parser.finish());
        out
    }

    #[test]
    fn events_are_cut_at_blank_lines_whatever_the_chunks() {
        let whole = read(&[b"data: one\n\ndata: two\n\n"]);
        assert_eq!(whole, ["one", "two"]);
        let split = read(&[b"data: o", b"ne\n", b"\ndata: two\n\n"]);
        assert_eq!(split, whole);
    }

    #[test]
    fn the_three_line_endings_and_the_data_lines_are_read_as_the_text_says() {
        assert_eq!(read(&[b"data: a\r\ndata: b\r\n\r\n"]), ["a\nb"]);
        assert_eq!(read(&[b"data:a\r\rdata:  b\r\r"]), ["a", " b"]);
        assert_eq!(read(&[b"\xEF\xBB\xBFdata: a\n\n"]), ["a"]);
        assert_eq!(
            read(&[b": keep-alive\n\nevent: x\ndata: a\nid: 4\n\n"]),
            ["a"]
        );
        assert_eq!(read(&[b"data\n\n"]), [""]);
    }

    #[test]
    fn a_character_cut_between_two_chunks_is_put_together() {
        let text = "data: é\n\n".as_bytes();
        let cut = text.iter().position(|&b| b == 0xC3).unwrap() + 1;
        assert_eq!(read(&[&text[..cut], &text[cut..]]), ["é"]);
    }

    #[test]
    fn an_event_with_no_field_and_a_last_event_without_a_blank_line() {
        assert!(read(&[b": only a comment\n\n"]).is_empty());
        assert_eq!(read(&[b"data: tail"]), ["tail"]);
    }

    #[test]
    fn bytes_that_are_not_text_break_the_stream() {
        assert_eq!(
            Parser::default().push(b"data: \xFF\n\n").unwrap_err(),
            StreamError::Broken
        );
    }
}
