"""Tests for fetch_fonts.py, offline. Run: python3 -m unittest discover -s scripts"""
import hashlib
import io
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fetch_fonts  # noqa: E402


def sha(data):
    return hashlib.sha256(data).hexdigest()


def tarball(members):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for name, data in members.items():
            info = tarfile.TarInfo(fetch_fonts.LIBERATION_DIR + name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


class Fetch(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dest = Path(self.tmp.name) / "fonts"
        carlito, licence, sans = b"carlito face", b"OFL text", b"liberation sans face"
        self.archive = tarball({"LiberationSans-Regular.ttf": sans, "LICENSE": b"licence"})
        self.served = {"u/carlito": carlito, "u/ofl": licence, fetch_fonts.LIBERATION: self.archive}
        self.asked = []
        patches = [
            mock.patch.dict(fetch_fonts.FILES, clear=True, values={
                "Carlito-Regular.ttf": ("u/carlito", sha(carlito)),
                "OFL-Carlito.txt": ("u/ofl", sha(licence)),
            }),
            mock.patch.dict(fetch_fonts.LIBERATION_FILES, clear=True, values={
                "LiberationSans-Regular.ttf": ("LiberationSans-Regular.ttf", sha(sans)),
                "LICENSE-Liberation.txt": ("LICENSE", sha(b"licence")),
            }),
            mock.patch.object(fetch_fonts, "LIBERATION_SHA256", sha(self.archive)),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def tearDown(self):
        self.tmp.cleanup()

    def get(self, url):
        self.asked.append(url)
        return self.served[url]

    def test_fetches_every_file_with_its_licence(self):
        self.assertEqual(fetch_fonts.fetch(self.dest, self.get), 4)
        names = sorted(p.name for p in self.dest.iterdir())
        self.assertEqual(names, ["Carlito-Regular.ttf", "LICENSE-Liberation.txt",
                                 "LiberationSans-Regular.ttf", "OFL-Carlito.txt"])
        self.assertEqual((self.dest / "LiberationSans-Regular.ttf").read_bytes(), b"liberation sans face")

    def test_files_already_there_are_not_fetched_again(self):
        fetch_fonts.fetch(self.dest, self.get)
        self.asked.clear()
        self.assertEqual(fetch_fonts.fetch(self.dest, self.get), 0)
        self.assertEqual(self.asked, [])
        # A changed file is fetched again; only the tarball's missing member is taken.
        (self.dest / "LICENSE-Liberation.txt").write_bytes(b"edited")
        self.assertEqual(fetch_fonts.fetch(self.dest, self.get), 1)
        self.assertEqual(self.asked, [fetch_fonts.LIBERATION])
        self.assertEqual((self.dest / "LICENSE-Liberation.txt").read_bytes(), b"licence")

    def test_a_wrong_checksum_stops_without_writing_the_file(self):
        self.served["u/carlito"] = b"tampered"
        with self.assertRaises(SystemExit) as stop:
            fetch_fonts.fetch(self.dest, self.get)
        self.assertIn("Carlito-Regular.ttf", str(stop.exception))
        self.assertFalse((self.dest / "Carlito-Regular.ttf").exists())
        self.assertFalse((self.dest / "Carlito-Regular.ttf.part").exists())


class RealList(unittest.TestCase):
    def test_three_families_and_their_licences(self):
        names = set(fetch_fonts.FILES) | set(fetch_fonts.LIBERATION_FILES)
        for family in ("Carlito", "Caladea", "LiberationSans", "LiberationSerif", "LiberationMono"):
            for style in ("Regular", "Bold", "Italic", "BoldItalic"):
                self.assertIn(f"{family}-{style}.ttf", names)
        self.assertTrue({"OFL-Carlito.txt", "OFL-Caladea.txt", "LICENSE-Liberation.txt"} <= names)
        self.assertEqual(len(names), 23)


if __name__ == "__main__":
    unittest.main()
