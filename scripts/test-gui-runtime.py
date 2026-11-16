#!/usr/bin/env python3
"""Regression checks for keeping GUI deployment independent of boot libraries."""
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("runtime", Path(__file__).with_name("package-gui-runtime.py"))
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class RuntimeTests(unittest.TestCase):
    def test_runtime_never_relocates_sd_boot_libraries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "target"
            libraries = ["libinput", "libevdev", "libudev", "libxkbcommon", "libfontconfig",
                         "libEGL", "libGLESv2", "libgbm"]
            for name in [*(f"usr/lib/{name}.so.1" for name in libraries),
                         "usr/lib/libgallium-test.so", "usr/lib/gbm/dri_gbm.so",
                         "lib/libgcc_s.so.1", "lib/libearlyboot.so.1"]:
                file = root / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(b"\x7fELFtest")
            for name in ("usr/share/libinput", "usr/share/X11/xkb", "usr/share/fontconfig", "etc/fonts"):
                (root / name).mkdir(parents=True)
            dependencies = "(NEEDED) Shared library: [libgcc_s.so.1]\n(NEEDED) Shared library: [libearlyboot.so.1]"
            archive = Path(directory) / "runtime.tar"
            with patch.object(runtime.subprocess, "check_output", return_value=dependencies):
                runtime.package(root, archive)
            with tarfile.open(archive) as packed:
                names = packed.getnames()
                self.assertIn("usr/lib/libEGL.so.1", names)
                self.assertIn("usr/lib/libgallium-test.so", names)
                self.assertIn("usr/lib/gbm/dri_gbm.so", names)
                self.assertFalse(any(name.startswith(("lib/", "lib64/")) for name in names))
            self.assertEqual((root / "lib/libgcc_s.so.1").read_bytes(), b"\x7fELFtest")

    def test_missing_runtime_fails_before_creating_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "runtime.tar"
            with self.assertRaisesRegex(ValueError, "missing libinput"):
                runtime.package(Path(directory), archive)
            self.assertFalse(archive.exists())


if __name__ == "__main__":
    unittest.main()
