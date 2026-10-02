#!/usr/bin/env python3
"""Check real release inventory selection with synthetic packages, without keys."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("sign-release.py")
spec = importlib.util.spec_from_file_location("sign_release", SCRIPT)
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class ReleaseInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="magicalcryptowallet-inventory-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def package(self, version="2.3.4", suffix=".msi"):
        path = self.directory / f"MagicalCryptoWallet-{version}{suffix}"
        path.write_bytes(b"Synthetic release inventory, no executable or wallet")
        return path

    def test_all_fourteen_targets_and_both_version_formats(self):
        for version in ("2.3.4", "2.3.4.5"):
            with self.subTest(version=version):
                files = [self.package(version, suffix) for suffix in signing.SUFFIXES]
                self.assertEqual(len(files), 14)
                self.assertEqual(signing.select_packages(self.directory, version), sorted(files))
                for path in files:
                    path.unlink()

    def test_mixed_versions_fail_before_manifest_or_key_access(self):
        self.package()
        self.package("2.3.3", ".deb")
        manifest = self.directory / "SHA256SUMS"
        manifest.write_text("Previous verified manifest\n")
        result = subprocess.run([sys.executable, str(SCRIPT), str(self.directory), "--version", "2.3.4"],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("different release version", result.stderr)
        self.assertEqual(manifest.read_text(), "Previous verified manifest\n")
        self.assertFalse((self.directory / "SHA256SUMS.asc").exists())

    def test_version_prefix_collision_is_rejected(self):
        self.package("2.3.4.5")
        with self.assertRaises(RuntimeError):
            signing.select_packages(self.directory, "2.3.4")

    def test_only_old_packages_are_rejected(self):
        self.package("2.3.3")
        with self.assertRaises(RuntimeError):
            signing.select_packages(self.directory, "2.3.4")

    def test_orphan_signatures_and_unrelated_files_are_excluded(self):
        package = self.package()
        (self.directory / "MagicalCryptoWallet-2.3.3.msi.asc").write_text("Synthetic signature")
        (self.directory / "MagicalCryptoWallet-2.3.4-notes.txt").write_text("Release notes")
        (self.directory / "unrelated.zip").write_bytes(b"Synthetic unrelated archive")
        self.assertEqual(signing.select_packages(self.directory, "2.3.4"), [package])

    def test_unknown_package_target_is_rejected(self):
        self.package(suffix="-unsupported.zip")
        with self.assertRaises(RuntimeError):
            signing.select_packages(self.directory, "2.3.4")

    def test_version_is_required(self):
        result = subprocess.run([sys.executable, str(SCRIPT), str(self.directory)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--version", result.stderr)

    def test_empty_inventory_and_invalid_versions_are_rejected(self):
        for version in ("2.3.4", "2.3", "v2.3.4", "02.3.4", "2.3.4\n", "2147483648.3.4"):
            with self.subTest(version=version), self.assertRaises(RuntimeError):
                signing.select_packages(self.directory, version)


if __name__ == "__main__":
    unittest.main()
