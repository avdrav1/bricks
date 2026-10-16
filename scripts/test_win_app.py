"""Tests for win_app.py. Run: python3 -m unittest discover -s scripts"""
import struct
import sys
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import win_app  # noqa: E402


def png(side):
    """A minimal square PNG of `side` pixels."""
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = b"".join(b"\0" + b"\0\0\0\xff" * side for _ in range(side))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", side, side, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


class Installer(unittest.TestCase):
    def test_each_apps_types_come_from_its_desktop_entry(self):
        exts = {app: [e for e, _ in win_app.extensions(app)] for app in ("brick", "slate", "frame", "etch")}
        self.assertEqual(exts["brick"], [".csv", ".tsv", ".xlsx", ".xlsm"])
        self.assertEqual(exts["slate"], [".docx", ".md", ".markdown", ".txt"])
        self.assertEqual(exts["frame"], [".pptx", ".pptm"])
        self.assertEqual(exts["etch"], [".png", ".jpg", ".jpeg", ".bmp"])

    def test_the_include_defines_the_app_and_registers_its_types_for_open_with(self):
        text = win_app.iss("slate")
        self.assertIn('#define App "slate"', text)
        self.assertIn('#define Name "Slate"', text)
        self.assertIn('#define AppId "dev.cronkite.Slate"', text)
        self.assertIn(f'#define Version "{win_app.mac_app.version("slate")}"', text)
        self.assertIn('#define Url "https://slate.cronkite.cc"', text)
        lines = text.splitlines()
        registry = lines[lines.index("[Registry]") + 1:]
        self.assertTrue(all(line.startswith("Root: HKCU; ") for line in registry), "per user only")
        # The ProgID opens the file with the installed exe; Open with lists it.
        self.assertIn('Root: HKCU; Subkey: "Software\\Classes\\Cronkite.Slate.docx\\shell\\open\\command"; '
                      'ValueType: string; ValueName: ""; ValueData: """{app}\\bin\\slate.exe"" ""%1"""', registry)
        self.assertIn('Root: HKCU; Subkey: "Software\\Classes\\.md\\OpenWithProgids"; ValueType: none; '
                      'ValueName: "Cronkite.Slate.md"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty', registry)
        self.assertIn('Root: HKCU; Subkey: "Software\\Classes\\Applications\\slate.exe\\SupportedTypes"; '
                      'ValueType: string; ValueName: ".txt"; ValueData: ""', registry)
        self.assertIn('Root: HKCU; Subkey: "Software\\RegisteredApplications"; ValueType: string; '
                      'ValueName: "Cronkite Slate"; ValueData: "Software\\Cronkite\\Slate\\Capabilities"; '
                      'Flags: uninsdeletevalue uninsdeletekeyifempty', registry)
        # Never the extension's default: Windows keeps the user's.
        for ext in (".docx", ".md", ".markdown", ".txt"):
            ext_key = f'Subkey: "Software\\Classes\\{ext}";'
            own = [line for line in registry if ext_key in line]
            self.assertEqual(own, [f"Root: HKCU; {ext_key} Flags: uninsdeletekeyifempty"])
        # Every key the installer owns goes at uninstall; shared ones only when empty.
        for key in ("Software\\Cronkite\\Slate", "Software\\Classes\\Applications\\slate.exe",
                    "Software\\Classes\\Cronkite.Slate.txt"):
            self.assertIn(f'Root: HKCU; Subkey: "{key}"; Flags: uninsdeletekey', registry)
        self.assertIn('Root: HKCU; Subkey: "Software\\Cronkite"; Flags: uninsdeletekeyifempty', registry)
        self.assertIn('Root: HKCU; Subkey: "Software\\Classes\\Applications"; Flags: uninsdeletekeyifempty', registry)

    def test_an_icon_holds_each_png_smallest_first(self):
        images = [png(256), png(16), png(48)]
        data = win_app.ico(images)
        reserved, kind, count = struct.unpack("<HHH", data[:6])
        self.assertEqual((reserved, kind, count), (0, 1, 3))
        entries = [struct.unpack("<BBBBHHII", data[6 + 16 * i:22 + 16 * i]) for i in range(3)]
        self.assertEqual([e[0] for e in entries], [16, 48, 0])  # 0 is 256
        for (w, _h, _c, _r, planes, bits, size, offset), image in zip(entries, [images[1], images[2], images[0]]):
            self.assertEqual((planes, bits), (1, 32))
            self.assertEqual(data[offset:offset + size], image)
        with self.assertRaises(SystemExit):
            win_app.ico([png(512)])


if __name__ == "__main__":
    unittest.main()
