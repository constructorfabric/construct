//! The model route of the host: a small gateway of our own that stands in for OAGW.
//!
//! Construct looks up `dyn ServiceGatewayClientV1` in its client hub for every model call and
//! posts the planner's chat-completions request to `/{upstream_alias}`. OAGW 0.7.0 needs a
//! credential store and a tenant resolver at start, which the host does not have, so the host
//! registers [`ModelGateway`] instead. It forwards the request to the user's OpenAI-compatible
//! API (`{base_url}/chat/completions`) with the user's key.
//!
//! The address, the model name and the key come from the settings store on every call, so a
//! change made in Settings takes effect on the next call, without a restart. The model name
//! of the request body is replaced by the saved one, since Construct is configured with the
//! name it had at start. The key is in `settings.json` (user-only), in the settings in memory, and
//! in the header of a call in progress. It is in no `Debug` output, error, log line or the
//! agent's `status`.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode};
use oagw_sdk::{
    Body, CreateRouteRequest, CreateUpstreamRequest, ListQuery, Route, ServiceGatewayClientV1,
    UpdateRouteRequest, UpdateUpstreamRequest, Upstream,
};
use toolkit_canonical_errors::CanonicalError;
use toolkit_security::SecurityContext;
use uuid::Uuid;

use crate::settings::Settings;
pub use chat_core::model::ANSWER_LIMIT;
pub(crate) use chat_core::model::Target;
use chat_core::model::{Model, read_capped, refuses_tools_with_reasoning, without_reasoning};

/// The upstream alias Construct is told to call. Its own gateway is the only one that answers it.
pub const UPSTREAM_ALIAS: &str = "model";
/// The model name Construct is started with. The gateway replaces it with the saved one.
const START_MODEL: &str = "model";
/// The limit for ONE attempt.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(60);
/// Attempts for one call: the first and the retries after a failure that may pass.
const ATTEMPTS: u32 = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(500);

/// The limit Construct puts on the whole model call: it wraps `proxy_request`, which makes
/// up to [`ATTEMPTS`] attempts of [`ATTEMPT_TIMEOUT`] each with the pauses between them. Giving
/// Construct the per-attempt number would cut the call off after the first attempt.
#[must_use]
pub fn call_budget_secs() -> u64 {
    let pauses: Duration = (1..ATTEMPTS).map(|n| RETRY_PAUSE * n).sum();
    // The pauses rounded up, and a second for the work around the attempts.
    ATTEMPT_TIMEOUT.as_secs() * u64::from(ATTEMPTS) + pauses.as_secs() + 2
}

/// The Construct `model` section. The host has it whenever the settings open: the model itself
/// is read from the settings on every call, so the host must be ready for one that is set later.
#[must_use]
pub fn construct_section() -> serde_json::Value {
    serde_json::json!({
        "upstream_alias": UPSTREAM_ALIAS,
        "model": START_MODEL,
        "timeout": format!("{}s", call_budget_secs()),
    })
}

/// What the agent knows of its model, shown in `status` as `model_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelHealth {
    /// The settings do not hold a complete model: address, name and key.
    NotConfigured,
    /// A model is set and the last call was not refused for its key.
    Configured,
    /// The API refused the key (`401` or `403`) on the last call. It clears as soon as another
    /// key or address is saved, and on the next accepted call.
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

/// The outcome of the calls, shared between the gateway and the socket threads. What `status`
/// shows is [`ModelState::health`], which also looks at the settings.
#[derive(Clone, Debug, Default)]
pub struct ModelState(Arc<Mutex<StateInner>>);

#[derive(Debug, Default)]
struct StateInner {
    /// The address and the key (a sensitive header value, which `Debug` hides) the API refused
    /// last; `None` after a call that was accepted.
    rejected: Option<(String, HeaderValue)>,
}

impl ModelState {
    fn lock(&self) -> std::sync::MutexGuard<'_, StateInner> {
        crate::sync::lock(&self.0)
    }

    /// The state for `status`, from the saved model and the last refusal. A refusal counts
    /// only while the saved key and address are the ones that were refused.
    #[must_use]
    pub fn health(&self, model: &Model) -> ModelHealth {
        let Some(target) = Target::from_settings(model) else {
            return ModelHealth::NotConfigured;
        };
        match &self.lock().rejected {
            Some((url, key)) if *url == target.base_url() && *key == target.authorization => {
                ModelHealth::KeyRejected
            }
            _ => ModelHealth::Configured,
        }
    }

    fn reject(&self, target: &Target) {
        self.lock().rejected = Some((target.base_url().to_owned(), target.authorization.clone()));
    }

    fn accept(&self) {
        self.lock().rejected = None;
    }
}

/// The gateway Construct calls for its model. Only `proxy_request` does anything.
pub struct ModelGateway {
    settings: Settings,
    client: reqwest::Client,
    state: ModelState,
}

impl fmt::Debug for ModelGateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelGateway").finish_non_exhaustive()
    }
}

impl ModelGateway {
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn new(settings: Settings, state: ModelState) -> Result<Self, reqwest::Error> {
        Self::with_attempt_timeout(settings, state, ATTEMPT_TIMEOUT)
    }

    /// As [`ModelGateway::new`], with `timeout` as the limit for one attempt; for the tests,
    /// since the host's limit is [`ATTEMPT_TIMEOUT`].
    ///
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn with_attempt_timeout(
        settings: Settings,
        state: ModelState,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            settings,
            client,
            state,
        })
    }

    /// One attempt: the answer's status and body, or why there is none.
    async fn attempt(
        &self,
        target: &Target,
        body: &Bytes,
    ) -> Result<(StatusCode, Bytes), reqwest::Error> {
        let response = target
            .request(&self.client, "application/json", body.clone())
            .send()
            .await?;
        let status = response.status();
        let answer = Bytes::from(read_capped(response, ANSWER_LIMIT).await?);
        Ok((status, answer))
    }
}

/// `body` with its `model` set to `model`. A body that is not a JSON object goes on as it is.
fn with_model(body: Bytes, model: &str) -> Bytes {
    match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(serde_json::Value::Object(mut map)) => {
            map.insert("model".into(), model.into());
            serde_json::to_vec(&map).map_or(body, Bytes::from)
        }
        _ => body,
    }
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
        // The settings are read for every call: a change in Settings is used by the next one.
        let Some(target) = Target::from_settings(&self.settings.model()) else {
            eprintln!("model: not configured, set the address, the model and the key in Settings");
            return Err(CanonicalError::service_unavailable().create());
        };
        let mut body = with_model(body, &target.model);
        if let Some(plain) = target.prepared(&body) {
            body = Bytes::from(plain);
        }

        let mut last = None;
        let mut retried_without_reasoning = false;
        for attempt in 1..=ATTEMPTS {
            match self.attempt(&target, &body).await {
                Ok((status, answer)) if !is_transient(status) => {
                    // The body is only searched here, and the request goes again without it.
                    if !retried_without_reasoning
                        && refuses_tools_with_reasoning(status.as_u16(), &answer)
                        && let Some(plain) = without_reasoning(&body)
                    {
                        target.refused_tools_with_reasoning();
                        body = Bytes::from(plain);
                        retried_without_reasoning = true;
                        continue;
                    }
                    // The status only: the body of an error may quote the request.
                    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                        // Not retried: the key stays as wrong as it was. The user is told in
                        // `status` and in the log, and saves another key in Settings.
                        self.state.reject(&target);
                        eprintln!(
                            "model: the API rejected the key ({status}); save a working key in Settings"
                        );
                    } else if status.is_success() {
                        self.state.accept();
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

    #[test]
    fn the_key_is_in_no_debug_output() {
        let model = Model {
            base_url: "https://x.example/v1".into(),
            model: "m".into(),
            api_key: Some("sk-top-secret".into()),
        };
        let target = Target::from_settings(&model).unwrap();
        let state = ModelState::default();
        state.reject(&target);
        assert_eq!(state.health(&model), ModelHealth::KeyRejected);
        assert!(!format!("{state:?}").contains("sk-top-secret"));
        assert!(!format!("{:?}", state.lock()).contains("sk-top-secret"));
    }
}
