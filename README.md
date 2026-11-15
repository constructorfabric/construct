# Construct Browser

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

**Construct Browser is a desktop app that runs your personal Construct on your own computer, under your control.**

It is part of [Constructor Fabric](http://www.constructorfabric.com/). The Construct gear of
[Constructor Fabric Gears](https://github.com/constructorfabric/gears-rust) builds a living profile of a
person from the tools they already use. Construct Browser is where that gear runs for one person: an
Electron window you open and close, and a Rust agent that keeps working in the background. The agent, the
data it collects and the settings stay on your machine. The only service the agent calls is the model you
configure yourself.

> **Status:** early. The app is not released yet, and there are no downloads.

## What it does today

- **Reads your coding tools.** The agent follows the session logs of Claude Code and Codex and turns every
  prompt and answer into a record, within seconds. Log files are only opened for reading.
- **Keeps the records on your computer.** The agent stores the records as plain files in its data directory.
- **Runs the Construct gear.** On Linux x86_64 the agent starts the Construct gear inside itself, next to an
  embedded PostgreSQL database with pgvector that serves graph storage. Nothing listens beyond your machine:
  the agent socket and the database socket are open to your user only, and the gear's REST interface is on the
  loopback address and needs a token that only your user can read.
- **Keeps working after you close the window.** The installed app registers the agent to start at login, as a
  systemd user service on Linux and a LaunchAgent on macOS.
- **Shows what the agent is doing.** The Status page shows whether the agent is running, its version, its
  process id, the model in use and whether it starts at login.
- **Talks to the model you choose.** The Settings page takes the address, the name and the API key of a model,
  can test the call, and can remove the key. The key is never written to a log or shown in the status. A
  change applies to the next call, with no restart.
- **Chat.** The Chat page streams the model's answers as Markdown, lets you stop an answer, and keeps your
  conversations on your computer.

The window follows the light or dark theme of your system.

## What works on each system

| Part                                                         | Linux x86_64         | macOS (Apple Silicon)                       | Windows                                                                |
| ------------------------------------------------------------ | -------------------- | ------------------------------------------- | ---------------------------------------------------------------------- |
| Window, Status, Settings and Chat pages                      | yes                  | yes                                         | the window opens; Status says `unknown`, Settings says "not available" |
| Agent socket and `status`                                    | yes                  | yes                                         | no: the agent stops with "not implemented on this platform"            |
| Start at login                                               | systemd user service | LaunchAgent                                 | no: "not implemented on Windows yet"                                   |
| Log readers (Claude Code, Codex), settings, model test, chat | yes                  | yes (no database needed)                    | no: the agent does not start                                           |
| Embedded PostgreSQL with pgvector                            | yes                  | no: pgvector is built for Linux x86_64 only | no                                                                     |
| Construct gear                                               | yes                  | no: it needs the database                   | no                                                                     |

`make smoke` installs the Linux package in a clean Ubuntu 24.04 container and starts it there. Tested on Linux x86_64 (Ubuntu 24.04).

## Build and test

You need Rust (the toolchain in `rust-toolchain.toml` is installed by `rustup`), Node.js 22.13 or newer (22 or 24) with npm, and
`protoc` with the protobuf headers (Debian and Ubuntu: `protobuf-compiler` and `libprotobuf-dev`; macOS:
`brew install protobuf`). On Linux the build also needs `curl`, `tar`, `make` and `gcc`, which build pgvector,
and `make test` needs `xvfb-run` for the Electron window, so it runs on Linux. `make smoke` and the Linux package (`npm run make`) also need `dpkg` and `fakeroot`.

```bash
make build    # build the agent: target/release/construct-agent
make test     # format check, clippy, agent tests, then the app tests and the Electron test
make lint     # the app's lint and type check
make run      # build the agent and start the app in development mode
make package  # package the app with the agent inside
make smoke    # build the .deb, install it in a clean Ubuntu 24.04 container and start it (needs Docker)
```

`make help` lists the targets. `make test` runs the Electron test under a virtual display; it opens the
window, takes a screenshot of each page in both themes and fails on any content security policy violation.
`make smoke` checks that the installed agent answers `status` with `running` and that the app is still alive
after 15 seconds.

## Install from source

There are no downloads yet, so you build the app yourself. First install the tools from
[Build and test](#build-and-test), then follow the steps for your system. All commands run in the root of this
repository.

### Linux (Ubuntu and Debian, x86_64)

```bash
make build app-deps
(cd app && npm run make)
sudo apt install ./app/out/make/deb/x64/*.deb
```

Start **Construct Browser** from your application menu, or run `construct-browser`. The installed app
registers the agent to start at login. `make smoke` builds the package and installs it in a clean container.

### macOS (Apple Silicon)

The build makes an app bundle, and you turn it into a disk image and sign it yourself. The signature is
ad-hoc: it is not tied to a developer account, so it only has to be good enough for your own Mac.

```bash
make package
APP="app/out/Construct Browser-darwin-arm64/Construct Browser.app"
codesign --force --deep --sign - "$APP"

# A disk image with the app and a shortcut to the Applications folder.
mkdir -p app/out/dmg && cp -R "$APP" app/out/dmg/ && ln -sf /Applications app/out/dmg/Applications
hdiutil create -volname "Construct Browser" -srcfolder app/out/dmg -ov -format UDZO "app/out/Construct Browser.dmg"
```

Open `app/out/Construct Browser.dmg` and drag **Construct Browser** onto **Applications**.

On macOS the agent runs without the database and the Construct gear (see the table above).

### Windows

Windows has no installer from source yet, and the agent does not run there. You can start the window in
development mode, which shows the Status page as `unknown`. With Node.js installed, run in PowerShell:

```powershell
cd app
npm ci
node node_modules/electron/install.js
npm start
```

## Contributing and license

Contribution rules and how to report a security issue are in the
[Constructor Fabric governance repository](https://github.com/constructorfabric/governance), starting with
[CONTRIBUTING.md](https://github.com/constructorfabric/governance/blob/main/CONTRIBUTING.md). Commits are
signed off, as the [DCO](https://github.com/constructorfabric/governance/blob/main/DCO.md) asks.

Construct Browser is licensed under the Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
