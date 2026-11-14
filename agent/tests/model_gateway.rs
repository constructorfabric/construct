//! The model gateway against a fake OpenAI-compatible server on the loopback address.
#![cfg(unix)]
// A failed `unwrap` in a test is a failed test.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use construct_agent::model::{ModelConfig, ModelGateway, ModelHealth, ModelState, ANSWER_LIMIT};
use http::{Method, Request, StatusCode};
use oagw_sdk::{Body, ServiceGatewayClientV1};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use toolkit_security::SecurityContext;

const KEY: &str = "sk-test-secret-key-0123456789";

/// What the fake server saw of one request.
#[derive(Clone, Debug)]
struct Seen {
    request_line: String,
    authorization: Option<String>,
    body: Value,
}

type Answer = Box<dyn Fn(&Seen, usize) -> (u16, String) + Send + Sync>;

/// A server that records each request and answers with `answer(request, index)`.
async fn fake_server(answer: Answer) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buf = Vec::new();
            let (head_end, length) = loop {
                let mut chunk = [0u8; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (pos + 4, length);
                }
                assert!(n > 0, "the client closed before the request ended");
            };
            while buf.len() < head_end + length {
                let mut chunk = [0u8; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let request = Seen {
                request_line: head.lines().next().unwrap().to_string(),
                authorization: head.lines().find_map(|l| {
                    l.to_lowercase()
                        .starts_with("authorization:")
                        .then(|| l.split_once(':').unwrap().1.trim().to_string())
                }),
                body: serde_json::from_slice(&buf[head_end..head_end + length]).unwrap(),
            };
            let index = {
                let mut log = log.lock().unwrap();
                log.push(request.clone());
                log.len() - 1
            };
            let (status, body) = answer(&request, index);
            let location = if (301..=308).contains(&status) {
                "Location: /v1/elsewhere\r\n"
            } else {
                ""
            };
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    (url, seen)
}

fn gateway(base_url: &str, key: &str) -> ModelGateway {
    gateway_with(base_url, key, ModelState::default())
}

fn gateway_with(base_url: &str, key: &str, state: ModelState) -> ModelGateway {
    ModelGateway::new(
        ModelConfig::new(base_url, "gpt-4.1", key, Some(5)).unwrap(),
        state,
    )
    .unwrap()
}

fn context() -> SecurityContext {
    SecurityContext::anonymous()
}

/// The planner's request: a chat-completions body with a tool and a strict `json_schema`
/// answer, posted to `/{upstream_alias}`; not streaming.
fn planner_request() -> Request<Body> {
    let body = json!({
        "model": "gpt-4.1",
        "messages": [
            { "role": "system", "content": "Plan the facts." },
            { "role": "user", "content": "I work as a nurse in Lyon." },
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "read_profile",
                "description": "Read the profile",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false },
            },
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "plan",
                "strict": true,
                "schema": { "type": "object", "properties": { "steps": { "type": "array" } } },
            },
        },
    });
    Request::builder()
        .method(Method::POST)
        .uri("/model")
        .header("content-type", "application/json")
        .body(Body::Bytes(body.to_string().into()))
        .unwrap()
}

async fn body_json(response: http::Response<Body>) -> Value {
    serde_json::from_slice(&response.into_body().into_bytes().await.unwrap()).unwrap()
}

#[tokio::test]
async fn the_planner_request_reaches_the_server_with_the_key_and_the_answer_comes_back() {
    let answer =
        json!({ "choices": [{ "message": { "role": "assistant", "content": "{\"steps\":[]}" } }] });
    let reply = answer.to_string();
    let (url, seen) = fake_server(Box::new(move |_, _| (200, reply.clone()))).await;

    let response = gateway(&url, KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, answer);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].request_line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(&*format!("Bearer {KEY}"))
    );
    // The body is the planner's, unchanged.
    assert_eq!(seen[0].body["model"], "gpt-4.1");
    assert_eq!(seen[0].body["tools"][0]["function"]["name"], "read_profile");
    assert_eq!(seen[0].body["response_format"]["type"], "json_schema");
    assert_eq!(
        seen[0].body["response_format"]["json_schema"]["strict"],
        true
    );
    assert!(seen[0].body.get("stream").is_none());
}

#[tokio::test]
async fn a_rejected_key_is_shown_once_and_not_retried_and_the_state_recovers() {
    // The API rejects the first call and accepts the second: the key was changed upstream.
    let (url, seen) = fake_server(Box::new(|_, index| {
        if index == 0 {
            // A real API quotes a part of the key it was given.
            (
                401,
                json!({ "error": { "message": "Incorrect API key provided: sk-wrong" } })
                    .to_string(),
            )
        } else {
            (200, "{}".to_string())
        }
    }))
    .await;

    let state = ModelState::default();
    let gateway = gateway_with(&url, "sk-wrong", state.clone());
    let response = gateway
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    // The user is told: `status` shows the key is rejected. Construct reads a 401 as a refusal.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.get(), ModelHealth::KeyRejected);
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "a refused key is not sent again"
    );

    // The same gateway, the next call is accepted: the state recovers.
    let again = gateway
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(state.get(), ModelHealth::Configured);
}

#[tokio::test]
async fn a_server_error_is_retried_and_the_later_answer_is_used() {
    let (url, seen) = fake_server(Box::new(|_, index| {
        if index < 2 {
            (503, "{}".to_string())
        } else {
            (200, json!({ "ok": true }).to_string())
        }
    }))
    .await;

    let response = gateway(&url, KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!({ "ok": true }));
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_server_that_keeps_failing_ends_in_the_status_after_three_attempts() {
    let (url, seen) = fake_server(Box::new(|_, _| (429, "{}".to_string()))).await;
    let response = gateway(&url, KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_server_that_is_not_there_is_unavailable() {
    // A port nothing listens on: bind, note the port, drop.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = gateway(&format!("http://127.0.0.1:{port}/v1"), KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap_err();
    assert!(!format!("{err:?}").contains(KEY));
    assert!(matches!(
        oagw_sdk::ServiceGatewayError::from(err),
        oagw_sdk::ServiceGatewayError::Unavailable { .. }
    ));
}

#[tokio::test]
async fn only_post_model_is_served_and_the_other_calls_are_not_implemented() {
    let (url, seen) = fake_server(Box::new(|_, _| (200, "{}".to_string()))).await;
    let gateway = gateway(&url, KEY);
    let other = Request::builder()
        .method(Method::POST)
        .uri("/elsewhere")
        .body(Body::Empty)
        .unwrap();
    assert!(gateway.proxy_request(context(), other).await.is_err());
    assert!(gateway
        .get_upstream(context(), uuid::Uuid::nil())
        .await
        .is_err());
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_server_that_never_answers_is_tried_three_times_and_is_unavailable() {
    // Accepts every connection, reads nothing back and never answers.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let accepted = Arc::new(Mutex::new(0usize));
    let count = accepted.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            *count.lock().unwrap() += 1;
            held.push(stream);
        }
    });

    let state = ModelState::default();
    let gateway = ModelGateway::new(
        ModelConfig::new(&url, "gpt-4.1", KEY, Some(1)).unwrap(),
        state.clone(),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let err = gateway
        .proxy_request(context(), planner_request())
        .await
        .unwrap_err();
    assert!(matches!(
        oagw_sdk::ServiceGatewayError::from(err),
        oagw_sdk::ServiceGatewayError::Unavailable { .. }
    ));
    assert_eq!(*accepted.lock().unwrap(), 3);
    // Three calls of one second each, and the pauses between them: bounded, not hung.
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // A hung API is not a rejected key.
    assert_eq!(state.get(), ModelHealth::Configured);
}

#[tokio::test]
async fn an_answer_over_the_limit_is_cut_and_never_read_whole() {
    let big = "x".repeat(2 * ANSWER_LIMIT);
    let (url, _) = fake_server(Box::new(move |_, _| (200, big.clone()))).await;
    let response = gateway(&url, KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    let body = response.into_body().into_bytes().await.unwrap();
    // One byte over the limit: enough for the caller to see that the answer is too large.
    assert_eq!(body.len(), ANSWER_LIMIT + 1);
}

#[tokio::test]
async fn a_redirect_is_returned_as_it_is_and_the_key_is_not_sent_again() {
    let (url, seen) = fake_server(Box::new(|_, _| (302, String::new()))).await;
    let response = gateway(&url, KEY)
        .proxy_request(context(), planner_request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "the redirect is not followed"
    );
}
