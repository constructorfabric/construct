#!/usr/bin/env python3
"""Checks that the device log check of test-phone-pairing.py fails when a protected value is in the log."""

import contextlib
import hashlib
import importlib.util
import io
from pathlib import Path
import unittest

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


def check(device):
    with contextlib.redirect_stdout(io.StringIO()):
        harness.verify_device_logs(device, PROTECTED, DIGESTS)


class DeviceLogCheck(unittest.TestCase):
    def test_a_clean_log_with_the_canary_passes(self):
        check(Device("I ConstructApp: started", CANARY_LINE, f"I other: {'12' * 32}"))

    def test_a_log_without_the_canary_fails(self):
        with self.assertRaisesRegex(RuntimeError, "missed its planted log marker"):
            check(Device("I ConstructApp: started"))

    def test_each_planted_plain_value_fails(self):
        for name, value in (("code", CODE), ("fingerprint", FINGERPRINT), ("QR secret", QR_SECRET)):
            with self.subTest(name), self.assertRaisesRegex(RuntimeError, "protected pairing material"):
                check(Device(CANARY_LINE, f"D app: saw {value} in a line"))

    def test_the_token_fails_in_either_case_and_inside_other_text(self):
        for text in (TOKEN, TOKEN.upper(), f"token={TOKEN}&x=1", f"0{TOKEN}"):
            with self.subTest(text), self.assertRaisesRegex(RuntimeError, "protected pairing material"):
                check(Device(CANARY_LINE, f"D app: {text}"))


if __name__ == "__main__":
    unittest.main()
