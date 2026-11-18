//! A scripted fake provider for the chat tests: request `n` is answered with the `n`th turn of the
//! script, and every request body is noted.
#![allow(clippy::unwrap_used, dead_code)]

use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::{done, head, hold, raw_chunk, read_request, status};

/// One model turn: the calls it asks for as `(name, arguments)`, a text when there are none, no
/// answer at all, a refusal with a server error, or another turn held back until a flag is set.
pub enum Turn {
    Calls(Vec<(&'static str, Value)>),
    Text(&'static str),
    Silence,
    Error,
    Gated(Arc<AtomicBool>, Box<Turn>),
}

/// A provider that answers request `n` with `turns[n]` and notes every request body.
pub fn provider(turns: Vec<Turn>) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&bodies);
    let turns = Arc::new(turns);
    thread::spawn(move || {
        let count = Arc::new(AtomicU32::new(0));
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (body, _) = read_request(&mut stream);
            noted.lock().unwrap().push(body);
            let n = count.fetch_add(1, Ordering::SeqCst) as usize;
            let turns = Arc::clone(&turns);
            thread::spawn(move || {
                let mut turn = turns.get(n);
                while let Some(Turn::Gated(open, inner)) = turn {
                    while !open.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_millis(10));
                    }
                    turn = Some(&**inner);
                }
                if matches!(turn, Some(Turn::Silence)) {
                    hold(&stream);
                    return;
                }
                if matches!(turn, Some(Turn::Error)) {
                    status(&mut stream, 500, "{}");
                    return;
                }
                head(&mut stream);
                let reason = match turn {
                    Some(Turn::Calls(calls)) => {
                        for (at, (name, arguments)) in calls.iter().enumerate() {
                            let call = json!({ "index": at, "id": format!("call_{n}_{at}"),
                                "type": "function",
                                "function": { "name": name, "arguments": arguments.to_string() } });
                            raw_chunk(
                                &mut stream,
                                &json!({ "choices": [{ "delta": { "tool_calls": [call] } }] }),
                            );
                        }
                        "tool_calls"
                    }
                    Some(Turn::Text(text)) => {
                        raw_chunk(
                            &mut stream,
                            &json!({ "choices": [{ "delta": { "content": text } }] }),
                        );
                        "stop"
                    }
                    _ => "stop",
                };
                raw_chunk(
                    &mut stream,
                    &json!({ "choices": [{ "delta": {}, "finish_reason": reason }] }),
                );
                done(&mut stream);
            });
        }
    });
    (addr, bodies)
}
