#!/usr/bin/env python3
"""Runs the Glasshouse page tests of the Android app on a disposable emulator, against the test server on this computer.

Build the app and its tests first, so that the build has made the test certificates:

    ./gradlew :androidApp:assembleDebug :androidApp:assembleDebugAndroidTest

then start an emulator and run:

    python3 scripts/test-glasshouse.py --device emulator-5554 [--screenshots <folder>]

The script starts `glasshouse_test_server.py`, installs both APKs, runs the page tests, runs the restart test (sign in,
stop the app's process, check the sign-in is still there) and, with --screenshots, takes a screenshot of each state of the
page in the light and the dark theme. The tests turn the emulator's network off and on, so use an emulator that is
yours alone. The exit status is 0 when every test passed.
"""

import argparse
import contextlib
import json
from pathlib import Path
import re
import subprocess
import sys

MOBILE = Path(__file__).resolve().parents[1]
APK = MOBILE / "androidApp/build/outputs/apk/debug/androidApp-debug.apk"
TEST_APK = MOBILE / "androidApp/build/outputs/apk/androidTest/debug/androidApp-debug-androidTest.apk"
CERTIFICATES = MOBILE / "androidApp/build/generated/glasshouseTest/server"
SERVER = Path(__file__).resolve().parent / "glasshouse_test_server.py"
PACKAGE = "cf.construct.mobile.android"
RUNNER = "cf.construct.mobile.test/androidx.test.runner.AndroidJUnitRunner"
RUN_TIMEOUT = 2400
STATES = ("nostart", "refused", "ready", "typing", "loading", "notfound", "certificate", "connection-refused",
          "stopped", "offline")


def application_id():
    properties = (MOBILE / "gradle.properties").read_text().splitlines()
    return next(line.split("=", 1)[1] for line in properties if line.startswith("constructApplicationId="))


class Emulator:
    def __init__(self, device):
        if not device.startswith("emulator-"):
            raise SystemExit("Use a disposable Android emulator: the tests turn its network off.")
        self.adb = ("adb", "-s", device)
        self.app = application_id()

    def run(self, *args, check=True, timeout=120):
        return subprocess.run((*self.adb, *args), check=check, capture_output=True, text=True, timeout=timeout)

    def instrument(self, test, **arguments):
        extra = [item for key, value in arguments.items() for item in ("-e", key, value)]
        result = self.run("shell", "am", "instrument", "-w", "-e", "class", f"{PACKAGE}.{test}", *extra, RUNNER,
                          check=False, timeout=RUN_TIMEOUT)
        print(result.stdout)
        print(result.stderr, end="")
        # A test that skips itself still counts as run, so say what the run has to hold: tests, and no failure.
        ran = re.search(r"^OK \((\d+) tests?\)", result.stdout, re.MULTILINE)
        return result.returncode == 0 and ran is not None and int(ran.group(1)) > 0 and "FAILURES" not in result.stdout

    def theme(self, name):
        self.run("shell", "cmd", "uimode", "night", "yes" if name == "dark" else "no")

    def demo(self, on):
        if on:
            self.run("shell", "settings", "put", "global", "sysui_demo_allowed", "1")
            for command in (("enter",), ("clock", "-e", "hhmm", "1200"), ("battery", "-e", "level", "100", "-e", "plugged", "false"),
                            ("network", "-e", "wifi", "show", "-e", "level", "4"), ("notifications", "-e", "visible", "false")):
                self.run("shell", "am", "broadcast", "-a", "com.android.systemui.demo", "-e", "command", *command)
        else:
            self.run("shell", "am", "broadcast", "-a", "com.android.systemui.demo", "-e", "command", "exit", check=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--device", required=True, help="the serial of the emulator, such as emulator-5554")
    parser.add_argument("--screenshots", type=Path, help="a folder to save a screenshot of each state in both themes")
    parser.add_argument("--screenshots-only", action="store_true", help="take the screenshots and run no test (needs --screenshots)")
    args = parser.parse_args()
    if args.screenshots_only and not args.screenshots:
        parser.error("--screenshots-only needs --screenshots")
    for path in (APK, TEST_APK, CERTIFICATES / "trusted.pem"):
        if not path.exists():
            raise SystemExit(f"{path} is missing: build the app and its tests first (see --help).")

    emulator = Emulator(args.device)
    server = subprocess.Popen([sys.executable, str(SERVER), "--certificates", str(CERTIFICATES)],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    assert server.stdin is not None and server.stdout is not None
    try:
        sites = json.loads(server.stdout.readline())
        # `glasshouseRunner` makes a test that finds no server fail, where on its own it would skip and print OK.
        arguments = {"glasshouseRunner": "1", "glasshouseTrusted": sites["trusted"], "glasshouseUntrusted": sites["untrusted"],
                     "glasshousePlain": sites["plain"], "glasshouseFlaky": sites["flaky"]}
        emulator.run("install", "-r", "-t", str(APK))
        emulator.run("install", "-r", "-t", str(TEST_APK))
        passed = True
        if not args.screenshots_only:
            passed = emulator.instrument("GlasshouseTest", **arguments)

            emulator.run("shell", "pm", "clear", emulator.app)
            passed &= emulator.instrument("GlasshouseRestartTest", glasshouseRestart="signin", **arguments)
            emulator.run("shell", "am", "force-stop", emulator.app)
            passed &= emulator.instrument("GlasshouseRestartTest", glasshouseRestart="check", **arguments)

        if args.screenshots:
            args.screenshots.mkdir(parents=True, exist_ok=True)
            folder = f"/sdcard/Android/data/{emulator.app}/files/glasshouse-shots"
            emulator.demo(True)
            try:
                for theme in ("light", "dark"):
                    emulator.theme(theme)
                    emulator.run("shell", "rm", "-rf", folder)
                    passed &= emulator.instrument("GlasshouseShotsTest", glasshouseTheme=theme, **arguments)
                    for state in STATES:
                        emulator.run("pull", f"{folder}/{theme}-{state}.png", str(args.screenshots / f"android-{theme}-{state}.png"),
                                     check=False)
            finally:
                emulator.demo(False)
                emulator.run("shell", "cmd", "uimode", "night", "auto", check=False)
        print("PASSED" if passed else "FAILED")
        return 0 if passed else 1
    finally:
        with contextlib.suppress(Exception):
            emulator.run("shell", "svc", "wifi", "enable", check=False)
            emulator.run("shell", "svc", "data", "enable", check=False)
        server.stdin.close()
        server.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())
