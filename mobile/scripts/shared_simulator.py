#!/usr/bin/env python3
"""The one iPhone Simulator that the scripts testing the iPhone app share.

A Simulator that every run created for itself stayed on the disk when a run stopped half way, and each booted
Simulator takes the memory of a phone. All the runs use the Simulator named "Construct Test" instead. It is created
the first time and never deleted. A run

* waits for its turn, so that two runs never use the Simulator at the same time,
* boots it and waits until it answers,
* resets its Keychain, removes the app with its files, installs the app again when the run has a build, and sets the
  light appearance and the normal status bar, so nothing of an earlier run is left,
* and shuts it down when it ends.

The name belongs to this script: another Simulator with it, or one of another device type or runtime, is deleted.

The shell scripts run a command inside this state, and the command gets the Simulator's id in SIMULATOR_UDID:

    python3 scripts/shared_simulator.py --device-type <id> --runtime <id> --bundle-id <id> [--app <Construct.app>] \\
        -- <command> [<argument> ...]
"""

import argparse
import contextlib
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

NAME = "Construct Test"
LOCK = Path.home() / "Library" / "Caches" / "construct-test-simulator.lock"
LOCK_TIMEOUT = 3600
LIST_TIMEOUT = 300
BOOT_TIMEOUT = 600
ANSWER_TIMEOUT = 60
READY_TIMEOUT = 600
STEP_TIMEOUT = 120
SHUTDOWN_TIMEOUT = 120
SIMCTL = ("xcrun", "simctl")


def simctl(*args, timeout=STEP_TIMEOUT, check=True, **options):
    return subprocess.run((*SIMCTL, *args), check=check, timeout=timeout, **options)


def listed():
    """Every Simulator of this name, as (runtime, description), the unavailable ones too."""
    output = simctl("list", "-j", "devices", timeout=LIST_TIMEOUT, capture_output=True, text=True).stdout
    return [(runtime, device) for runtime, devices in json.loads(output)["devices"].items() for device in devices
            if device["name"] == NAME]


def state(udid):
    return next((device["state"] for _, device in listed() if device["udid"] == udid), None)


def shut_down(udid):
    """Shuts a Simulator down and makes sure it is down: a Simulator that is down already is not an error."""
    simctl("shutdown", udid, timeout=SHUTDOWN_TIMEOUT, check=False, stderr=subprocess.DEVNULL)
    now = state(udid)
    if now not in (None, "Shutdown"):
        raise RuntimeError(f"The {NAME} Simulator is still {now}.")


def find_or_create(device_type, runtime):
    """The id of the Simulator for this device type and runtime. Any other Simulator of the name is deleted."""
    found = None
    for runtime_id, device in listed():
        if found is None and runtime_id == runtime and device["deviceTypeIdentifier"] == device_type \
                and device["isAvailable"]:
            found = device["udid"]
            continue
        print(f"Deleting a {NAME} Simulator that is not the one to use ({device['udid']}).", file=sys.stderr)
        simctl("shutdown", device["udid"], timeout=SHUTDOWN_TIMEOUT, check=False, stderr=subprocess.DEVNULL)
        simctl("delete", device["udid"])
    if found is None:
        found = simctl("create", NAME, device_type, runtime, capture_output=True, text=True).stdout.strip()
        print(f"Created the {NAME} Simulator.", file=sys.stderr)
    return found


@contextlib.contextmanager
def alone():
    """Holds the lock of the Simulator until the block ends. The lock goes with the process, so a run that died
    leaves none behind."""
    LOCK.parent.mkdir(parents=True, exist_ok=True)
    with LOCK.open("a") as handle:
        deadline = time.monotonic() + LOCK_TIMEOUT
        waiting = False
        while True:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() > deadline:
                    raise RuntimeError(f"Another run has had the {NAME} Simulator for {LOCK_TIMEOUT // 60} minutes.")
                if not waiting:
                    print(f"Another run is using the {NAME} Simulator; waiting for it.", file=sys.stderr)
                    waiting = True
                time.sleep(5)
        yield


def bundle_id_of(app):
    return subprocess.run(("plutil", "-extract", "CFBundleIdentifier", "raw", "-o", "-", str(Path(app) / "Info.plist")),
                          check=True, capture_output=True, text=True, timeout=30).stdout.strip()


def prepare(udid, bundle_id, app, theme, certificate):
    """Boots the Simulator and leaves it as new: nothing of the app, no Keychain item, the app installed again."""
    simctl("bootstatus", udid, "-b", timeout=BOOT_TIMEOUT)
    # A booted Simulator can take minutes to answer on a busy Mac: ask until it does, then go on.
    deadline = time.monotonic() + READY_TIMEOUT
    while True:
        try:
            simctl("spawn", udid, "launchctl", "list", timeout=ANSWER_TIMEOUT, stdout=subprocess.DEVNULL)
            simctl("keychain", udid, "reset")
            simctl("uninstall", udid, bundle_id, check=False, stderr=subprocess.DEVNULL)
            # The container of the app is the app's files; it is gone when the app is.
            if simctl("get_app_container", udid, bundle_id, "data", check=False, stdout=subprocess.DEVNULL,
                      stderr=subprocess.DEVNULL).returncode == 0:
                raise RuntimeError("The app is still installed after it was removed.")
            if app:
                simctl("install", udid, str(app), timeout=READY_TIMEOUT)
            simctl("ui", udid, "appearance", theme)
            simctl("status_bar", udid, "clear")
            if certificate:
                simctl("keychain", udid, "add-root-cert", str(certificate))
            return
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            if time.monotonic() > deadline:
                raise
            time.sleep(5)


@contextlib.contextmanager
def shared_simulator(device_type, runtime, bundle_id=None, app=None, theme="light", certificate=None):
    """Yields the id of the Simulator, booted and as new, and shuts it down when the block ends.

    `app` is the Construct.app to install, `bundle_id` its identifier (read from the app when it is not given),
    `theme` the appearance (light or dark) and `certificate` a PEM file to put into the trust store of the
    Simulator after the Keychain was reset.
    """
    bundle_id = bundle_id or bundle_id_of(app)
    failed = False
    with alone():
        udid = find_or_create(device_type, runtime)
        try:
            prepare(udid, bundle_id, app, theme, certificate)
            yield udid
        except BaseException:
            failed = True
            raise
        finally:
            try:
                shut_down(udid)
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                if not failed:
                    raise
                print(f"The {NAME} Simulator could not be shut down: {error}", file=sys.stderr)


def interrupted(number, frame):
    del frame
    raise SystemExit(128 + number)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--device-type", required=True, help="simctl device type identifier")
    parser.add_argument("--runtime", required=True, help="simctl runtime identifier")
    parser.add_argument("--bundle-id", required=True, help="bundle identifier of the app")
    parser.add_argument("--app", type=Path, help="the Construct.app to install")
    parser.add_argument("command", nargs="+", help="the command to run in the Simulator, after --")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    with shared_simulator(args.device_type, args.runtime, args.bundle_id, args.app) as udid:
        return subprocess.run(args.command, env={**os.environ, "SIMULATOR_UDID": udid}, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
