//! The model the user connected: its settings type with the check of the address, and the
//! [`Target`] a call goes to.
//!
//! The key is in no `Debug` output, error, log line or answer. It travels in one header of a call.

use std::fmt;
use std::sync::Mutex;

use reqwest::header::{self, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};

use crate::sync::lock;

/// The most bytes of an answer that is read in one piece.
pub const ANSWER_LIMIT: usize = 1 << 20;

/// The model the user connected.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    pub base_url: String,
    pub model: String,
    /// Never printed: `Debug` hides it, and the socket answers only whether it is set.
    pub api_key: Option<String>,
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key_set", &self.api_key.is_some())
            .finish()
    }
}

/// Where the chat reads the model from. It is asked for every call, so a change the user makes in
/// Settings takes effect on the next one.
pub trait ModelSource: Send + Sync {
    fn model(&self) -> Model;
}

/// Why the address or the key of a model was refused. The texts are fixed on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The model address is not an `https` address, nor `http` on `localhost` or `127.0.0.1`,
    /// or it has a user name or password in it.
    InvalidBaseUrl,
    /// The key has a control character, or a key and `remove_key` came together.
    InvalidKey,
}

impl Error {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Error::InvalidBaseUrl => {
                "base_url must be an https address, or http on localhost or 127.0.0.1, without a user name or password"
            }
            Error::InvalidKey => "invalid api_key",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for Error {}

/// Check the model address: `https`, or `http` only for the host `localhost` or `127.0.0.1`,
/// and no user name or password. The host is compared after parsing. An empty address means
/// no model.
///
/// # Errors
/// [`Error::InvalidBaseUrl`] for any other address.
pub fn check_base_url(raw: &str) -> Result<(), Error> {
    if raw.is_empty() {
        return Ok(());
    }
    let url = Url::parse(raw).map_err(|_| Error::InvalidBaseUrl)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidBaseUrl);
    }
    // The parser writes a domain in lower case and an IPv4 address in its dotted form, whatever
    // spelling it was given in; an IPv6 address keeps its brackets and is never local here.
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    match url.scheme() {
        "https" if url.host().is_some() => Ok(()),
        "http" if local => Ok(()),
        _ => Err(Error::InvalidBaseUrl),
    }
}

/// Check a key: no control character. Shared by `settings-set` and `settings-test`.
///
/// # Errors
/// [`Error::InvalidKey`] for a key with a control character.
pub fn check_key(key: &str) -> Result<(), Error> {
    if key.chars().any(char::is_control) {
        return Err(Error::InvalidKey);
    }
    Ok(())
}

/// A model the settings describe completely, checked for one call.
pub struct Target {
    /// The API root, without a trailing `/`: the path `/chat/completions` is added to it.
    base_url: String,
    pub model: String,
    /// `Bearer <key>`, marked sensitive.
    pub authorization: HeaderValue,
}

impl Target {
    /// `None` when the address, the model name or the key is missing or cannot be used. The
    /// address is checked here too: a file edited by hand, or written by an older agent, must
    /// not make the key go out over plain `http` to another host.
    #[must_use]
    pub fn from_settings(model: &Model) -> Option<Self> {
        let base_url = model.base_url.trim().trim_end_matches('/');
        let key = model.api_key.as_deref()?.trim();
        let name = model.model.as_str();
        if base_url.is_empty()
            || check_base_url(base_url).is_err()
            || key.is_empty()
            || name.is_empty()
            || name.len() > 128
            || name.contains(|c: char| c.is_whitespace() || c.is_control())
        {
            return None;
        }
        // The key goes into a header: it must be a legal value.
        let mut authorization = HeaderValue::from_str(&format!("Bearer {key}")).ok()?;
        authorization.set_sensitive(true);
        Some(Self {
            base_url: base_url.to_string(),
            model: name.to_string(),
            authorization,
        })
    }

    /// The API root the calls go to.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    /// The POST to the chat-completions address with the key as a sensitive header: the one
    /// place that decides where the key goes. `accept` is the media type the caller reads.
    pub fn request(
        &self,
        client: &reqwest::Client,
        accept: &'static str,
        body: impl Into<reqwest::Body>,
    ) -> reqwest::RequestBuilder {
        client
            .post(self.url())
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, accept)
            .body(body)
    }
}

/// The models (address and name) the API refused function tools for next to a reasoning effort.
/// A request with tools to one of them carries `reasoning_effort: "none"` from then on, so the
/// refusal costs one extra request per model and run.
static TOOLS_WITHOUT_REASONING: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

impl Target {
    /// `body` with `reasoning_effort: "none"` when this model refused tools next to a reasoning
    /// effort before; `None` when the body goes out as it is.
    #[must_use]
    pub fn prepared(&self, body: &[u8]) -> Option<Vec<u8>> {
        let known = lock(&TOOLS_WITHOUT_REASONING)
            .iter()
            .any(|(url, model)| *url == self.base_url && *model == self.model);
        if known { without_reasoning(body) } else { None }
    }

    /// Note that this model refused tools next to a reasoning effort.
    pub fn refused_tools_with_reasoning(&self) {
        let mut known = lock(&TOOLS_WITHOUT_REASONING);
        let entry = (self.base_url.clone(), self.model.clone());
        if !known.contains(&entry) {
            known.push(entry);
        }
    }
}

/// Whether the API answered that function tools and a reasoning effort do not go together (the
/// newer models do this in `/chat/completions`, and accept the tools with `none`).
#[must_use]
pub fn refuses_tools_with_reasoning(status: u16, answer: &[u8]) -> bool {
    status == StatusCode::BAD_REQUEST.as_u16() && {
        let answer = String::from_utf8_lossy(answer);
        answer.contains("tools") && answer.contains("reasoning_effort")
    }
}

/// `body` with `reasoning_effort: "none"`; `None` for a request without tools, which that
/// refusal is not about, and for a body that is not a JSON object.
#[must_use]
pub fn without_reasoning(body: &[u8]) -> Option<Vec<u8>> {
    let Ok(serde_json::Value::Object(mut map)) = serde_json::from_slice(body) else {
        return None;
    };
    map.get("tools")?;
    map.insert("reasoning_effort".into(), "none".into());
    serde_json::to_vec(&map).ok()
}

/// The body, up to `limit`. A body over it is cut there (one byte past the limit is kept, so
/// the caller can tell), which makes it a bad answer instead of memory for the host.
///
/// # Errors
/// When the body cannot be read to its end.
pub async fn read_capped(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, reqwest::Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > limit {
            bytes.truncate(limit + 1);
            break;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_in_no_debug_output_and_no_error_text() {
        let model = Model {
            base_url: "https://x.example/v1".into(),
            model: "m".into(),
            api_key: Some("sk-top-secret".into()),
        };
        assert!(!format!("{model:?}").contains("sk-top-secret"));
        assert!(format!("{model:?}").contains("api_key_set: true"));
        for error in [Error::InvalidBaseUrl, Error::InvalidKey] {
            assert!(!error.as_str().contains("sk-top-secret"));
        }
        assert_eq!(check_key("sk-top\nsecret"), Err(Error::InvalidKey));
    }

    #[test]
    fn addresses_are_checked_by_parsed_host() {
        for ok in [
            "https://api.example.com/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "https://localhost",
            "http://LOCALHOST/v1",
            "",
        ] {
            assert_eq!(check_base_url(ok), Ok(()), "{ok}");
        }
        for bad in [
            "http://example.com",
            "http://localhost.example.com",
            "http://127.0.0.1.example.com",
            "http://localhostevil.com",
            "http://[::1]:8080",
            "http://0.0.0.0",
            "https://user:pass@example.com",
            "http://localhost@example.com",
            "ftp://example.com",
            "file:///etc/passwd",
            "example.com",
            "https://",
        ] {
            assert_eq!(check_base_url(bad), Err(Error::InvalidBaseUrl), "{bad}");
        }
    }

    #[test]
    fn a_target_needs_a_checked_address_a_name_and_a_key() {
        let model = |url: &str, name: &str, key: Option<&str>| Model {
            base_url: url.into(),
            model: name.into(),
            api_key: key.map(Into::into),
        };
        let ok = Target::from_settings(&model("https://x.example/v1/", "m", Some("k"))).unwrap();
        assert_eq!(ok.base_url(), "https://x.example/v1");
        for bad in [
            model("", "m", Some("k")),
            model("http://example.com", "m", Some("k")),
            model("https://x.example", "", Some("k")),
            model("https://x.example", "a b", Some("k")),
            model("https://x.example", "m", None),
            model("https://x.example", "m", Some(" ")),
            model("https://x.example", "m", Some("k\nk")),
        ] {
            assert!(Target::from_settings(&bad).is_none(), "{bad:?}");
        }
    }
}
