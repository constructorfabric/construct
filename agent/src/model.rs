//! The model route of the host: a small gateway of our own that stands in for OAGW.
//!
//! Construct looks up `dyn ServiceGatewayClientV1` in its client hub for every model call and
//! posts the planner's chat-completions request to `/{upstream_alias}`. OAGW 0.7.0 needs a
//! credential store and a tenant resolver at start, which the host does not have, so the host
//! registers [`ModelGateway`] instead. It forwards the request to the user's OpenAI-compatible
//! API (`{base_url}/chat/completions`) with the user's key.
//!
//! Base URL, model and key come from `$XDG_CONFIG_HOME/construct/agent.toml` (default
//! `~/.config/construct/agent.toml`), which must be readable by the user only:
//!
//! ```toml
//! [model]
//! base_url = "https://api.openai.com/v1"
//! model = "gpt-4.1"
//! api_key = "sk-..."
//! timeout_secs = 60   # optional, 1 to 190: the limit for ONE attempt
//! ```
//!
//! The key lives in memory of this type only. It is not part of `Debug`, errors, log lines or
//! the agent's status.

use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use oagw_sdk::{
    Body, CreateRouteRequest, CreateUpstreamRequest, ListQuery, Route, ServiceGatewayClientV1,
    UpdateRouteRequest, UpdateUpstreamRequest, Upstream,
};
use serde::Deserialize;
use toolkit_canonical_errors::CanonicalError;
use toolkit_security::SecurityContext;
use uuid::Uuid;

use crate::ipc::absolute_env_path;

/// The upstream alias Construct is told to call. Its own gateway is the only one that answers it.
pub const UPSTREAM_ALIAS: &str = "model";
/// Name of the file in `$XDG_CONFIG_HOME/construct`.
pub const CONFIG_FILE: &str = "agent.toml";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
/// The limit for one attempt. Construct limits the whole call to 600 seconds and gets the
/// budget of every attempt (see [`ModelConfig::call_budget_secs`]), so one attempt is held to a third.
const TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 1..=190;
/// Attempts for one call: the first and the retries after a failure that may pass.
const ATTEMPTS: u32 = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(500);
/// The most bytes of an answer the gateway reads.
pub const ANSWER_LIMIT: usize = 1 << 20;

/// The `[model]` table of `agent.toml`, validated.
#[derive(Clone, PartialEq, Eq)]
pub struct ModelConfig {
    /// The API root, without a trailing `/`: the path `/chat/completions` is added to it.
    pub base_url: String,
    /// The model name the API expects.
    pub model: String,
    /// The limit for one model call.
    pub timeout: Duration,
    /// `Bearer <key>`, built and checked once at load, marked sensitive.
    authorization: HeaderValue,
}

// The key never reaches a log line through `{:?}`.
impl fmt::Debug for ModelConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    model: Option<RawModel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    base_url: String,
    model: String,
    api_key: String,
    timeout_secs: Option<u64>,
}

impl ModelConfig {
    /// Build a config; for the tests and for [`ModelConfig::parse`].
    ///
    /// # Errors
    /// Fails when a value is empty, the URL is not `https://` (`http://` is allowed for a
    /// loopback host only, so the key never crosses a network in the clear), the model name holds a space
    /// or a control character, or the timeout is outside 1 to 190 seconds.
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: &str,
        timeout_secs: Option<u64>,
    ) -> Result<Self, ConfigError> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.contains(char::is_whitespace) || !is_safe_url(base_url) {
            return Err(ConfigError::Invalid(
                "model.base_url must be an https:// URL (http:// only for localhost)",
            ));
        }
        if model.is_empty()
            || model.len() > 128
            || model.contains(|c: char| c.is_whitespace() || c.is_control())
        {
            return Err(ConfigError::Invalid(
                "model.model must be 1 to 128 characters with no space",
            ));
        }
        // The key goes into a header: it must be a legal value, and must not be blank.
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
            .ok()
            .filter(|_| !api_key.trim().is_empty())
            .ok_or(ConfigError::Invalid("model.api_key is empty or not valid"))?;
        authorization.set_sensitive(true);
        let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        if !TIMEOUT_RANGE.contains(&secs) {
            return Err(ConfigError::Invalid("model.timeout_secs must be 1 to 190"));
        }
        Ok(Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            timeout: Duration::from_secs(secs),
            authorization,
        })
    }

    /// Read the `[model]` table from the TOML text of `agent.toml`. `None` when there is none.
    ///
    /// # Errors
    /// Fails when the text is not valid TOML, holds an unknown key, or a value is refused.
    /// The message never holds the text of the file.
    pub fn parse(text: &str) -> Result<Option<Self>, ConfigError> {
        // The parser's own message quotes the source line, which may be the key line.
        let file: FileConfig = toml::from_str(text).map_err(|e| ConfigError::Syntax(e.span()))?;
        file.model
            .map(|m| Self::new(&m.base_url, &m.model, &m.api_key, m.timeout_secs))
            .transpose()
    }

    /// Load `path`, which must be a file only the user can read.
    ///
    /// # Errors
    /// Fails when the file cannot be read, is readable by others, or is refused by
    /// [`ModelConfig::parse`].
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        let meta = fs::metadata(path).map_err(ConfigError::Read)?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(ConfigError::Mode(meta.permissions().mode() & 0o777));
        }
        let text = fs::read_to_string(path).map_err(ConfigError::Read)?;
        Self::parse(&text)
    }

    /// Load the file in the user's config directory; `None` when there is no file.
    ///
    /// # Errors
    /// As [`ModelConfig::load`].
    pub fn load_default() -> Result<Option<Self>, ConfigError> {
        let Some(path) = config_path() else {
            return Ok(None);
        };
        match Self::load(&path) {
            Err(ConfigError::Read(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            other => other,
        }
    }

    /// The limit Construct puts on the whole model call: it wraps `proxy_request`, which makes
    /// up to [`ATTEMPTS`] attempts of `timeout` each with the pauses between them. Giving Construct
    /// the per-attempt number would cut the call off after the first attempt.
    #[must_use]
    pub fn call_budget_secs(&self) -> u64 {
        let pauses: Duration = (1..ATTEMPTS).map(|n| RETRY_PAUSE * n).sum();
        // The pauses rounded up, and a second for the work around the attempts.
        self.timeout.as_secs() * u64::from(ATTEMPTS) + pauses.as_secs() + 2
    }

    /// The Construct `model` section for this config.
    #[must_use]
    pub fn construct_section(&self) -> serde_json::Value {
        serde_json::json!({
            "upstream_alias": UPSTREAM_ALIAS,
            "model": self.model,
            "timeout": format!("{}s", self.call_budget_secs()),
        })
    }
}

/// `https://` for any host, `http://` only for a loopback host (`localhost`, `127.0.0.1`, `[::1]`),
/// and no user info in the authority: `http://localhost@example.com` is not a loopback host.
fn is_safe_url(url: &str) -> bool {
    let (https, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        (None, None) => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() || authority.contains('@') {
        return false;
    }
    https || matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Why `agent.toml` is not used. No variant holds the key.
#[derive(Debug)]
pub enum ConfigError {
    Read(io::Error),
    Mode(u32),
    Syntax(Option<std::ops::Range<usize>>),
    Invalid(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "cannot read the file: {}", e.kind()),
            Self::Mode(mode) => write!(
                f,
                "the file is readable by others (mode {mode:o}); run chmod 600 on it"
            ),
            Self::Syntax(Some(span)) => write!(f, "not valid TOML near byte {}", span.start),
            Self::Syntax(None) => write!(f, "not valid TOML"),
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `$XDG_CONFIG_HOME/construct/agent.toml`, or under `~/.config`.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    let config_home = absolute_env_path("XDG_CONFIG_HOME")
        .or_else(|| absolute_env_path("HOME").map(|home| home.join(".config")))?;
    Some(config_home.join("construct").join(CONFIG_FILE))
}

/// What the agent knows of its model, shown in `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelHealth {
    /// No `[model]` in `agent.toml`: records are taken and dropped.
    NotConfigured,
    /// A model is set up and the last call was not refused for its key.
    Configured,
    /// The API refused the key (`401` or `403`) on the last call. Only the user can fix it:
    /// change the key in `agent.toml` and restart the agent.
    KeyRejected,
}

impl ModelHealth {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Configured => "configured",
            Self::KeyRejected => "key_rejected",
        }
    }
}

/// The model state, shared between the gateway and the socket threads.
#[derive(Clone, Debug)]
pub struct ModelState(Arc<Mutex<ModelHealth>>);

impl ModelState {
    #[must_use]
    pub fn new(health: ModelHealth) -> Self {
        Self(Arc::new(Mutex::new(health)))
    }

    #[must_use]
    pub fn get(&self) -> ModelHealth {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set(&self, health: ModelHealth) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = health;
    }
}

impl Default for ModelState {
    fn default() -> Self {
        Self::new(ModelHealth::NotConfigured)
    }
}

/// The gateway Construct calls for its model. Only `proxy_request` does anything.
pub struct ModelGateway {
    config: ModelConfig,
    client: reqwest::Client,
    state: ModelState,
}

impl fmt::Debug for ModelGateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelGateway")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ModelGateway {
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn new(config: ModelConfig, state: ModelState) -> Result<Self, reqwest::Error> {
        state.set(ModelHealth::Configured);
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            config,
            client,
            state,
        })
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.config.base_url)
    }

    /// One attempt: the answer's status and body, or why there is none.
    async fn attempt(&self, body: &Bytes) -> Result<(StatusCode, Bytes), reqwest::Error> {
        let response = self
            .client
            .post(self.url())
            .header(header::AUTHORIZATION, self.config.authorization.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json")
            .body(body.clone())
            .send()
            .await?;
        let status = response.status();
        let answer = read_capped(response).await?;
        Ok((status, answer))
    }
}

/// The body, up to the limit. An answer over the limit is cut there, which makes it a bad
/// answer for the caller instead of memory for the host.
async fn read_capped(mut response: reqwest::Response) -> Result<Bytes, reqwest::Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > ANSWER_LIMIT {
            bytes.truncate(ANSWER_LIMIT + 1);
            break;
        }
    }
    Ok(Bytes::from(bytes))
}

/// A failure that may pass on its own: the next attempt can work.
fn is_transient(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn unimplemented_call() -> CanonicalError {
    CanonicalError::internal("the host's model gateway only forwards model calls").create()
}

#[async_trait]
impl ServiceGatewayClientV1 for ModelGateway {
    async fn create_upstream(
        &self,
        _: SecurityContext,
        _: CreateUpstreamRequest,
    ) -> Result<Upstream, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn get_upstream(&self, _: SecurityContext, _: Uuid) -> Result<Upstream, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn list_upstreams(
        &self,
        _: SecurityContext,
        _: &ListQuery,
    ) -> Result<Vec<Upstream>, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn update_upstream(
        &self,
        _: SecurityContext,
        _: Uuid,
        _: UpdateUpstreamRequest,
    ) -> Result<Upstream, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn delete_upstream(&self, _: SecurityContext, _: Uuid) -> Result<(), CanonicalError> {
        Err(unimplemented_call())
    }

    async fn create_route(
        &self,
        _: SecurityContext,
        _: CreateRouteRequest,
    ) -> Result<Route, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn get_route(&self, _: SecurityContext, _: Uuid) -> Result<Route, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn list_routes(
        &self,
        _: SecurityContext,
        _: Option<Uuid>,
        _: &ListQuery,
    ) -> Result<Vec<Route>, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn update_route(
        &self,
        _: SecurityContext,
        _: Uuid,
        _: UpdateRouteRequest,
    ) -> Result<Route, CanonicalError> {
        Err(unimplemented_call())
    }

    async fn delete_route(&self, _: SecurityContext, _: Uuid) -> Result<(), CanonicalError> {
        Err(unimplemented_call())
    }

    async fn resolve_proxy_target(
        &self,
        _: SecurityContext,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<(Upstream, Route), CanonicalError> {
        Err(unimplemented_call())
    }

    async fn proxy_request(
        &self,
        _: SecurityContext,
        req: Request<Body>,
    ) -> Result<Response<Body>, CanonicalError> {
        let (parts, body) = req.into_parts();
        if parts.method != Method::POST || parts.uri.path() != format!("/{UPSTREAM_ALIAS}") {
            return Err(CanonicalError::internal(
                "the host's model gateway answers POST /model only",
            )
            .create());
        }
        let body = body
            .into_bytes()
            .await
            .map_err(|_| CanonicalError::internal("the request body could not be read").create())?;

        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            match self.attempt(&body).await {
                Ok((status, answer)) if !is_transient(status) => {
                    // The status only: the body of an error may quote the request.
                    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                        // Not retried: the key stays as wrong as it was. The user is told in
                        // `status` and in the log, and fixes it in `agent.toml`.
                        self.state.set(ModelHealth::KeyRejected);
                        eprintln!(
                            "model: the API rejected the key ({status}); put a working key in agent.toml and restart the agent"
                        );
                    } else if status.is_success() {
                        self.state.set(ModelHealth::Configured);
                    } else {
                        eprintln!("model: the API answered {status}");
                    }
                    return Ok(answer_response(status, answer));
                }
                Ok((status, _)) => {
                    eprintln!("model: the API answered {status} (attempt {attempt} of {ATTEMPTS})");
                    last = Some(Some(status));
                }
                Err(e) => {
                    // A transport error can quote the URL; the key is never in the URL.
                    eprintln!(
                        "model: the call failed: {} (attempt {attempt} of {ATTEMPTS})",
                        failure_kind(&e)
                    );
                    last = Some(None);
                }
            }
            if attempt < ATTEMPTS {
                tokio::time::sleep(RETRY_PAUSE * attempt).await;
            }
        }
        match last {
            // The upstream's own refusal after the retries: pass it on as it is.
            Some(Some(status)) => Ok(answer_response(status, Bytes::new())),
            _ => Err(CanonicalError::service_unavailable().create()),
        }
    }
}

fn failure_kind(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else {
        "transport"
    }
}

fn answer_response(status: StatusCode, body: Bytes) -> Response<Body> {
    let mut response = Response::new(Body::Bytes(body));
    *response.status_mut() = status;
    response
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-secret-key-0123456789";

    #[test]
    fn a_complete_table_is_read_and_the_url_loses_its_trailing_slash() {
        let text = format!(
            "[model]\nbase_url = \"http://127.0.0.1:9/v1/\"\nmodel = \"gpt-4.1\"\napi_key = \"{KEY}\"\ntimeout_secs = 5\n"
        );
        let config = ModelConfig::parse(&text).unwrap().unwrap();
        assert_eq!(config.base_url, "http://127.0.0.1:9/v1");
        assert_eq!(config.timeout, Duration::from_secs(5));
        // Three attempts of 5 s, the pauses (1.5 s) and the margin.
        assert_eq!(config.call_budget_secs(), 3 * 5 + 1 + 2);
        assert_eq!(config.construct_section()["timeout"], "18s");
        assert_eq!(config.construct_section()["upstream_alias"], UPSTREAM_ALIAS);
    }

    #[test]
    fn a_file_without_a_model_table_means_no_model() {
        assert_eq!(ModelConfig::parse("").unwrap(), None);
    }

    #[test]
    fn bad_values_are_refused() {
        let ok = ("https://api.example.com/v1", "m", KEY);
        assert!(ModelConfig::new(ok.0, ok.1, ok.2, None).is_ok());
        assert!(ModelConfig::new("ftp://x", ok.1, ok.2, None).is_err());
        // A key is never sent in the clear over a network.
        assert!(ModelConfig::new("http://api.example.com/v1", ok.1, ok.2, None).is_err());
        assert!(ModelConfig::new("http://localhost@example.com", ok.1, ok.2, None).is_err());
        assert!(ModelConfig::new("http://127.0.0.1.example.com", ok.1, ok.2, None).is_err());
        assert!(ModelConfig::new("http://localhost:11434/v1", ok.1, ok.2, None).is_ok());
        assert!(ModelConfig::new("http://127.0.0.1:9", ok.1, ok.2, None).is_ok());
        assert!(ModelConfig::new("http://[::1]:9/v1", ok.1, ok.2, None).is_ok());
        assert!(ModelConfig::new("https://", ok.1, ok.2, None).is_err());
        assert!(ModelConfig::new(ok.0, "two words", ok.2, None).is_err());
        assert!(ModelConfig::new(ok.0, "", ok.2, None).is_err());
        assert!(ModelConfig::new(ok.0, ok.1, "  ", None).is_err());
        assert!(ModelConfig::new(ok.0, ok.1, "line\nbreak", None).is_err());
        assert!(ModelConfig::new(ok.0, ok.1, ok.2, Some(0)).is_err());
        assert!(ModelConfig::new(ok.0, ok.1, ok.2, Some(191)).is_err());
    }

    #[test]
    fn the_key_is_in_no_debug_output_and_no_error_text() {
        let config = ModelConfig::new("https://x.example", "m", KEY, None).unwrap();
        assert!(!format!("{config:?}").contains(KEY));
        let gateway = ModelGateway::new(config, ModelState::default()).unwrap();
        assert!(!format!("{gateway:?}").contains(KEY));
        // A broken file whose bad line holds the key: the message does not quote it.
        let err = ModelConfig::parse(&format!("[model]\napi_key = {KEY}\n")).unwrap_err();
        assert!(!err.to_string().contains(KEY), "{err}");
        assert!(!format!("{err:?}").contains(KEY), "{err:?}");
    }

    #[test]
    fn a_file_others_can_read_is_refused() {
        let dir = std::env::temp_dir().join(format!("cb-model-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(CONFIG_FILE);
        let text = format!(
            "[model]\nbase_url = \"https://x.example\"\nmodel = \"m\"\napi_key = \"{KEY}\"\n"
        );
        fs::write(&file, &text).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let err = ModelConfig::load(&file).unwrap_err();
        assert!(matches!(err, ConfigError::Mode(0o644)), "{err}");
        assert!(!err.to_string().contains(KEY));
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(ModelConfig::load(&file).unwrap().is_some());
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn the_budget_of_the_largest_attempt_still_fits_the_limit_of_construct() {
        let config =
            ModelConfig::new("https://x.example", "m", "k", Some(*TIMEOUT_RANGE.end())).unwrap();
        // Construct refuses a model call timeout above 600 seconds.
        assert!(
            config.call_budget_secs() <= 600,
            "{}",
            config.call_budget_secs()
        );
    }

    #[test]
    fn the_budget_covers_every_attempt_and_the_pauses() {
        let config = ModelConfig::new("https://x.example", "m", "k", Some(60)).unwrap();
        let needed = config.timeout * ATTEMPTS + RETRY_PAUSE + RETRY_PAUSE * 2;
        assert!(Duration::from_secs(config.call_budget_secs()) > needed);
    }
}
