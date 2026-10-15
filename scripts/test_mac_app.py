"""Tests for mac_app.py. Run: python3 -m unittest discover -s scripts"""
import plistlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mac_app  # noqa: E402


class InfoPlist(unittest.TestCase):
    def test_each_app_has_its_identifier_and_file_types(self):
        expected = {
            "brick": ("Brick", {"public.comma-separated-values-text", "org.openxmlformats.spreadsheetml.sheet"}),
            "slate": ("Slate", {"public.plain-text", "net.daringfireball.markdown", "org.openxmlformats.wordprocessingml.document"}),
            "frame": ("Frame", {"org.openxmlformats.presentationml.presentation"}),
            "etch": ("Etch", {"public.png", "public.jpeg", "com.microsoft.bmp"}),
        }
        for app, (name, utis) in expected.items():
            info = mac_app.info_plist(app)
            self.assertEqual(info["CFBundleIdentifier"], f"dev.cronkite.{name}")
            self.assertEqual(info["CFBundleExecutable"], app)
            self.assertEqual(info["CFBundlePackageType"], "APPL")
            types = info["CFBundleDocumentTypes"][0]["LSItemContentTypes"]
            self.assertTrue(utis <= set(types), f"{app}: {types}")
            self.assertRegex(info["CFBundleShortVersionString"], r"^\d+\.\d+\.\d+$")

    def test_the_bundle_holds_the_binary_and_the_plist(self):
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            (bin_dir / "frame").write_bytes(b"binary")
            path = mac_app.bundle("frame", bin_dir, tmp)
            self.assertEqual(path, Path(tmp) / "Frame.app")
            self.assertEqual((path / "Contents/MacOS/frame").read_bytes(), b"binary")
            with open(path / "Contents/Info.plist", "rb") as f:
                self.assertEqual(plistlib.load(f)["CFBundleName"], "Frame")


if __name__ == "__main__":
    unittest.main()
