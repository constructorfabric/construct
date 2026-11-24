#!/usr/bin/env python3
"""Runs the Computer page test of the app against the phone_pairing example on a disposable emulator or Simulator."""

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
from queue import Empty, Queue
import re
import signal
import socket
import subprocess
import sys
import tempfile
from threading import Thread
import time

MOBILE = Path(__file__).resolve().parents[1]
FIXTURE = "computer-pairing-test"
RUNNER_TIMEOUT = 240
CONTROL_TIMEOUT = 10
AGENT_START_TIMEOUT = 30
SIMULATOR_READY_TIMEOUT = 300
SIMULATOR_ANSWER_TIMEOUT = 60
ANDROID_PATH = f"files/{FIXTURE}"
ANDROID_COMPUTER_TEST = "cf.construct.mobile.android.ComputerPageTest"
IOS_COMPUTER_TEST = "ConstructUITests/ComputerPageTests/testComputerPage"
# The test logs this value; the log check must find it, or it is not reading the device log.
CANARY = "c3" * 32


def read(*args):
    return subprocess.check_output(args, stderr=subprocess.DEVNULL, text=True, timeout=30).strip()


class Android:
    host = "10.0.2.2"
    bind = "127.0.0.1:0"
    SCREENSHOTS = ("not-paired", "manual", "connected", "settings-connected", "unreachable", "settings-unreachable",
                   "chat-unreachable", "confirm-unpair")

    def __init__(self, device, screenshots=None):
        if not device.startswith("emulator-"):
            raise ValueError("Use a disposable Android emulator.")
        self.adb = ("adb", "-s", device)
        self.screenshots = Path(screenshots) if screenshots else None
        properties = (MOBILE / "gradle.properties").read_text().splitlines()
        self.app = next(line.split("=", 1)[1] for line in properties if line.startswith("constructApplicationId="))
        self.storage = (*self.adb, "shell", "run-as", self.app)
        if int(read(*self.adb, "shell", "getprop", "ro.build.version.sdk")) >= 37:
            subprocess.run((*self.adb, "shell", "pm", "grant", self.app,
                            "android.permission.ACCESS_LOCAL_NETWORK"), check=True)
        self.clear()
        subprocess.run((*self.storage, "mkdir", "-p", ANDROID_PATH), check=True)
        self.since = read(*self.adb, "shell", "date '+%m-%d %H:%M:%S.000'")

    def write(self, name, value):
        file = f"{ANDROID_PATH}/{name}"
        command = f"umask 077; cat > {file}.part && mv {file}.part {file}"
        subprocess.run((*self.adb, "shell", f"run-as {self.app} sh -c '{command}'"), input=value, text=True, check=True)

    def exists(self, name):
        return subprocess.run((*self.storage, "test", "-f", f"{ANDROID_PATH}/{name}"),
                              check=False, timeout=10).returncode == 0

    def read(self, name):
        return read(*self.storage, "cat", f"{ANDROID_PATH}/{name}")

    def command(self):
        subprocess.run((*self.adb, "shell", "am", "force-stop", self.app), check=True)
        screenshots = ("-e", "computerScreenshots", "enabled") if self.screenshots else ()
        return (*self.adb, "shell", "am", "instrument", "-w", "-e", "computerPairingFixture", "enabled", *screenshots,
                "-e", "class", f"{ANDROID_COMPUTER_TEST}#computerPage",
                f"{self.app}.test/androidx.test.runner.AndroidJUnitRunner")

    def collect_screenshots(self):
        if self.screenshots is None:
            return
        self.screenshots.mkdir(parents=True, exist_ok=True)
        for name in self.SCREENSHOTS:
            with (self.screenshots / f"{name}.png").open("wb") as image:
                subprocess.run((*self.adb, "exec-out", "run-as", self.app, "cat", f"{ANDROID_PATH}/{name}.png"),
                               stdout=image, check=True, timeout=30)

    def environment(self, readiness_port):
        return None

    def logs(self):
        return read(*self.adb, "logcat", "-d", "-T", self.since)

    def clear(self):
        subprocess.run((*self.storage, "rm", "-rf", ANDROID_PATH), check=True)


class Ios:
    """The Computer page test of the iPhone app. It asks the host for each step over a loopback port."""

    host = "127.0.0.1"
    bind = "127.0.0.1:0"

    def __init__(self, device, build, screenshots=None):
        self.device = device
        manifests = list((Path(build) / "Build" / "Products").glob("Construct_iphonesimulator*.xctestrun"))
        if len(manifests) != 1:
            raise ValueError("Use a fresh build-for-testing directory with one Simulator test run.")
        self.manifest = str(manifests[0])
        self.screenshots = Path(screenshots).resolve() if screenshots else None
        self.result = self.screenshots / "ComputerPage.xcresult" if self.screenshots else None
        if self.result:
            if self.result.exists():
                raise ValueError("Use a screenshot directory without an earlier ComputerPage.xcresult.")
            self.screenshots.mkdir(parents=True, exist_ok=True)
        self.since = time.strftime("%Y-%m-%d %H:%M:%S")

    def command(self):
        result = ("-resultBundlePath", str(self.result)) if self.result else ()
        return ("xcodebuild", "-xctestrun", self.manifest, "-destination", f"id={self.device}",
                "-parallel-testing-enabled", "NO", *result, f"-only-testing:{IOS_COMPUTER_TEST}",
                "test-without-building")

    def environment(self, readiness_port):
        env = dict(os.environ, TEST_RUNNER_CONSTRUCT_PAIRING_READY_PORT=str(readiness_port))
        if self.screenshots:
            env["TEST_RUNNER_CAPTURE_MOBILE_STATES"] = "1"
        return env

    def collect_screenshots(self):
        if self.result is None:
            return
        subprocess.run(("xcrun", "xcresulttool", "export", "attachments", "--path", str(self.result),
                        "--output-path", str(self.screenshots), "--filter", "*.png"), check=True, timeout=30)

    def logs(self):
        # Without --info and --debug the log leaves out the lines an app writes at those levels.
        return read("xcrun", "simctl", "spawn", self.device, "log", "show", "--start", self.since,
                    "--info", "--debug", "--predicate", 'process == "Construct"', "--style", "compact")


def contains_pairing_material(log, protected, digests):
    if any(secret in log for secret in protected):
        return True
    # Compare hashed log candidates with the app's digests so the token never leaves secure storage.
    return any(hashlib.sha256(match[1].lower().encode()).hexdigest() in digests
               for match in re.finditer(r"(?=([0-9a-fA-F]{64}))", log))


def confirm_credential(device, label, agent_directory, digests):
    name = f"{label}-digest"
    if not device.exists(name):
        raise RuntimeError("The app did not provide a credential digest.")
    digest = device.read(name).strip()
    if not re.fullmatch(r"[0-9a-f]{64}", digest) or digest in digests:
        raise RuntimeError("The app did not provide a distinct, valid credential digest.")
    if credential_digest(agent_directory) != digest:
        raise RuntimeError("The stored phone credential does not match the agent's token digest.")
    digests.add(digest)


def credential_digest(agent_directory):
    try:
        phones = json.loads((agent_directory / "phones.json").read_text())["phones"]
        if len(phones) == 1 and re.fullmatch(r"[0-9a-f]{64}", phones[0]["token_hash"]):
            return phones[0]["token_hash"]
    except (OSError, ValueError, TypeError, KeyError):
        raise RuntimeError("The agent's pairing record could not be read.") from None
    raise RuntimeError("The agent did not store one valid pairing digest.")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=CONTROL_TIMEOUT)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=CONTROL_TIMEOUT)


def read_agent_output(stdout, lines):
    for line in stdout:
        lines.put(line.rstrip("\n"))
    lines.put(None)


def agent_line(lines, deadline):
    try:
        line = lines.get(timeout=max(0, deadline - time.monotonic()))
    except Empty:
        raise RuntimeError("Timed out waiting for the agent example.") from None
    if line is None:
        raise RuntimeError("The agent example closed its output.")
    return line


@contextlib.contextmanager
def agent(binary, device, directory, protected, bind=None):
    with subprocess.Popen((binary, "--control", "--host", device.host, "--bind", bind or device.bind,
                           "--dir", str(directory)), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, text=True) as process:
        lines = Queue()
        reader = Thread(target=read_agent_output, args=(process.stdout, lines), daemon=True)
        reader.start()
        try:
            fields = {}
            deadline = time.monotonic() + AGENT_START_TIMEOUT
            while True:
                line = agent_line(lines, deadline)
                if line == "ready":
                    break
                name, _, value = line.partition(": ")
                fields[name] = value
            if not all(fields.get(field) for field in ("address", "code", "payload", "fingerprint")):
                raise RuntimeError("The agent example did not produce a pairing offer.")
            protected.update((fields["code"], fields["fingerprint"], fields["payload"].split("&s=")[-1]))
            yield process, lines, fields
        finally:
            stop(process)
            reader.join(timeout=CONTROL_TIMEOUT)


@contextlib.contextmanager
def test(device, protected, digests, readiness_port=None):
    with tempfile.TemporaryFile(mode="w+") as output:
        env = device.environment(readiness_port)
        with subprocess.Popen(device.command(), stdout=output, stderr=subprocess.STDOUT, env=env) as process:
            failure = None
            code = 1
            try:
                yield process
                code = process.wait(timeout=RUNNER_TIMEOUT)
            except (RuntimeError, subprocess.TimeoutExpired) as error:
                failure = str(error) if isinstance(error, RuntimeError) else "The test runner timed out."
            finally:
                stop(process)
                output.seek(0)
                log = output.read()
                if contains_pairing_material(log, protected, set()):
                    raise RuntimeError("An app test exposed protected pairing material.")
            passed = "OK (1 test)" in log or "Executed 1 test, with 0 failures" in log
            if failure or code != 0 or not passed:
                reason = failure or "The test runner did not report a passing test."
                # The log was checked for pairing material above, so its tail can say what failed.
                raise RuntimeError(f"computerPage: {reason} Exit code: {code}.\n{log[-3000:]}")
            if contains_pairing_material(log, protected, digests):
                raise RuntimeError("An app test exposed a pairing credential.")
            print("computerPage passed")


def wait_for(device, name, test_process):
    end = time.monotonic() + RUNNER_TIMEOUT
    while time.monotonic() < end and test_process.poll() is None:
        if device.exists(name):
            return
        time.sleep(0.2)
    raise RuntimeError(f"The app test did not signal {name}.")


def send(process, command):
    process.stdin.write(command + "\n")
    process.stdin.flush()


def write_offer(device, name, fields):
    device.write(name, fields["address"] + "\n" + fields["code"] + "\n")


def verify_device_logs(device, protected, digests, *, marker=None):
    # No raw log or key material is emitted, including on failure.
    device_log = device.logs()
    canary_digest = hashlib.sha256(CANARY.encode()).hexdigest()
    captured = marker in device_log if marker else contains_pairing_material(device_log, set(), {canary_digest})
    if not captured:
        raise RuntimeError("The device collector or scanner missed its planted log marker.")
    print("The device collector captured the planted log marker")
    if contains_pairing_material(device_log, protected, digests):
        raise RuntimeError("The app log contains protected pairing material.")
    print("No pairing material in the device log")


def verify_computer_page(args, device):
    protected = set()
    digests = set()
    try:
        with tempfile.TemporaryDirectory(prefix="construct-computer-test-") as temporary:
            directory = Path(temporary)
            with agent(args.agent, device, directory, protected) as (first, _, offer):
                write_offer(device, "offer", offer)
                with test(device, protected, digests) as app:
                    wait_for(device, "computer-stop", app)
                    confirm_credential(device, "computer", directory, digests)
                    send(first, "stop")
                    first.wait(timeout=CONTROL_TIMEOUT)
                    device.write("computer-stopped", "ready")
                    wait_for(device, "computer-retry", app)
                    port = int(offer["address"].rsplit(":", 1)[1])
                    with agent(args.agent, device, directory, protected, f"127.0.0.1:{port}"):
                        device.write("computer-restarted", "ready")
                        if app.wait(timeout=RUNNER_TIMEOUT) != 0:
                            raise RuntimeError("The Computer page test did not complete.")
            verify_device_logs(device, protected, digests)
            device.collect_screenshots()
    finally:
        device.clear()


def wait_for_client(listener, process, missing):
    deadline = time.monotonic() + RUNNER_TIMEOUT
    listener.settimeout(1)
    while time.monotonic() < deadline and process.poll() is None:
        try:
            client, _ = listener.accept()
        except TimeoutError:
            continue
        return client
    raise RuntimeError(missing)


@contextlib.contextmanager
def fixture_request(listener, process, command):
    client = wait_for_client(listener, process, f"The Simulator did not request {command}.")
    with client, client.makefile("rb") as request:
        client.settimeout(CONTROL_TIMEOUT)
        if request.readline(32) != (command + "\n").encode():
            raise RuntimeError(f"The Simulator did not request {command}.")
        yield client


def verify_ios_computer_page(args, device):
    protected = set()
    digests = set()
    with tempfile.TemporaryDirectory(prefix="construct-computer-test-") as temporary:
        directory = Path(temporary)
        with socket.create_server(("127.0.0.1", 0)) as listener:
            with agent(args.agent, device, directory, protected) as (first, _, offer):
                with test(device, protected, digests, listener.getsockname()[1]) as app:
                    with fixture_request(listener, app, "offer") as client:
                        client.sendall((offer["address"] + "\n" + offer["code"]).encode())
                    with fixture_request(listener, app, "stop") as client:
                        digests.add(credential_digest(directory))
                        send(first, "stop")
                        first.wait(timeout=CONTROL_TIMEOUT)
                        client.sendall(b"ready")
                    # The test reads a reply until the host closes the connection, so the host answers and
                    # closes first, and the restarted agent keeps running until the test ends.
                    with contextlib.ExitStack() as restarted:
                        with fixture_request(listener, app, "restart") as client:
                            restarted.enter_context(agent(args.agent, device, directory, protected, offer["address"]))
                            client.sendall(b"ready")
                        if app.wait(timeout=RUNNER_TIMEOUT) != 0:
                            raise RuntimeError("The Computer page test did not complete.")
        verify_device_logs(device, protected, digests, marker="Computer page fixture started")
        device.collect_screenshots()


@contextlib.contextmanager
def disposable_simulator(device_type, runtime, build, theme):
    apps = list((Path(build) / "Build" / "Products").glob("*-iphonesimulator/Construct.app"))
    if len(apps) != 1:
        raise RuntimeError("Use a build-for-testing directory with one Construct Simulator app.")
    # An unsigned Simulator app has no application identifier, and the Simulator of iOS 27 then refuses it
    # every Keychain call, so the page would show a storage error instead of "Not paired yet". Signing ad hoc
    # puts the entitlements, with the application identifier, into an `__entitlements` section of the binary.
    if "__entitlements" not in read("otool", "-l", str(apps[0] / "Construct")):
        raise RuntimeError("Build the app with CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- (see the README).")
    simctl = ("xcrun", "simctl")
    device = read(*simctl, "create", "Construct Pairing Fixture", device_type, runtime)
    failed = False
    try:
        subprocess.run((*simctl, "bootstatus", device, "-b"), check=True, timeout=300)
        # A booted Simulator can take minutes to answer on a busy Mac: ask until it does, then install.
        deadline = time.monotonic() + SIMULATOR_READY_TIMEOUT
        while True:
            try:
                subprocess.run((*simctl, "spawn", device, "launchctl", "list"),
                               stdout=subprocess.DEVNULL, check=True, timeout=SIMULATOR_ANSWER_TIMEOUT)
                subprocess.run((*simctl, "install", device, str(apps[0])), check=True,
                               timeout=SIMULATOR_READY_TIMEOUT)
                break
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                if time.monotonic() > deadline:
                    raise
                time.sleep(5)
        if theme:
            subprocess.run((*simctl, "ui", device, "appearance", theme), check=True, timeout=30)
        yield device
    except BaseException:
        failed = True
        raise
    finally:
        try:
            try:
                subprocess.run((*simctl, "shutdown", device), stderr=subprocess.DEVNULL, timeout=30)
            except subprocess.TimeoutExpired:
                pass
            finally:
                subprocess.run((*simctl, "delete", device), check=True, timeout=60)
        except (OSError, subprocess.SubprocessError) as error:
            if not failed:
                raise
            print(f"The disposable Simulator could not be deleted: {error}", file=sys.stderr)


def interrupted(number, _frame):
    raise SystemExit(128 + number)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", required=True, help="path to the compiled phone_pairing example")
    parser.add_argument("--device", help="serial of a disposable Android emulator")
    parser.add_argument("--ios-build", help="iOS build-for-testing DerivedData path; runs the iPhone test")
    parser.add_argument("--ios-device-type", help="simctl device type identifier")
    parser.add_argument("--ios-runtime", help="simctl runtime identifier")
    parser.add_argument("--ios-theme", choices=("light", "dark"), help="Simulator appearance")
    parser.add_argument("--screenshots", help="optional directory for the screenshots of the test")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    if bool(args.device) == bool(args.ios_build):
        parser.error("use --device for Android or --ios-build for the iPhone, not both")
    if args.device:
        verify_computer_page(args, Android(args.device, args.screenshots))
        return
    if not (args.ios_device_type and args.ios_runtime):
        parser.error("the iPhone creates its own Simulator and needs --ios-device-type and --ios-runtime")
    with disposable_simulator(args.ios_device_type, args.ios_runtime, args.ios_build, args.ios_theme) as device:
        verify_ios_computer_page(args, Ios(device, args.ios_build, args.screenshots))


if __name__ == "__main__":
    main()
