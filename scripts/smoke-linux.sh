#!/usr/bin/env bash
# Smoke test of the installed Linux package: the agent answers `status` and the app starts.
# Run as a normal user (PostgreSQL, which the agent starts, refuses to run as root) on a machine
# where the .deb is installed. No systemd is needed: the agent is started in the foreground.
#
# usage: scripts/smoke-linux.sh
# INSTALL_DIR overrides where the package puts the app (default /usr/lib/construct-browser).
set -euo pipefail

install_dir=${INSTALL_DIR:-/usr/lib/construct-browser}
agent="$install_dir/resources/construct-agent"
app=${APP_BIN:-/usr/bin/construct-browser}

work=$(mktemp -d)
agent_pid=
app_pid=
cleanup() {
  [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null || true
  [ -n "$agent_pid" ] && kill "$agent_pid" 2>/dev/null || true
  wait 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

for f in "$agent" "$app"; do
  [ -x "$f" ] || { echo "smoke: $f is missing or not executable" >&2; exit 1; }
done

# The agent keeps its socket in the runtime directory and its data under XDG_DATA_HOME.
export XDG_RUNTIME_DIR="$work/run"
export XDG_DATA_HOME="$work/data"
mkdir -m 0700 "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME"

"$agent" run >"$work/agent.log" 2>&1 &
agent_pid=$!

# `status` answers as soon as the socket is up; it does not wait for the database.
status_ok=0
for _ in $(seq 1 30); do
  if "$agent" status >"$work/status.out" 2>&1; then
    status_ok=1
    break
  fi
  kill -0 "$agent_pid" 2>/dev/null || break
  sleep 1
done
if [ "$status_ok" != 1 ]; then
  echo "smoke: construct-agent status did not report running" >&2
  cat "$work/status.out" "$work/agent.log" >&2 || true
  exit 1
fi
echo "smoke: agent status: $(cat "$work/status.out")"

# The container cannot run the Chromium sandbox. The app registers the agent as a login service
# at start; without systemd that fails, is logged, and the app goes on.
xvfb-run --auto-servernum "$app" --no-sandbox --user-data-dir="$work/app-data" >"$work/app.log" 2>&1 &
app_pid=$!
sleep 15
if ! kill -0 "$app_pid" 2>/dev/null; then
  echo "smoke: the app exited within 15 seconds" >&2
  cat "$work/app.log" >&2 || true
  exit 1
fi
echo "smoke: the app is running"
