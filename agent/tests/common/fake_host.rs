//! A fake Construct host for the tests: a local port that records the calls to the facts route
//! and answers as scripted.
#![allow(clippy::unwrap_used, dead_code)]

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use construct_agent::host::Connection;
use serde_json::Value;

pub const TOKEN: &str = "manage-token-77c1";
pub const STORED: &str = r#"{"outcome":"stored","reason":null,"added":1,"replaced":0,"removed":0}"#;
pub const NO_CHANGE: &str =
    r#"{"outcome":"nothing_changed","reason":"no_change","added":0,"replaced":0,"removed":0}"#;

pub struct Call {
    pub method: String,
    pub path: String,
    pub authorization: String,
    pub body: Value,
}

/// A host that answers every call with the next scripted answer, then with `STORED`.
pub struct FakeHost {
    pub addr: SocketAddr,
    pub calls: Arc<Mutex<Vec<Call>>>,
    pub script: Arc<Mutex<VecDeque<(u16, String)>>>,
    pub delay: Arc<Mutex<Duration>>,
}

impl FakeHost {
    pub fn start() -> Self {
        Self::start_at(0)
    }

    pub fn start_at(port: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let addr = listener.local_addr().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let script = Arc::new(Mutex::new(VecDeque::new()));
        let delay = Arc::new(Mutex::new(Duration::ZERO));
        let (c, s, d) = (Arc::clone(&calls), Arc::clone(&script), Arc::clone(&delay));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let call = read_call(&mut stream);
                c.lock().unwrap().push(call);
                thread::sleep(*d.lock().unwrap());
                let (code, body) = s
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or((200, STORED.to_string()));
                super::status(&mut stream, code, &body);
            }
        });
        Self {
            addr,
            calls,
            script,
            delay,
        }
    }

    pub fn connection(&self) -> Connection {
        Connection {
            port: self.addr.port(),
            token: TOKEN.into(),
        }
    }

    pub fn answer(&self, code: u16, body: &str) {
        self.script.lock().unwrap().push_back((code, body.into()));
    }

    pub fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// The `message` of every call so far.
    pub fn messages(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| c.body["message"].as_str().map(str::to_string))
            .collect()
    }
}

pub fn read_call(stream: &mut TcpStream) -> Call {
    let request = super::read_http(stream);
    // A call without a body (a read) is recorded with a null one.
    let body = if request.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&request.body).unwrap()
    };
    Call {
        method: request.method,
        path: request.path,
        authorization: request.authorization,
        body,
    }
}
