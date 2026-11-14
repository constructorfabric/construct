# Construct Browser

Construct Browser is a desktop app made of an Electron shell and a Rust agent
that runs in the background. It is part of
[Constructor Fabric](https://github.com/constructorfabric/governance).

> **Status:** work in progress. Nothing here is ready for use yet.

## Building the agent

On Linux and macOS the build needs `protoc` and the protobuf headers (Debian and Ubuntu:
`protobuf-compiler` and `libprotobuf-dev`; macOS: `brew install protobuf`), because the toolkit
compiles a `.proto` file. `make test` runs the format check, clippy and the tests.

See the [governance repository](https://github.com/constructorfabric/governance)
for the project's license, contribution rules and community documents.
