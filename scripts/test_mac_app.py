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
            self.assertEqual(info["CFBundleIconFile"], app)
            self.assertEqual(info["LSMinimumSystemVersion"], "14.0")

    def test_types_macos_may_lack_are_imported(self):
        imported = {
            app: {d["UTTypeIdentifier"]: d for d in mac_app.info_plist(app)["UTImportedTypeDeclarations"]}
            for app in ("brick", "slate", "frame", "etch")
        }
        self.assertEqual(set(imported["slate"]), {"net.daringfireball.markdown"})
        self.assertEqual(
            imported["slate"]["net.daringfireball.markdown"]["UTTypeTagSpecification"]["public.filename-extension"],
            ["md", "markdown"],
        )
        self.assertEqual(set(imported["brick"]), {"org.openxmlformats.spreadsheetml.sheet.macroenabled"})
        self.assertEqual(set(imported["frame"]), {"org.openxmlformats.presentationml.presentation.macroenabled"})
        self.assertEqual(imported["etch"], {})

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

    def test_only_the_office_apps_carry_the_fonts(self):
        fonts = mac_app.ROOT / "target" / "fonts"
        if not fonts.is_dir():
            self.skipTest("no target/fonts (scripts/fetch_fonts.py)")
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            for app in ("etch", "slate"):
                (bin_dir / app).write_bytes(b"binary")
            self.assertFalse((mac_app.bundle("etch", bin_dir, tmp) / "Contents/Resources/fonts").exists())
            self.assertTrue((mac_app.bundle("slate", bin_dir, tmp) / "Contents/Resources/fonts").is_dir())


if __name__ == "__main__":
    unittest.main()
