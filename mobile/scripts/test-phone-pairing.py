#!/usr/bin/env python3
"""Runs the Computer page test of the app, and with --sync the sync test (on Android and on the iPhone) or with
--dashboard the iPhone Dashboard test, against the phone_pairing example on a disposable Android emulator or on the
shared iPhone Simulator (see shared_simulator.py)."""

import argparse
from collections import defaultdict
import contextlib
from datetime import datetime, timedelta, timezone
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
from zoneinfo import ZoneInfo

# The checks of this script load it with `python3 -I`, which has no folder of the script on the path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import shared_simulator  # noqa: E402

MOBILE = Path(__file__).resolve().parents[1]
FIXTURE = "computer-pairing-test"
RUNNER_TIMEOUT = 240
CONTROL_TIMEOUT = 10
AGENT_START_TIMEOUT = 30
ANDROID_PATH = f"files/{FIXTURE}"
ANDROID_COMPUTER_TEST = "cf.construct.mobile.android.ComputerPageTest#computerPage"
ANDROID_SYNC_TEST = "cf.construct.mobile.android.SyncTest#sync"
IOS_COMPUTER_TEST = "ConstructUITests/ComputerPageTests/testComputerPage"
IOS_SYNC_TEST = "ConstructUITests/SyncTests/testSync"
IOS_DASHBOARD_TEST = "ConstructUITests/DashboardTests/testDashboard"
# The time zone of the computer in the data file: the days of the Dashboard page are its days.
COMPUTER_ZONE = "Europe/Berlin"
# The sync test waits for a loading screen that is slowed on purpose, then for checks, so it runs longer.
SYNC_TIMEOUT = 600
# The emulator applies a new appearance to every app and restarts some of them. A test started in the middle of that
# can have its app killed for a start that takes too long, so the test starts when the emulator has been idle for
# QUIET_POLLS looks in a row, one second apart, and gives up after THEME_SETTLE_SECONDS.
THEME_SETTLE_SECONDS = 60
QUIET_POLLS = 5
QUIET_IDLE_SHARE = 0.8
COMPUTER_SCREENSHOTS = ("not-paired", "manual", "connected", "settings-connected", "unreachable",
                        "settings-unreachable", "chat-unreachable", "confirm-unpair")
SYNC_SCREENSHOTS = ("loading", "syncing", "justsynced", "unreachable", "computer")
# The test logs this value; the log check must find it, or it is not reading the device log.
CANARY = "c3" * 32


def read(*args):
    return subprocess.check_output(args, stderr=subprocess.DEVNULL, text=True, timeout=30).strip()


class Android:
    host = "10.0.2.2"
    bind = "127.0.0.1:0"
    # The test asks for the pairing offer through a file, and the credential digest is the app's to give.
    offer_in_command = False

    def __init__(self, device, screenshots=None, only_testing=ANDROID_COMPUTER_TEST, names=COMPUTER_SCREENSHOTS,
                 theme=None):
        if not device.startswith("emulator-"):
            raise ValueError("Use a disposable Android emulator.")
        self.adb = ("adb", "-s", device)
        self.only_testing = only_testing
        self.names = names
        if theme:
            self.apply_theme(theme)
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

    def apply_theme(self, theme):
        """Sets the appearance of the emulator, and returns when it has settled."""
        subprocess.run((*self.adb, "shell", "cmd", "uimode", "night", "yes" if theme == "dark" else "no"),
                       check=True, timeout=30)
        deadline = time.monotonic() + THEME_SETTLE_SECONDS
        quiet = 0
        while quiet < QUIET_POLLS:
            if time.monotonic() > deadline:
                raise RuntimeError("The emulator did not settle after the change of appearance.")
            quiet = quiet + 1 if self.idle_share() >= QUIET_IDLE_SHARE else 0

    def idle_share(self):
        """The share of the processor that was idle during the last second, from the second look of `top`."""
        output = read(*self.adb, "shell", "top", "-b", "-n", "2", "-d", "1")
        looks = re.findall(r"(\d+)%cpu\s.*?(\d+)%idle", output)
        if not looks:
            raise RuntimeError("The emulator does not say how busy it is.")
        total, idle = looks[-1]
        return int(idle) / int(total)

    def confirm_stop(self, directory, digests):
        confirm_credential(self, "computer", directory, digests)

    def run_sync(self, name, protected, digests, serve, offer):
        write_offer(self, "offer", offer)
        with test(self, protected, digests, None, name, SYNC_TIMEOUT) as app:
            serve_files(self, app, serve)
        verify_device_logs(self, protected, digests)

    def write(self, name, value):
        file = f"{ANDROID_PATH}/{name}"
        command = f"umask 077; cat > {file}.part && mv {file}.part {file}"
        subprocess.run((*self.adb, "shell", f"run-as {self.app} sh -c '{command}'"), input=value, text=True, check=True)

    def exists(self, name):
        return subprocess.run((*self.storage, "test", "-f", f"{ANDROID_PATH}/{name}"),
                              check=False, timeout=10).returncode == 0

    def remove(self, name):
        subprocess.run((*self.storage, "rm", "-f", f"{ANDROID_PATH}/{name}"), check=True, timeout=10)

    def read(self, name):
        return read(*self.storage, "cat", f"{ANDROID_PATH}/{name}")

    def command(self):
        subprocess.run((*self.adb, "shell", "am", "force-stop", self.app), check=True)
        screenshots = ("-e", "computerScreenshots", "enabled") if self.screenshots else ()
        return (*self.adb, "shell", "am", "instrument", "-w", "-e", "computerPairingFixture", "enabled", *screenshots,
                "-e", "class", self.only_testing,
                f"{self.app}.test/androidx.test.runner.AndroidJUnitRunner")

    def collect_screenshots(self):
        if self.screenshots is None:
            return
        self.screenshots.mkdir(parents=True, exist_ok=True)
        for name in self.names:
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
    # The test asks for the offer in a command, and the host reads the credential digest from the agent.
    offer_in_command = True

    def __init__(self, device, build, screenshots=None, only_testing=IOS_COMPUTER_TEST, name="ComputerPage"):
        self.device = device
        self.only_testing = only_testing
        manifests = list((Path(build) / "Build" / "Products").glob("Construct_iphonesimulator*.xctestrun"))
        if len(manifests) != 1:
            raise ValueError("Use a fresh build-for-testing directory with one Simulator test run.")
        self.manifest = str(manifests[0])
        self.screenshots = Path(screenshots).resolve() if screenshots else None
        self.result = self.screenshots / f"{name}.xcresult" if self.screenshots else None
        if self.result:
            if self.result.exists():
                raise ValueError(f"Use a screenshot directory without an earlier {name}.xcresult.")
            self.screenshots.mkdir(parents=True, exist_ok=True)
        self.since = time.strftime("%Y-%m-%d %H:%M:%S")

    def confirm_stop(self, directory, digests):
        digests.add(credential_digest(directory))

    def run_sync(self, name, protected, digests, serve, offer):
        with socket.create_server(("127.0.0.1", 0)) as listener:
            with test(self, protected, digests, listener.getsockname()[1], name, SYNC_TIMEOUT) as app:
                serve_socket(listener, app, serve)
        verify_device_logs(self, protected, digests, marker="Sync fixture started")

    def command(self):
        result = ("-resultBundlePath", str(self.result)) if self.result else ()
        return ("xcodebuild", "-xctestrun", self.manifest, "-destination", f"id={self.device}",
                "-parallel-testing-enabled", "NO", *result, f"-only-testing:{self.only_testing}",
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
def agent(binary, device, directory, protected, bind=None, data=None):
    data_arguments = ("--data", str(data)) if data else ()
    with subprocess.Popen((binary, "--control", "--host", device.host, "--bind", bind or device.bind,
                           "--dir", str(directory), *data_arguments), stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True) as process:
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
def test(device, protected, digests, readiness_port=None, name="computerPage", timeout=RUNNER_TIMEOUT):
    with tempfile.TemporaryFile(mode="w+") as output:
        env = device.environment(readiness_port)
        with subprocess.Popen(device.command(), stdout=output, stderr=subprocess.STDOUT, env=env) as process:
            failure = None
            code = 1
            try:
                yield process
                code = process.wait(timeout=timeout)
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
                raise RuntimeError(f"{name}: {reason} Exit code: {code}.\n{log[-3000:]}")
            if contains_pairing_material(log, protected, digests):
                raise RuntimeError("An app test exposed a pairing credential.")
            print(f"{name} passed")


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
            directory = Path(temporary) / "agent"
            directory.mkdir()
            # The first copy after the pairing has a chat and two facts to copy, as on the iPhone.
            data = Path(temporary) / "data.json"
            write_sync_data(data, [sync_chat("c1", "Plan my week")],
                            sync_facts([("name", "Alex Example"), ("title", "Mobile developer")]))
            with agent(args.agent, device, directory, protected, data=data) as (first, _, offer):
                write_offer(device, "offer", offer)
                with test(device, protected, digests) as app:
                    wait_for(device, "computer-stop", app)
                    confirm_credential(device, "computer", directory, digests)
                    send(first, "stop")
                    first.wait(timeout=CONTROL_TIMEOUT)
                    device.write("computer-stopped", "ready")
                    wait_for(device, "computer-retry", app)
                    port = int(offer["address"].rsplit(":", 1)[1])
                    with agent(args.agent, device, directory, protected, f"127.0.0.1:{port}", data):
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
        directory = Path(temporary) / "agent"
        directory.mkdir()
        # The program answers the sync commands as the real one does: the app syncs after pairing and on Try again.
        # It serves what the app shows before pairing, so what the sync copies is what the test looks for.
        data = Path(temporary) / "data.json"
        write_sync_data(data, [sync_chat("c1", "Plan my week")],
                        sync_facts([("name", "Alex Example"), ("title", "Mobile developer")]))
        with socket.create_server(("127.0.0.1", 0)) as listener:
            with agent(args.agent, device, directory, protected, data=data) as (first, _, offer):
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
                            restarted.enter_context(agent(args.agent, device, directory, protected, offer["address"], data))
                            client.sendall(b"ready")
                        if app.wait(timeout=RUNNER_TIMEOUT) != 0:
                            raise RuntimeError("The Computer page test did not complete.")
        verify_device_logs(device, protected, digests, marker="Computer page fixture started")
        device.collect_screenshots()


def sync_chat(chat_id, title):
    started = "2026-11-01T10:00:00Z"
    return {"id": chat_id, "title": title, "started": started,
            "items": [{"kind": "message", "role": "user", "time": started, "text": "Hello"}]}


def sync_facts(properties):
    """Facts for the data file: `properties` is a list of (property, value) pairs."""
    return [{"fact_id": f"f{number}", "entity_kind": "skill", "property": name, "value": value,
             "stored_at": "2026-11-01T10:00:00Z"} for number, (name, value) in enumerate(properties, 1)]


def write_sync_data(path, chats, facts=None, sessions=None, hidden=()):
    """Made-up facts, chats and sessions for the computer's test program, which reads the file again at every command.
    The dashboard is the program's default one, a total, bars per day, two tables and the summary of a day, unless
    `hidden` names projects the person hides on the computer."""
    facts = facts or sync_facts([(f"Skill {number}", f"Level {number}") for number in range(1, 13)])
    path.write_text(json.dumps({"zone": COMPUTER_ZONE, "facts": {"version": 1, "items": facts},
                                "chats": chats, "document": activity_document(hidden) if hidden else None,
                                "sessions": sessions or []}))


def activity_document(hidden):
    """The default dashboard of the computer (`default_activity` in the agent), with `hidden` projects hidden."""
    def widget(name, kind, place, group_by=None):
        column, row, width, height = place
        item = {"id": name, "kind": kind, "position": {"column": column, "row": row, "width": width, "height": height}}
        return {**item, "group_by": group_by} if group_by else item

    return {"type": "gts.cf.construct.core.dashboard.v1~cf.construct.dashboard.activity.v2~", "title": "Activity",
            "payload": {"period": "this_week", "sessions": "own", "hidden_projects": list(hidden), "project_names": {},
                        "widgets": [widget("total", "total", (0, 0, 12, 2)),
                                    widget("per-day", "bars_per_day", (0, 2, 12, 4)),
                                    widget("by-project", "table", (0, 6, 6, 5), "project"),
                                    widget("by-ticket", "table", (6, 6, 6, 5), "ticket"),
                                    widget("end-of-day", "day_summary", (0, 11, 12, 3))]}}


def dashboard_sessions(now=None):
    """Sessions on four days counted from today in the computer's zone, and what the page must show for them.

    Returns the sessions for the data file and `key=value` lines for the test: the days it looks at (today,
    yesterday, and a day of the week before the one of yesterday) and the minutes and sessions of each, and of the
    weeks. Today holds two sessions, so that every table has two rows."""
    zone = ZoneInfo(COMPUTER_ZONE)
    today = (now or datetime.now(zone)).astimezone(zone).date()
    monday = today - timedelta(days=today.weekday())
    picked = monday - timedelta(days=5)
    plan = [(today, "s1", "/w/app", "APP-1", 10, 45), (today, "s2", "/w/site", "WEB-2", 14, 20),
            (today - timedelta(days=1), "s3", "/w/app", "APP-1", 9, 30),
            (picked, "s4", "/w/app", "APP-3", 11, 50),
            (monday - timedelta(days=4), "s5", "/w/site", "WEB-4", 11, 25)]
    sessions = []
    minutes, count = defaultdict(int), defaultdict(int)
    for date, name, project, ticket, hour, length in plan:
        start = datetime(date.year, date.month, date.day, hour, tzinfo=zone)
        stamps = [(start + timedelta(minutes=step)).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:00Z")
                  for step in range(length)]
        sessions.append({"tool": "claude", "session": name, "project": project, "ticket": ticket, "minutes": stamps})
        minutes[date] += length
        count[date] += 1

    def week(date):
        first = date - timedelta(days=date.weekday())
        return sum(minutes[first + timedelta(days=step)] for step in range(7))

    yesterday = today - timedelta(days=1)
    # What today comes to when the project of the second session is hidden on the computer.
    kept = sum(length for date, _, project, _, _, length in plan if date == today and project != "/w/site")
    facts = {"today": today.isoformat(), "yesterday": yesterday.isoformat(), "picked": picked.isoformat(),
             "today.minutes": minutes[today], "today.sessions": count[today],
             "yesterday.minutes": minutes[yesterday], "yesterday.sessions": count[yesterday],
             "yesterday.week.minutes": week(yesterday), "before.week.minutes": week(yesterday - timedelta(days=7)),
             "picked.minutes": minutes[picked], "picked.sessions": count[picked], "hidden.today.minutes": kept}
    return sessions, "\n".join(f"{key}={value}" for key, value in facts.items())


def serve_socket(listener, process, answer):
    """The Simulator test asks for each step over a loopback port: one command a connection."""
    deadline = time.monotonic() + SYNC_TIMEOUT
    listener.settimeout(1)
    while process.poll() is None and time.monotonic() < deadline:
        try:
            client, _ = listener.accept()
        except TimeoutError:
            continue
        with client, client.makefile("rb") as request:
            client.settimeout(CONTROL_TIMEOUT)
            client.sendall(answer(request.readline(32).decode().strip()).encode())


def serve_files(device, process, answer):
    """The emulator test leaves a command in a file of the app, and waits for the answer in another."""
    deadline = time.monotonic() + SYNC_TIMEOUT
    while process.poll() is None and time.monotonic() < deadline:
        if not device.exists("sync-request"):
            time.sleep(0.3)
            continue
        command = device.read("sync-request")
        device.remove("sync-request")
        device.write("sync-reply", answer(command))


def verify_sync(args, device, dashboard=False):
    """The sync test: the program serves the data file, and the test asks the script to add a chat, stop and restart.
    With `dashboard` the file holds sessions too, and the test asks what the page must show for them (`expect`)."""
    protected = set()
    digests = set()
    sessions, expectation = dashboard_sessions() if dashboard else ([], "")
    chats = [sync_chat("c1", "Plan the trip"), sync_chat("c2", "Tea notes"), sync_chat("c3", "Garden ideas"),
             sync_chat("c4", "Reading list")]
    with tempfile.TemporaryDirectory(prefix="construct-sync-test-") as temporary, contextlib.ExitStack() as running:
        root = Path(temporary)
        directory = root / "agent"
        directory.mkdir()
        data = root / "data.json"
        write_sync_data(data, chats, sessions=sessions)
        process, _, offer = running.enter_context(agent(args.agent, device, directory, protected, data=data))
        current = [process]
        port = int(offer["address"].rsplit(":", 1)[1])

        def answer(command):
            if command == "offer" and device.offer_in_command:
                return offer["address"] + "\n" + offer["code"]
            if command == "chat":
                chats.append(sync_chat("c5", "A chat from the computer"))
                write_sync_data(data, chats, sessions=sessions)
                return "ready"
            if command == "expect" and dashboard:
                return expectation
            if command == "hide" and dashboard:
                write_sync_data(data, chats, sessions=sessions, hidden=("/w/site",))
                return "ready"
            if command == "stop":
                device.confirm_stop(directory, digests)
                send(current[0], "stop")
                current[0].wait(timeout=CONTROL_TIMEOUT)
                return "ready"
            if command == "restart":
                # The program comes back on the port the phone knows, on the loopback side of the host.
                current[0] = running.enter_context(
                    agent(args.agent, device, directory, protected, f"127.0.0.1:{port}", data)
                )[0]
                return "ready"
            raise RuntimeError(f"The app test asked for {command}.")

        device.run_sync("dashboard" if dashboard else "sync", protected, digests, answer, offer)
        device.collect_screenshots()


@contextlib.contextmanager
def test_simulator(device_type, runtime, build, theme):
    apps = list((Path(build) / "Build" / "Products").glob("*-iphonesimulator/Construct.app"))
    if len(apps) != 1:
        raise RuntimeError("Use a build-for-testing directory with one Construct Simulator app.")
    # An unsigned Simulator app has no application identifier, and the Simulator of iOS 27 then refuses it
    # every Keychain call, so the page would show a storage error instead of "Not paired yet". Signing ad hoc
    # puts the entitlements, with the application identifier, into an `__entitlements` section of the binary.
    if "__entitlements" not in read("otool", "-l", str(apps[0] / "Construct")):
        raise RuntimeError("Build the app with CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- (see the README).")
    with shared_simulator.shared_simulator(device_type, runtime, app=apps[0], theme=theme or "light") as device:
        yield device


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
    parser.add_argument("--android-theme", choices=("light", "dark"), help="emulator appearance")
    parser.add_argument("--sync", action="store_true", help="run the sync test instead of the Computer page test")
    parser.add_argument("--dashboard", action="store_true", help="run the iPhone Dashboard test instead")
    parser.add_argument("--screenshots", help="optional directory for the screenshots of the test")
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, interrupted)
    if bool(args.device) == bool(args.ios_build):
        parser.error("use --device for Android or --ios-build for the iPhone, not both")
    if args.device:
        if args.dashboard:
            parser.error("the Dashboard test runs on the iPhone only")
        if args.sync:
            verify_sync(args, Android(args.device, args.screenshots, ANDROID_SYNC_TEST, SYNC_SCREENSHOTS,
                                      args.android_theme))
        else:
            verify_computer_page(args, Android(args.device, args.screenshots, theme=args.android_theme))
        return
    if not (args.ios_device_type and args.ios_runtime):
        parser.error("the iPhone Simulator is created from --ios-device-type and --ios-runtime, and needs both")
    with test_simulator(args.ios_device_type, args.ios_runtime, args.ios_build, args.ios_theme) as device:
        if args.dashboard:
            verify_sync(args, Ios(device, args.ios_build, args.screenshots, IOS_DASHBOARD_TEST, "Dashboard"), True)
        elif args.sync:
            verify_sync(args, Ios(device, args.ios_build, args.screenshots, IOS_SYNC_TEST, "Sync"))
        else:
            verify_ios_computer_page(args, Ios(device, args.ios_build, args.screenshots))


if __name__ == "__main__":
    main()
