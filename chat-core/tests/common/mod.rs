//! A fake provider on a local port, and the pieces of a host the tests give the chat.
#![allow(clippy::unwrap_used, dead_code)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chat_core::model::{Model, ModelSource};
use chat_core::records::{Records, SessionInfo};
use connectors::Record;

/// What the fake provider does with one connection.
pub enum Reply {
    /// A plain answer with this status and body.
    Status(u16, &'static str),
    /// An event stream of these `data:` lines, then `[DONE]`.
    Events(Vec<String>),
    /// Read the request and say nothing.
    Hang,
}

pub struct Fake {
    pub addr: SocketAddr,
    /// The body of every request, in the order they came.
    pub bodies: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    /// The n-th connection gets the n-th reply; the last one repeats.
    pub fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&bodies);
        thread::spawn(move || {
            let mut open = Vec::new();
            for (n, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                seen.lock().unwrap().push(read_body(&mut stream));
                match &replies[n.min(replies.len() - 1)] {
                    Reply::Status(status, body) => {
                        let preamble = format!(
                            "HTTP/1.1 {status} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(preamble.as_bytes());
                        let _ = stream.write_all(body.as_bytes());
                    }
                    Reply::Events(lines) => {
                        let mut text = String::new();
                        for line in lines {
                            write!(text, "data: {line}\n\n").unwrap();
                        }
                        text.push_str("data: [DONE]\n\n");
                        let preamble = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(preamble.as_bytes());
                        let _ = stream.write_all(text.as_bytes());
                    }
                    Reply::Hang => open.push(stream),
                }
            }
        });
        Self { addr, bodies }
    }

    /// An address nobody listens on.
    pub fn closed() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    }
}

/// The body of the request on `stream`, read by its `Content-Length`.
fn read_body(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut data = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (head_end, length) = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "the client closed before the request was whole");
        data.extend_from_slice(&chunk[..n]);
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..at]).to_lowercase();
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map_or(0, |v| v.trim().parse::<usize>().unwrap());
            break (at + 4, length);
        }
    };
    while data.len() < head_end + length {
        let n = stream.read(&mut chunk).unwrap();
        data.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(data[head_end..head_end + length].to_vec()).unwrap()
}

/// The model of a fake provider on `addr`.
pub fn model_at(addr: SocketAddr) -> Model {
    Model {
        base_url: format!("http://127.0.0.1:{}/v1", addr.port()),
        model: "m".into(),
        api_key: Some("sk-test-key-4f9a".into()),
    }
}

pub struct Fixed(pub Model);

impl ModelSource for Fixed {
    fn model(&self) -> Model {
        self.0.clone()
    }
}

/// A store of records in memory, shared with the test.
#[derive(Clone, Default)]
pub struct Memory(pub Arc<Mutex<Vec<Record>>>);

impl Records for Memory {
    fn read_session(&self, _: &str, session: &str) -> std::io::Result<Vec<Record>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.session() == session)
            .cloned()
            .collect())
    }

    fn list_sessions(&self, _: &str) -> std::io::Result<Vec<SessionInfo>> {
        Ok(Vec::new())
    }

    fn append(&mut self, record: &Record) -> std::io::Result<bool> {
        self.0.lock().unwrap().push(record.clone());
        Ok(true)
    }

    fn flush_session(&mut self, _: &str, _: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn delete_session(&mut self, _: &str, session: &str) -> std::io::Result<()> {
        self.0.lock().unwrap().retain(|r| r.session() != session);
        Ok(())
    }

    fn delete_source(&mut self, _: &str) -> std::io::Result<()> {
        self.0.lock().unwrap().clear();
        Ok(())
    }
}
