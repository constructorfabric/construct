# Construct Browser

Construct Browser is a desktop app made of an Electron shell and a Rust agent
that runs in the background. It is part of
[Constructor Fabric](https://github.com/constructorfabric/governance).

> **Status:** work in progress. Nothing here is ready for use yet.

## Building the agent

On Linux and macOS the build needs `protoc` and the protobuf headers (Debian and Ubuntu:
`protobuf-compiler` and `libprotobuf-dev`; macOS: `brew install protobuf`), because the toolkit
compiles a `.proto` file. `make test` runs the format check, clippy and the tests.

## Model

The planner of the Construct host calls a model through a small gateway of the agent's own
(`agent/src/model.rs`), not through OAGW, which needs a credential store and a tenant resolver at start.
Base URL, model and key come from `$XDG_CONFIG_HOME/construct/agent.toml` (default
`~/.config/construct/agent.toml`), which must be readable by the user only (`chmod 600`); the agent
does not use a file that others can read.

```toml
[model]
base_url = "https://api.openai.com/v1"   # any OpenAI-compatible API; /chat/completions is added;
                                           # http:// is accepted for localhost only
model = "gpt-4.1"
api_key = "sk-..."
timeout_secs = 60                          # optional, 1 to 190: the limit for one attempt
```

- Without the file (or without a `[model]` table) the host runs without a model: it takes records and drops
  them, and the agent prints `model: not configured`. A file that is refused is named in the log, with
  the reason and never its content.
- With it the host also gets `processing_identity` and `processing.run_without_sensitive_data_checks: true`
  (no sensitive-data check exists yet), so a chat-message record gives facts in the profile.
- A failed call (connect error, 429, 5xx) is tried up to three times, each with the `timeout_secs` limit (Construct is given the time of all three). A rejected key (`401` or `403`) is not
  retried, since it stays wrong: the record is dropped, the host stays up, `status` shows
  `"model":"key_rejected"` and the log says to put a working key in `agent.toml` and restart the agent.
  `status` also shows `not_configured` and `configured`.
- The key is in no log line, no error text and not in `status`. The agent reads the file at start; restart
  it after a change.

See the [governance repository](https://github.com/constructorfabric/governance)
for the project's license, contribution rules and community documents.
