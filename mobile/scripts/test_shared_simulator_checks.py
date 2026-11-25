#!/usr/bin/env python3
"""Checks that shared_simulator.py keeps one Simulator, leaves it as new before a run and shuts it down after it.

It runs no Simulator: `simctl` is replaced by a fake that keeps a list of devices and the calls it was given.
"""

import fcntl
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("shared_simulator", Path(__file__).with_name("shared_simulator.py"))
assert SPEC is not None and SPEC.loader is not None
shared = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(shared)

TYPE = "com.apple.CoreSimulator.SimDeviceType.iPhone-17"
RUNTIME = "com.apple.CoreSimulator.SimRuntime.iOS-27-2"
OTHER_RUNTIME = "com.apple.CoreSimulator.SimRuntime.iOS-26-0"
BUNDLE = "example.construct"


def device(udid, name=shared.NAME, runtime=RUNTIME, available=True, state="Shutdown"):
    return {"udid": udid, "name": name, "state": state, "isAvailable": available, "deviceTypeIdentifier": TYPE,
            "runtime": runtime}


class FakeSimctl:
    """Answers the simctl commands of shared_simulator.py and keeps the calls, in order."""

    def __init__(self, *devices, app_stays=False, shutdown_fails=False):
        self.devices = list(devices)
        self.calls = []
        self.app_installed = True
        self.app_stays = app_stays
        self.shutdown_fails = shutdown_fails

    def __call__(self, *args, timeout=None, check=True, **options):
        del timeout, check, options
        self.calls.append(args)
        stdout = ""
        returncode = 0
        if args[:3] == ("list", "-j", "devices"):
            runtimes = {}
            for found in self.devices:
                runtimes.setdefault(found["runtime"], []).append(found)
            stdout = json.dumps({"devices": runtimes})
        elif args[0] == "create":
            self.devices.append(device("NEW-UDID", name=args[1], runtime=args[3]))
            stdout = "NEW-UDID\n"
        elif args[0] == "delete":
            self.devices = [found for found in self.devices if found["udid"] != args[1]]
        elif args[0] == "bootstatus":
            self.state_of(args[1])["state"] = "Booted"
        elif args[0] == "shutdown" and not self.shutdown_fails:
            self.state_of(args[1])["state"] = "Shutdown"
        elif args[0] == "uninstall" and not self.app_stays:
            self.app_installed = False
        elif args[0] == "install":
            self.app_installed = True
        elif args[0] == "get_app_container":
            returncode = 0 if self.app_installed else 2
        return subprocess.CompletedProcess(args, returncode, stdout=stdout)

    def state_of(self, udid):
        return next(found for found in self.devices if found["udid"] == udid)

    def commands(self):
        return [call[0] for call in self.calls if call[0] != "list"]


class SharedSimulatorTest(unittest.TestCase):
    def use(self, fake):
        patcher = mock.patch.object(shared, "simctl", fake)
        patcher.start()
        self.addCleanup(patcher.stop)
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        lock = mock.patch.object(shared, "LOCK", Path(folder.name) / "lock")
        lock.start()
        self.addCleanup(lock.stop)
        sleeping = mock.patch.object(shared.time, "sleep")
        sleeping.start()
        self.addCleanup(sleeping.stop)
        return fake

    def test_the_first_run_creates_the_simulator_and_later_runs_find_it(self):
        fake = self.use(FakeSimctl(device("OTHER", name="iPhone 17")))
        self.assertEqual(shared.find_or_create(TYPE, RUNTIME), "NEW-UDID")
        self.assertEqual(shared.find_or_create(TYPE, RUNTIME), "NEW-UDID")
        self.assertEqual(fake.commands(), ["create"])
        self.assertEqual(sorted(found["udid"] for found in fake.devices), ["NEW-UDID", "OTHER"])

    def test_a_simulator_of_another_runtime_or_an_unavailable_one_is_replaced(self):
        fake = self.use(FakeSimctl(device("OLD", runtime=OTHER_RUNTIME), device("GONE", available=False),
                                   device("OTHER", name="iPhone 17")))
        self.assertEqual(shared.find_or_create(TYPE, RUNTIME), "NEW-UDID")
        self.assertEqual(sorted(found["udid"] for found in fake.devices), ["NEW-UDID", "OTHER"])

    def test_only_one_simulator_of_the_name_is_kept(self):
        fake = self.use(FakeSimctl(device("FIRST"), device("SECOND")))
        self.assertEqual(shared.find_or_create(TYPE, RUNTIME), "FIRST")
        self.assertEqual([found["udid"] for found in fake.devices], ["FIRST"])

    def test_a_run_leaves_the_simulator_as_new_then_shuts_it_down_and_keeps_it(self):
        fake = self.use(FakeSimctl(device("KEPT")))
        with shared.shared_simulator(TYPE, RUNTIME, BUNDLE, app=Path("Construct.app"), theme="dark",
                                     certificate=Path("trusted.pem")) as udid:
            self.assertEqual(udid, "KEPT")
            self.assertEqual(fake.state_of("KEPT")["state"], "Booted")
        self.assertEqual(fake.state_of("KEPT")["state"], "Shutdown")
        # The Keychain is reset before the certificate is added, and the app is removed before it is installed.
        self.assertEqual(
            fake.commands(),
            ["bootstatus", "spawn", "keychain", "uninstall", "get_app_container", "install", "ui", "status_bar",
             "keychain", "shutdown"])
        keychain = [call for call in fake.calls if call[0] == "keychain"]
        self.assertEqual([call[2] for call in keychain], ["reset", "add-root-cert"])
        self.assertIn(("ui", "KEPT", "appearance", "dark"), fake.calls)
        self.assertEqual([found["udid"] for found in fake.devices], ["KEPT"])

    def test_a_run_without_a_build_removes_the_app_and_installs_nothing(self):
        fake = self.use(FakeSimctl(device("KEPT")))
        with shared.shared_simulator(TYPE, RUNTIME, BUNDLE):
            self.assertFalse(fake.app_installed)
        self.assertNotIn("install", fake.commands())
        self.assertIn(("ui", "KEPT", "appearance", "light"), fake.calls)

    def test_the_run_stops_when_the_app_is_still_there_after_it_was_removed(self):
        fake = self.use(FakeSimctl(device("KEPT"), app_stays=True))
        with self.assertRaisesRegex(RuntimeError, "still installed"):
            with shared.shared_simulator(TYPE, RUNTIME, BUNDLE):
                self.fail("the block must not run")
        self.assertEqual(fake.state_of("KEPT")["state"], "Shutdown")

    def test_the_simulator_is_shut_down_when_the_run_fails(self):
        fake = self.use(FakeSimctl(device("KEPT")))
        with self.assertRaisesRegex(ValueError, "the run failed"):
            with shared.shared_simulator(TYPE, RUNTIME, BUNDLE):
                raise ValueError("the run failed")
        self.assertEqual(fake.state_of("KEPT")["state"], "Shutdown")

    def test_a_simulator_that_stays_up_fails_a_good_run_and_does_not_hide_a_failed_one(self):
        self.use(FakeSimctl(device("KEPT"), shutdown_fails=True))
        with self.assertRaisesRegex(RuntimeError, "still Booted"):
            with shared.shared_simulator(TYPE, RUNTIME, BUNDLE):
                pass
        with self.assertRaisesRegex(ValueError, "the run failed"):
            with shared.shared_simulator(TYPE, RUNTIME, BUNDLE):
                raise ValueError("the run failed")

    def test_a_second_run_waits_for_the_first(self):
        self.use(FakeSimctl(device("KEPT")))
        shared.LOCK.parent.mkdir(parents=True, exist_ok=True)
        with shared.LOCK.open("a") as first:
            fcntl.flock(first, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with mock.patch.object(shared, "LOCK_TIMEOUT", -1), self.assertRaisesRegex(RuntimeError, "Another run"):
                with shared.alone():
                    self.fail("the lock is held")
        with shared.alone():
            pass


if __name__ == "__main__":
    unittest.main()
