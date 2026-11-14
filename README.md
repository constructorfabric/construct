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
The address, the model name and the key come from the settings store (`settings.json`, saved by the
Settings page or `settings-set`). The gateway reads them on every call, so a change takes effect on the
next call, without a restart. There is no other source.

- Whenever the settings open, the host is set up for a model (if the settings file cannot be opened, the
  host has none and drops records), and the model name of each request is replaced by the saved one.
  A call while the address, the name or the key is missing makes no request and fails as unavailable.
- A failed call (connect error, 429, 5xx) is tried up to three times, each with a 60 s limit (Construct is
  given the time of all three). A rejected key (`401` or `403`) is not retried, since it stays wrong: the
  record is dropped, the host stays up, and `status` shows `"model_state":"key_rejected"` until another
  key or address is saved.
- `status` shows `"model"`: the saved model name, or `not set`; and `"model_state"`: `not_configured`
  (the settings do not hold an address, a name and a key), `configured` or `key_rejected`.
- The key is in no log line, no error text and not in `status`.

See the [governance repository](https://github.com/constructorfabric/governance)
for the project's license, contribution rules and community documents.
