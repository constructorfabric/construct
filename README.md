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
- **Builds your profile from what you type.** Every new prompt in Claude Code or Codex goes to the
  Construct gear with the ten turns before it as context, and the gear turns it into facts about you.
  Prompts from before the first start go out only when you ask for them (see
  [Your profile from past prompts](#your-profile-from-past-prompts)).
- **Keeps working after you close the window.** The installed app registers the agent to start at login, as a
  systemd user service on Linux and a LaunchAgent on macOS.
- **Shows what the agent is doing.** The Status page shows whether the agent is running, its version, its
  process id, the model in use (`not set` until the address, the name and the key are all saved, and again
  after the key is removed) and whether it starts at login.
- **Talks to the model you choose.** The Settings page takes the address, the name and the API key of a model,
  can test the call, and can remove the key. The key is never written to a log or shown in the status. A
  change applies to the next call, with no restart.
- **Chat.** The Chat page streams the model's answers as Markdown, lets you stop an answer, and keeps your
  conversations on your computer.

The window follows the light or dark theme of your system.

## Your profile from past prompts

The agent sends every prompt you type after its first start to the Construct gear by itself. The history it
found in the logs of the last 30 days is not sent by itself, because every prompt costs one model call.
`backfill` sends it when you ask, oldest first, headless sessions included:

```bash
construct-agent backfill --dry-run     # count the prompts per tool, send nothing
construct-agent backfill --limit 50    # a small first run: the 50 oldest prompts
construct-agent backfill               # everything that is left (--days N changes the 30)
construct-agent status                 # the "facts" part shows the progress
```

The agent remembers which prompts it sent, so a prompt is never sent twice: not by a second `backfill`, not
after a restart or a crash, and not when a prompt is both in the history and new. A prompt goes out with the
turns before it in the same session, cut to the limits of the gear (4,000 characters for the prompt, 16,000 for
the context, the oldest turns first). Prompts go one at a time, and a new prompt goes before the rest of a
`backfill`. A `backfill` stops with its reason in `status` when the gear refuses it or the model does not answer.
Prompt text is never written to a log or to the status.

## Size and memory

Measured on Linux x86_64 with the release build (`make build`): the agent is one 118 MB file, with the
database inside it. With the database and the gear ready and nothing to do, the agent holds about 57 MB and
PostgreSQL about 35 MB (proportional set size, so shared memory counts once), 92 MB together. The first start,
which creates the database, takes about 100 MB for the agent.

## Where your data lives and who can read it

The records, the profile and the database are plain files under your user's data directory
(`~/.local/share/construct` on Linux), readable by your user only. The agent does not encrypt them: it relies on
the disk encryption of your operating system (FileVault on macOS, BitLocker on Windows, LUKS on Linux). Turn it
on if the computer can be lost or shared.

## What works on each system

| Part                                                         | Linux x86_64         | macOS (Apple Silicon)    | Windows                                                                |
| ------------------------------------------------------------ | -------------------- | ------------------------ | ---------------------------------------------------------------------- |
| Window, Status, Settings and Chat pages                      | yes                  | yes                      | the window opens; Status says `unknown`, Settings says "not available" |
| Agent socket and `status`                                    | yes                  | yes                      | no: the agent stops with "not implemented on this platform"            |
| Start at login                                               | systemd user service | LaunchAgent              | no: "not implemented on Windows yet"                                   |
| Log readers (Claude Code, Codex), settings, model test, chat | yes                  | yes (no database needed) | no: the agent does not start                                           |
| Embedded PostgreSQL with pgvector                            | yes                  | yes                      | no                                                                     |
| Construct gear                                               | yes                  | yes                      | no                                                                     |

`make smoke` installs the Linux package in a clean Ubuntu 24.04 container and starts it there. Tested on Linux x86_64 (Ubuntu 24.04).

## Build and test

You need Rust (the toolchain in `rust-toolchain.toml` is installed by `rustup`), Node.js 22.13 or newer (22 or 24) with npm, and
`protoc` with the protobuf headers (Debian and Ubuntu: `protobuf-compiler` and `libprotobuf-dev`; macOS:
`brew install protobuf`). The agent build (`make build`, and so `make package`) also builds pgvector against the
bundled PostgreSQL; for that it needs `curl` and `tar`, and on Linux `make` and `gcc`, on macOS the Xcode command
line tools for `make` and `clang` (`xcode-select --install`). `make test` needs `xvfb-run` for the Electron window, so it runs on Linux. `make smoke` and the Linux package (`npm run make`) also need `dpkg` and `fakeroot`.

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
