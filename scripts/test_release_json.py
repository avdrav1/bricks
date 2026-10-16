"""Tests for release_json.py. Run: python3 -m unittest discover -s scripts"""
import hashlib
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_json  # noqa: E402

URL = "https://dl.example.com/slate/releases/v1.2.3"


class LatestJson(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.dist = Path(self.dir.name)
        self.linux = self.put("slate-1.2.3-x86_64-linux.tar.gz", b"linux tarball")
        self.mac_tar = self.dist / "slate-1.2.3-arm64-macos.tar.gz"
        self.mac_dmg = self.dist / "slate-1.2.3-arm64-macos.dmg"
        self.setup = self.dist / "slate-1.2.3-x86_64-windows-setup.exe"

    def tearDown(self):
        self.dir.cleanup()

    def put(self, name, data):
        (self.dist / name).write_bytes(data)
        return self.dist / name

    def release(self):
        return release_json.release(URL, "1.2.3", str(self.linux), str(self.mac_tar), str(self.mac_dmg),
                                    str(self.setup), "2026-10-16")

    def test_every_platform_with_linux_at_the_top_level_last(self):
        self.put(self.mac_tar.name, b"mac tarball")
        self.put(self.mac_dmg.name, b"mac dmg")
        self.put(self.setup.name, b"MZ setup")
        rel = self.release()
        self.assertEqual(list(rel), ["platforms", "version", "date", "url", "sha256", "bytes"])
        self.assertEqual(list(rel["platforms"]), ["linux-x86_64", "macos-arm64", "windows-x86_64"])
        # install.ps1 reads exactly these three (WIN-5).
        self.assertEqual(rel["platforms"]["windows-x86_64"], {
            "url": f"{URL}/slate-1.2.3-x86_64-windows-setup.exe",
            "sha256": hashlib.sha256(b"MZ setup").hexdigest(),
            "bytes": 8,
        })
        mac = rel["platforms"]["macos-arm64"]
        self.assertEqual(mac["url"], f"{URL}/slate-1.2.3-arm64-macos.tar.gz")
        self.assertEqual(mac["dmg"], f"{URL}/slate-1.2.3-arm64-macos.dmg")
        self.assertEqual(mac["dmg_bytes"], 7)
        for key in ("url", "sha256", "bytes"):
            self.assertEqual(rel[key], rel["platforms"]["linux-x86_64"][key])
        self.assertEqual(release_json.check(rel, str(self.dist)), [])

    def test_a_platform_not_built_is_left_out(self):
        self.put(self.setup.name, b"MZ setup")
        rel = self.release()
        self.assertEqual(list(rel["platforms"]), ["linux-x86_64", "windows-x86_64"])
        self.put(self.mac_tar.name, b"mac tarball")  # the DMG is still missing
        self.assertNotIn("macos-arm64", self.release()["platforms"])
        self.setup.unlink()
        self.assertEqual(list(self.release()["platforms"]), ["linux-x86_64"])

    def test_the_old_installers_sed_still_finds_linux(self):
        # install.sh before MAC-7: sed -n 's/.*"url":"\([^"]*\)".*/\1/p', greedy, so the last match.
        self.put(self.mac_tar.name, b"mac tarball")
        self.put(self.mac_dmg.name, b"mac dmg")
        self.put(self.setup.name, b"MZ setup")
        line = release_json.dumps(self.release())
        self.assertNotIn("\n", line)
        self.assertEqual(re.sub(r'.*"url":"([^"]*)".*', r"\1", line), str(URL + "/" + self.linux.name))
        self.assertEqual(json.loads(line), self.release())

    def test_check_finds_a_changed_or_missing_file(self):
        self.put(self.setup.name, b"MZ setup")
        rel = self.release()
        self.put(self.setup.name, b"MZ setup, rebuilt")
        self.assertEqual(release_json.check(rel, str(self.dist)),
                         ["windows-x86_64: " + str(self.setup) + " doesn't match its sha256 and size"])
        self.setup.unlink()
        self.assertEqual(release_json.check(rel, str(self.dist)), ["windows-x86_64: " + str(self.setup) + " missing"])

    def test_check_wants_linux_last_and_each_file_in_the_versions_folder(self):
        rel = self.release()
        moved = {"version": rel["version"], "url": rel["url"], "sha256": rel["sha256"], "bytes": rel["bytes"],
                 "platforms": rel["platforms"]}
        self.assertIn("the top-level url, sha256, and bytes aren't last", release_json.check(moved, str(self.dist)))
        other = dict(rel, version="1.2.4")
        self.assertEqual(release_json.check(other, str(self.dist)),
                         [f"linux-x86_64: {rel['url']} isn't in v1.2.4's folder"])


if __name__ == "__main__":
    unittest.main()
