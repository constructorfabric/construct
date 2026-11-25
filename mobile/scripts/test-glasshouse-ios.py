#!/usr/bin/env python3
"""Runs the Glasshouse page tests of the iPhone app in the shared Simulator, against the test server on this Mac.

Build the app and its tests once, signed ad hoc (the Simulator of iOS 27 refuses an unsigned app its Keychain):

    cd iosApp && xcodegen generate && cd ..
    xcodebuild -project iosApp/Construct.xcodeproj -scheme Construct -derivedDataPath <build folder> \\
        -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- build-for-testing

then run:

    python3 scripts/test-glasshouse-ios.py --ios-build <build folder> \\
        --ios-device-type com.apple.CoreSimulator.SimDeviceType.iPhone-17 \\
        --ios-runtime com.apple.CoreSimulator.SimRuntime.iOS-27-2 [--screenshots <folder>]

The script makes two certificates with `openssl`. It uses the Simulator named "Construct Test" (shared_simulator.py):
it resets the Simulator's Keychain, installs the app again, puts the first certificate into the Simulator's trust
store (`simctl keychain add-root-cert`), so the page tests load pages over https from this Mac, and shuts the Simulator
down at the end. The second certificate is trusted by nothing and stands for a site that cannot be trusted. Nothing
about the certificates is in the app. The script starts `glasshouse_test_server.py`, runs the page tests
(`ConstructUITests/GlasshouseTests`) and, with --screenshots, runs the screenshot test in the light and the dark
theme and saves a picture of each state of the page. It also answers the test's one request that only a Mac can do:
stopping the web process of the page. The exit status is 0 when every test passed.
"""

import argparse
import contextlib
import json
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import shared_simulator  # noqa: E402

MOBILE = Path(__file__).resolve().parents[1]
SERVER = Path(__file__).resolve().parent / "glasshouse_test_server.py"
TESTS = "ConstructUITests/GlasshouseTests"
SHOTS_TEST = "ConstructUITests/GlasshouseShotsTests/testEveryStateForTheScreenshots"
RUN_TIMEOUT = 1800
STATES = ("nostart", "ready", "refused", "typing", "loading", "notfound", "certificate", "connection-refused", "stopped",
          "offline")


def read(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=120).stdout.strip()


def app_version():
    text = (MOBILE / "iosApp/project.yml").read_text()
    found = re.search(r'MARKETING_VERSION:\s*"?([^"\s]+)"?', text)
    if found is None:
        raise RuntimeError("project.yml has no MARKETING_VERSION.")
    return found.group(1)


def make_certificates(folder):
    """A certificate the Simulator will be told to trust, and one nothing trusts. Both are for 127.0.0.1."""
    for name in ("trusted", "untrusted"):
        subprocess.run(
            ("openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes",
             "-keyout", str(folder / f"{name}-key.pem"), "-out", str(folder / f"{name}.pem"), "-days", "30",
             "-subj", f"/CN=Garden {name} test", "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost",
             "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,digitalSignature",
             "-addext", "extendedKeyUsage=serverAuth"),
            check=True, capture_output=True, timeout=60)


@contextlib.contextmanager
def test_simulator(device_type, runtime, build, certificate):
    apps = list((Path(build) / "Build" / "Products").glob("*-iphonesimulator/Construct.app"))
    if len(apps) != 1:
        raise RuntimeError("Use a build-for-testing folder with one Construct Simulator app.")
    if "__entitlements" not in read("otool", "-l", str(apps[0] / "Construct")):
        raise RuntimeError("Build the app with CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- (see --help).")
    with shared_simulator.shared_simulator(device_type, runtime, app=apps[0], certificate=certificate) as device:
        yield device


def stop_web_processes(device):
    """Kills the web content processes of this Simulator, as a crash of the page's renderer does.

    The Simulator's processes run on the Mac under the `launchd_sim` that was started for the device, so those are the
    web processes below it, and no other Simulator's and not the Mac's own.
    """
    table = []
    for line in read("ps", "-axo", "pid=,ppid=,command=").splitlines():
        fields = line.split(None, 2)
        if len(fields) == 3 and fields[0].isdigit() and fields[1].isdigit():
            table.append((int(fields[0]), int(fields[1]), fields[2]))
    roots = {pid for pid, _, command in table if "launchd_sim" in command and device in command}
    below = set(roots)
    grown = True
    while grown:
        grown = False
        for pid, parent, _ in table:
            if parent in below and pid not in below:
                below.add(pid)
                grown = True
    killed = 0
    for pid, _, command in table:
        if pid in below and "com.apple.WebKit.WebContent" in command:
            with contextlib.suppress(ProcessLookupError):
                os.kill(pid, signal.SIGKILL)
                killed += 1
    return killed


def serve_control(listener, device):
    """One word per line in, one word back: `crash` stops the web processes of the Simulator."""
    while True:
        try:
            client, _ = listener.accept()
        except OSError:
            return
        with client:
            command = client.makefile().readline().strip()
            answer = "done" if command == "crash" and stop_web_processes(device) else "unknown"
            client.sendall((answer + "\n").encode())


def find_manifest(build):
    manifests = list((Path(build) / "Build" / "Products").glob("Construct_iphonesimulator*.xctestrun"))
    if len(manifests) != 1:
        raise RuntimeError("Use a fresh build-for-testing folder with one Simulator test run.")
    return str(manifests[0])


def run_tests(manifest, device, environment, only, skip=(), result=None):
    """Runs the tests named in `only` (a name or a list of names) and says whether all of them passed."""
    command = ["xcodebuild", "-xctestrun", manifest, "-destination", f"id={device}", "-parallel-testing-enabled", "NO"]
    if result:
        command += ["-resultBundlePath", str(result)]
    command += [f"-only-testing:{item}" for item in ([only] if isinstance(only, str) else only)]
    command += [f"-skip-testing:{item}" for item in skip]
    command += ["test-without-building"]
    env = dict(os.environ, **{f"TEST_RUNNER_{key}": value for key, value in environment.items()})
    return subprocess.run(command, env=env, timeout=RUN_TIMEOUT).returncode == 0


def export_screenshots(result, folder, theme):
    """Saves the attachments of one run as `ios-<theme>-<state>.png`, and says whether every state was saved."""
    with tempfile.TemporaryDirectory(prefix="glasshouse-shots-") as temporary:
        subprocess.run(("xcrun", "xcresulttool", "export", "attachments", "--path", str(result), "--output-path",
                        temporary, "--filter", "*.png"), check=True, timeout=120)
        manifest = json.loads((Path(temporary) / "manifest.json").read_text())
        saved = set()
        for test in manifest:
            for attachment in test.get("attachments", []):
                name = attachment.get("suggestedHumanReadableName", "")
                match = re.match(rf"{theme}-([a-z-]+)_", name)
                if match and match.group(1) in STATES:
                    (folder / f"ios-{theme}-{match.group(1)}.png").write_bytes(
                        (Path(temporary) / attachment["exportedFileName"]).read_bytes())
                    saved.add(match.group(1))
    missing = [state for state in STATES if state not in saved]
    if missing:
        print(f"No {theme} screenshot of: {', '.join(missing)}", file=sys.stderr)
    return not missing


def interrupted(number, frame):
    del frame
    raise SystemExit(128 + number)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--ios-build", required=True, help="the build-for-testing folder (DerivedData path)")
    parser.add_argument("--ios-device-type", required=True, help="simctl device type identifier")
    parser.add_argument("--ios-runtime", required=True, help="simctl runtime identifier")
    parser.add_argument("--screenshots", type=Path, help="a folder to save a screenshot of each state in both themes")
    parser.add_argument("--screenshots-only", action="store_true", help="take the screenshots and run no other test")
    parser.add_argument("--only", action="append",
                        help="run only this test of GlasshouseTests, such as testBackForwardAndReloadWork; repeat it for more")
    args = parser.parse_args()
    if args.screenshots_only and not args.screenshots:
        parser.error("--screenshots-only needs --screenshots")
    signal.signal(signal.SIGTERM, interrupted)
    manifest = find_manifest(args.ios_build)

    with tempfile.TemporaryDirectory(prefix="glasshouse-test-") as temporary:
        folder = Path(temporary)
        make_certificates(folder)
        with test_simulator(args.ios_device_type, args.ios_runtime, args.ios_build, folder / "trusted.pem") as device:
            server = subprocess.Popen([sys.executable, str(SERVER), "--certificates", str(folder), "--host", "127.0.0.1"],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            assert server.stdin is not None and server.stdout is not None
            with socket.create_server(("127.0.0.1", 0)) as listener:
                threading.Thread(target=serve_control, args=(listener, device), daemon=True).start()
                try:
                    sites = json.loads(server.stdout.readline())
                    environment = {
                        "GLASSHOUSE_TRUSTED": sites["trusted"], "GLASSHOUSE_UNTRUSTED": sites["untrusted"],
                        "GLASSHOUSE_PLAIN": sites["plain"], "GLASSHOUSE_FLAKY": sites["flaky"],
                        "GLASSHOUSE_VERSION": app_version(), "GLASSHOUSE_CONTROL_PORT": str(listener.getsockname()[1]),
                    }
                    passed = True
                    if args.only:
                        passed = run_tests(manifest, device, environment, [f"{TESTS}/{name}" for name in args.only])
                    elif not args.screenshots_only:
                        passed = run_tests(manifest, device, environment, TESTS)
                    if args.screenshots and not args.only:
                        args.screenshots.mkdir(parents=True, exist_ok=True)
                        subprocess.run(("xcrun", "simctl", "status_bar", device, "override", "--time", "9:41",
                                        "--batteryState", "charged", "--batteryLevel", "100", "--wifiBars", "3",
                                        "--cellularBars", "4"), check=False, timeout=60)
                        for theme in ("light", "dark"):
                            subprocess.run(("xcrun", "simctl", "ui", device, "appearance", theme), check=True, timeout=60)
                            result = args.screenshots / f"Glasshouse-{theme}.xcresult"
                            if result.exists():
                                raise SystemExit(f"{result} exists: use a folder with no earlier run.")
                            passed &= run_tests(manifest, device, {**environment, "GLASSHOUSE_THEME": theme}, SHOTS_TEST,
                                                result=result)
                            passed &= export_screenshots(result, args.screenshots, theme)
                    print("PASSED" if passed else "FAILED")
                    return 0 if passed else 1
                finally:
                    server.stdin.close()
                    server.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())
