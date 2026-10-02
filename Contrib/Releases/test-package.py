#!/usr/bin/env python3
"""Regression checks for native snapshot packaging failures."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("snapshot_package", Path(__file__).with_name("package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class DiskImageTests(unittest.TestCase):
    def create(self, responses):
        calls = []

        def execute(command, **options):
            calls.append(command)
            code, error = responses.pop(0)
            result = subprocess.CompletedProcess(command, code, "", error)
            if options.get("check"):
                result.check_returncode()
            return result

        with patch.object(package.subprocess, "run", side_effect=execute), patch.object(package.time, "sleep"), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            try:
                package.create_disk_image(Path("synthetic app with spaces"), Path("synthetic image.dmg"))
            except subprocess.CalledProcessError as error:
                return calls, error
        return calls, None

    def test_busy_creation_retries_and_requires_verification(self):
        calls, error = self.create([(1, "hdiutil: create failed - Resource busy\n"), (1, "Resource busy"), (0, ""), (0, "")])
        self.assertIsNone(error)
        self.assertEqual([command[1] for command in calls], ["create", "create", "create", "verify"])
        self.assertEqual(calls[0][-1], "synthetic image.dmg")
        self.assertEqual(calls[0][5], "synthetic app with spaces")

    def test_other_errors_fail_immediately(self):
        calls, error = self.create([(1, "Permission denied")])
        self.assertIsNotNone(error)
        self.assertEqual(len(calls), 1)

    def test_persistent_busy_failure_is_bounded(self):
        calls, error = self.create([(1, "Resource busy")] * 3)
        self.assertIsNotNone(error)
        self.assertEqual([command[1] for command in calls], ["create"] * 3)

    def test_invalid_image_is_rejected_after_creation(self):
        calls, error = self.create([(0, ""), (1, "Image verification failed")])
        self.assertIsNotNone(error)
        self.assertEqual([command[1] for command in calls], ["create", "verify"])


if __name__ == "__main__":
    unittest.main()
