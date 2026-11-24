#!/usr/bin/env python3
"""Verifies phone pairing against current and previous agent builds on a disposable device."""

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
ANDROID_PATH = f"files/{FIXTURE}"
ANDROID_TEST = "cf.construct.mobile.android.PhonePairingTest"
ANDROID_COMPUTER_TEST = "cf.construct.mobile.android.ComputerPageTest"
IOS_TEST = "ConstructTests/PhonePairingTests"
IOS_FRAME_TEST = "ConstructUITests/PhonePairingFrameTests"
IOS_COMPUTER_TEST = "ConstructUITests/ComputerPageTests"
CANARY = "c3" * 32


def read(*args):
    return subprocess.check_output(args, stderr=subprocess.DEVNULL, text=True, timeout=30).strip()


class Android:
    host = "10.0.2.2"
    bind = "127.0.0.1:0"
    SCREENSHOTS = ("not-paired", "manual", "connected", "unreachable", "confirm-unpair")

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

    def command(self, method):
        subprocess.run((*self.adb, "shell", "am", "force-stop", self.app), check=True)
        screenshots = ("-e", "computerScreenshots", "enabled") if self.screenshots else ()
        suite = ANDROID_COMPUTER_TEST if method == "computerPage" else ANDROID_TEST
        return (*self.adb, "shell", "am", "instrument", "-w", "-e", "computerPairingFixture", "enabled", *screenshots,
                "-e", "class", f"{suite}#{method}", f"{self.app}.test/androidx.test.runner.AndroidJUnitRunner")

    def collect_screenshots(self):
        if self.screenshots is None:
            return
        self.screenshots.mkdir(parents=True, exist_ok=True)
        for name in self.SCREENSHOTS:
            with (self.screenshots / f"{name}.png").open("wb") as image:
                subprocess.run((*self.adb, "exec-out", "run-as", self.app, "cat", f"{ANDROID_PATH}/{name}.png"),
                               stdout=image, check=True, timeout=30)

    def logs(self):
        return read(*self.adb, "logcat", "-d", "-T", self.since)

    def environment(self, readiness_port):
        return None

    def clear(self):
        subprocess.run((*self.storage, "rm", "-rf", ANDROID_PATH), check=True)


class Ios:
    host = "127.0.0.1"
    bind = "127.0.0.1:0"

    def __init__(self, device, build, screenshots=None):
        self.device = device
        products = Path(build) / "Build" / "Products"
        manifests = list(products.glob("Construct_iphonesimulator*.xctestrun"))
        if len(manifests) != 1:
            raise ValueError("Use a fresh build-for-testing directory with one Simulator test run.")
        self.manifest = str(manifests[0])
        self.screenshots = Path(screenshots).resolve() if screenshots else None
        self.result = self.screenshots / "ComputerPage.xcresult" if self.screenshots else None
        if self.screenshots:
            self.screenshots.mkdir(parents=True, exist_ok=True)
        project = MOBILE / "iosApp" / "project.yml"
        self.app = next(line.split(":", 1)[1].strip() for line in project.read_text().splitlines()
                        if line.strip().startswith("APP_BUNDLE_ID:"))
        self.directory = self.app_directory()
        self.directory.mkdir(parents=True, exist_ok=True)
        self.clear()
        self.write("enabled", "ready")
        self.since = time.strftime("%Y-%m-%d %H:%M:%S")

    def app_directory(self):
        container = read("xcrun", "simctl", "get_app_container", self.device, self.app, "data")
        return Path(container) / "Documents" / FIXTURE

    def write(self, name, value):
        temporary = self.directory / f"{name}.part"
        with open(temporary, "w", opener=private_file) as handle:
            handle.write(value)
        temporary.replace(self.directory / name)

    def exists(self, name):
        if (self.directory / name).is_file():
            return True
        # XCTest may reinstall the app and move its Documents directory between processes.
        try:
            self.directory = self.app_directory()
        except subprocess.CalledProcessError:
            return False
        return (self.directory / name).is_file()

    def read(self, name):
        return (self.directory / name).read_text()

    def command(self, method):
        swift = method[:1].upper() + method[1:]
        suite = {"reconnectAfterAppRestart": IOS_FRAME_TEST, "computerPage": IOS_COMPUTER_TEST}.get(method, IOS_TEST)
        result = ("-resultBundlePath", str(self.result)) if self.result else ()
        return ("xcodebuild", "-xctestrun", self.manifest, "-destination",
                f"id={self.device}", "-parallel-testing-enabled", "NO", *result,
                f"-only-testing:{suite}/test{swift}", "test-without-building")

    def environment(self, readiness_port):
        if readiness_port is None:
            return None
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
        return read("xcrun", "simctl", "spawn", self.device, "log", "show", "--start", self.since,
                    "--predicate", 'process == "Construct"', "--style", "compact")

    def clear(self):
        for path in self.directory.glob("*"):
            if path.is_file():
                path.unlink()


def private_file(path, flags):
    return os.open(path, flags, 0o600)


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
def agent(binary, device, directory, protected, bind=None, control=True):
    flags = ("--control",) if control else ()
    with subprocess.Popen((binary, *flags, "--host", device.host, "--bind", bind or device.bind,
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
def test(device, method, protected, digests, readiness_port=None):
    with tempfile.TemporaryFile(mode="w+") as output:
        env = device.environment(readiness_port)
        with subprocess.Popen(device.command(method), stdout=output, stderr=subprocess.STDOUT, env=env) as process:
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
                raise RuntimeError(f"{method}: {reason} Exit code: {code}.")
            if contains_pairing_material(log, protected, digests):
                raise RuntimeError("An app test exposed a pairing credential.")
            print(f"{method} passed")


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


def verify_frame(args, device, directory, latter, protected, digests):
    port = int(latter["address"].rsplit(":", 1)[1])
    loopback = ("127.0.0.1", port)
    # The agent is stopped. Accept and close the app's first connection so the host knows
    # when the app encountered the outage before restarting the agent on the same address.
    with contextlib.ExitStack() as sockets:
        outage = sockets.enter_context(socket.create_server(loopback))
        ready_socket = None
        if isinstance(device, Ios):
            ready_socket = sockets.enter_context(socket.create_server(("127.0.0.1", 0)))
        ready_port = ready_socket.getsockname()[1] if ready_socket else None
        with test(device, "reconnectAfterAppRestart", protected, digests, ready_port) as restart:
            wait_for_client(outage, restart, "The restarted app did not check its saved computer address.").close()
            outage.close()
            with agent(args.agent, device, directory, protected, f"{loopback[0]}:{port}"):
                if ready_socket:
                    ready_socket.settimeout(AGENT_START_TIMEOUT)
                    client, _ = ready_socket.accept()
                    with client:
                        client.sendall(b"\x01")
                else:
                    device.write("online", "ready")
                if restart.wait(timeout=RUNNER_TIMEOUT) != 0:
                    raise RuntimeError("The phone did not reconnect from the frame.")


def verify(args, device):
    protected = set()
    digests = set()
    try:
        with tempfile.TemporaryDirectory(prefix="construct-phone-test-") as temporary:
            path = Path(temporary)
            previous = path / "previous"
            with agent(args.previous_agent, device, previous, protected, control=False) as (_, _, legacy):
                write_offer(device, "previous-offer", legacy)
                device.write("previous-qr", legacy["payload"])
                with test(device, "pairWithPreviousAgent", protected, digests) as pairing:
                    if pairing.wait(timeout=RUNNER_TIMEOUT) != 0:
                        raise RuntimeError("The previous-agent test failed.")
                    confirm_credential(device, "previous", previous, digests)
            with agent(args.agent, device, path / "first", protected) as (first, lines, initial):
                write_offer(device, "offer", initial)
                fingerprint = initial["fingerprint"]
                different = ("1" if fingerprint[0] == "0" else "0") + fingerprint[1:]
                device.write("wrong-qr", initial["payload"].replace(fingerprint, different, 1))
                with test(device, "pairAndUnpair", protected, digests) as pairing:
                    wait_for(device, "revoke", pairing)
                    confirm_credential(device, "first", path / "first", digests)
                    send(first, "unpair")
                    if agent_line(lines, time.monotonic() + CONTROL_TIMEOUT) != "unpaired":
                        raise RuntimeError("The agent example did not revoke the pairing.")
                    if isinstance(device, Ios):
                        device.write("revoked", "acknowledged")
                    wait_for(device, "next", pairing)
                    with agent(args.agent, device, path / "second", protected) as (second, _, later):
                        device.write("second-qr", later["payload"])
                        if pairing.wait(timeout=RUNNER_TIMEOUT) != 0:
                            raise RuntimeError("The pairing test failed before the app restart.")
                        confirm_credential(device, "second", path / "second", digests)
                        send(second, "stop")
                        second.wait(timeout=CONTROL_TIMEOUT)
                verify_frame(args, device, path / "second", later, protected, digests)
            verify_device_logs(device, protected, digests)
    finally:
        device.clear()


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
                with test(device, "computerPage", protected, digests) as app:
                    wait_for(device, "computer-stop", app)
                    confirm_credential(device, "computer", directory, digests)
                    send(first, "stop")
                    first.wait(timeout=CONTROL_TIMEOUT)
                    wait_for(device, "computer-retry", app)
                    port = int(offer["address"].rsplit(":", 1)[1])
                    with agent(args.agent, device, directory, protected, f"127.0.0.1:{port}"):
                        if app.wait(timeout=RUNNER_TIMEOUT) != 0:
                            raise RuntimeError("The Computer page test did not complete.")
            verify_device_logs(device, protected, digests)
            device.collect_screenshots()
    finally:
        device.clear()


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
    try:
        with tempfile.TemporaryDirectory(prefix="construct-computer-test-") as temporary:
            directory = Path(temporary)
            with socket.create_server(("127.0.0.1", 0)) as listener:
                with agent(args.agent, device, directory, protected) as (first, _, offer):
                    port = listener.getsockname()[1]
                    with test(device, "computerPage", protected, digests, port) as app:
                        with fixture_request(listener, app, "offer") as client:
                            client.sendall((offer["address"] + "\n" + offer["code"]).encode())
                        with fixture_request(listener, app, "stop") as client:
                            digests.add(credential_digest(directory))
                            send(first, "stop")
                            first.wait(timeout=CONTROL_TIMEOUT)
                            client.sendall(b"ready")
                        with contextlib.ExitStack() as agents:
                            with fixture_request(listener, app, "restart") as client:
                                endpoint = offer["address"]
                                agents.enter_context(agent(args.agent, device, directory, protected, endpoint))
                                client.sendall(b"ready")
                            if app.wait(timeout=RUNNER_TIMEOUT) != 0:
                                raise RuntimeError("The Computer page test did not complete.")
            verify_device_logs(device, protected, digests, marker="Computer page fixture started")
            device.collect_screenshots()
    finally:
        device.clear()


@contextlib.contextmanager
def disposable_simulator(device_type, runtime, build, theme):
    products = Path(build) / "Build" / "Products"
    apps = list(products.glob("*-iphonesimulator/Construct.app"))
    if len(apps) != 1:
        raise RuntimeError("Use a build-for-testing directory with one Construct Simulator app.")
    simctl = ("xcrun", "simctl")
    device = read(*simctl, "create", "Construct Pairing Fixture", device_type, runtime)
    failed = False
    try:
        subprocess.run((*simctl, "bootstatus", device, "-b"), check=True, timeout=300)
        for attempt in range(2):
            try:
                subprocess.run((*simctl, "spawn", device, "launchctl", "list"),
                               stdout=subprocess.DEVNULL, check=True, timeout=30)
                subprocess.run((*simctl, "install", device, str(apps[0])), check=True, timeout=120)
                break
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                if attempt == 1:
                    raise
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
    parser.add_argument("platform", choices=("android", "ios"))
    parser.add_argument("--agent", required=True, help="path to the compiled phone_pairing example")
    parser.add_argument("--previous-agent", help="phone_pairing example built from the previous agent")
    parser.add_argument("--device", help="serial of a disposable Android emulator")
    parser.add_argument("--ios-build", help="iOS build-for-testing DerivedData path")
    parser.add_argument("--ios-device-type", help="simctl device type identifier")
    parser.add_argument("--ios-runtime", help="simctl runtime identifier")
    parser.add_argument("--computer-page", action="store_true", help="run the Computer page fixture")
    parser.add_argument("--screenshots", help="optional directory for Computer page screenshots")
    parser.add_argument("--ios-theme", choices=("light", "dark"), help="Simulator appearance")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    if args.screenshots and not args.computer_page:
        parser.error("screenshots require --computer-page")
    if not args.computer_page and not args.previous_agent:
        parser.error("the pairing compatibility fixture requires --previous-agent")
    if args.platform == "android":
        if not args.device:
            parser.error("Android needs --device")
        device = Android(args.device, args.screenshots)
        if args.computer_page:
            verify_computer_page(args, device)
        else:
            verify(args, device)
    else:
        if not (args.ios_build and args.ios_device_type and args.ios_runtime) or args.device:
            parser.error("iOS creates its own Simulator and needs --ios-build, --ios-device-type and --ios-runtime")
        with disposable_simulator(args.ios_device_type, args.ios_runtime, args.ios_build, args.ios_theme) as device:
            simulator = Ios(device, args.ios_build, args.screenshots)
            if args.computer_page:
                verify_ios_computer_page(args, simulator)
            else:
                verify(args, simulator)


if __name__ == "__main__":
    main()
