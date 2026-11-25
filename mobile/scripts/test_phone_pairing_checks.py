#!/usr/bin/env python3
"""Checks that the device log check of test-phone-pairing.py fails when a protected value is in the log."""

import contextlib
from datetime import datetime
import hashlib
import importlib.util
import io
from pathlib import Path
import unittest
from zoneinfo import ZoneInfo

SPEC = importlib.util.spec_from_file_location("phone_pairing_harness", Path(__file__).with_name("test-phone-pairing.py"))
harness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(harness)

CODE = "48151623"
FINGERPRINT = "ab" * 32
QR_SECRET = "cd" * 16
TOKEN = "ef" * 32
PROTECTED = {CODE, FINGERPRINT, QR_SECRET}
DIGESTS = {hashlib.sha256(TOKEN.encode()).hexdigest()}
CANARY_LINE = f"I PhonePairingFixture: {harness.CANARY}"


class Device:
    def __init__(self, *lines):
        self.text = "\n".join(lines)

    def logs(self):
        return self.text


MARKER = "Computer page fixture started"


def check(device, marker=None):
    with contextlib.redirect_stdout(io.StringIO()):
        harness.verify_device_logs(device, PROTECTED, DIGESTS, marker=marker)


class DeviceLogCheck(unittest.TestCase):
    def test_a_clean_log_with_the_canary_passes(self):
        check(Device("I ConstructApp: started", CANARY_LINE, f"I other: {'12' * 32}"))

    def test_a_log_without_the_canary_fails(self):
        with self.assertRaisesRegex(RuntimeError, "missed its planted log marker"):
            check(Device("I ConstructApp: started"))

    def test_a_log_with_the_marker_passes_and_one_without_it_fails(self):
        check(Device(f"Default 0x0 Construct: {MARKER}"), marker=MARKER)
        with self.assertRaisesRegex(RuntimeError, "missed its planted log marker"):
            check(Device("Default 0x0 Construct: started"), marker=MARKER)

    def test_a_planted_value_fails_beside_the_marker(self):
        with self.assertRaisesRegex(RuntimeError, "protected pairing material"):
            check(Device(f"Default 0x0 Construct: {MARKER}", f"Info 0x0 Construct: code {CODE}"), marker=MARKER)

    def test_each_planted_plain_value_fails(self):
        for name, value in (("code", CODE), ("fingerprint", FINGERPRINT), ("QR secret", QR_SECRET)):
            with self.subTest(name), self.assertRaisesRegex(RuntimeError, "protected pairing material"):
                check(Device(CANARY_LINE, f"D app: saw {value} in a line"))

    def test_the_token_fails_in_either_case_and_inside_other_text(self):
        for text in (TOKEN, TOKEN.upper(), f"token={TOKEN}&x=1", f"0{TOKEN}"):
            with self.subTest(text), self.assertRaisesRegex(RuntimeError, "protected pairing material"):
                check(Device(CANARY_LINE, f"D app: {text}"))


class DashboardSessionsCheck(unittest.TestCase):
    """The sessions of the Dashboard test, and what the test must find for them, whatever day the test runs."""

    @staticmethod
    def facts(year, month, day):
        now = datetime(year, month, day, 12, tzinfo=ZoneInfo(harness.COMPUTER_ZONE))
        sessions, text = harness.dashboard_sessions(now)
        return sessions, dict(line.split("=") for line in text.splitlines())

    def test_a_wednesday_counts_the_week_of_yesterday_and_the_one_before(self):
        sessions, facts = self.facts(2026, 11, 25)
        self.assertEqual([len(session["minutes"]) for session in sessions], [45, 20, 30, 50, 25])
        self.assertEqual(facts["today"], "2026-11-25")
        self.assertEqual((facts["today.minutes"], facts["today.sessions"]), ("65", "2"))
        self.assertEqual((facts["yesterday.minutes"], facts["yesterday.week.minutes"]), ("30", "95"))
        picked = (facts["picked"], facts["picked.minutes"], facts["before.week.minutes"])
        self.assertEqual(picked, ("2026-11-18", "50", "75"))
        self.assertEqual(facts["hidden.today.minutes"], "45")

    def test_a_monday_takes_yesterday_from_the_week_before(self):
        _, facts = self.facts(2026, 11, 23)
        self.assertEqual(facts["yesterday"], "2026-11-22")
        self.assertEqual(facts["yesterday.minutes"], "30")
        # Yesterday is a Sunday of the week before, so that week holds it and the two days after the 16th.
        self.assertEqual(facts["yesterday.week.minutes"], "105")
        self.assertEqual(facts["before.week.minutes"], "0")

    def test_the_minutes_are_utc_minutes_of_the_computers_days(self):
        sessions, _ = self.facts(2026, 11, 25)
        # 10:00 in Berlin is 09:00 UTC in November, and the first session counts 45 minutes from there.
        self.assertEqual(sessions[0]["minutes"][0], "2026-11-25T09:00:00Z")
        self.assertEqual(sessions[0]["minutes"][-1], "2026-11-25T09:44:00Z")
        self.assertEqual({session["project"] for session in sessions}, {"/w/app", "/w/site"})


if __name__ == "__main__":
    unittest.main()
