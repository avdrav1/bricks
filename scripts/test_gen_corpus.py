"""Tests for gen_corpus.py. Run: python3 -m unittest discover -s scripts"""
import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "gen_corpus.py"


def generate(out: Path, max_mb: int) -> None:
    subprocess.run(
        [sys.executable, str(SCRIPT), "--out", str(out), "--max-mb", str(max_mb)],
        check=True,
        capture_output=True,
        # A non-UTF-8 locale must not change the bytes written.
        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C", "PYTHONUTF8": "0"},
    )


class SizedFiles(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.out = Path(cls.tmp.name)
        generate(cls.out, 10)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_reaches_target_size_and_stops(self):
        size = (self.out / "rows_10mb.csv").stat().st_size
        self.assertGreaterEqual(size, 10 * 1024 * 1024)
        self.assertLess(size, 10 * 1024 * 1024 + 200)
        self.assertFalse((self.out / "rows_100mb.csv").exists())

    def test_every_row_has_the_header_width(self):
        with (self.out / "rows_10mb.csv").open("rb") as f:
            header = f.readline()
            widths = {line.count(b",") for line in f}
        self.assertEqual(header, b"id,name,city,revenue,score,date,active,code\n")
        self.assertEqual(widths, {7})

    def test_same_seed_same_bytes(self):
        with tempfile.TemporaryDirectory() as again:
            generate(Path(again), 10)
            digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
            for name in ("rows_10mb.csv", "nasty/quoted_newlines.csv", "nasty/latin1.csv"):
                self.assertEqual(digest(self.out / name), digest(Path(again) / name), name)


class NastySet(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        generate(Path(cls.tmp.name), 0)
        cls.dir = Path(cls.tmp.name) / "nasty"

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def read(self, name: str) -> bytes:
        return (self.dir / name).read_bytes()

    def test_encodings_are_exact_bytes(self):
        self.assertEqual(self.read("bom.csv"), b"\xef\xbb\xbfa,b\n1,2\n")
        self.assertEqual(self.read("utf16le.csv"), b"\xff\xfe" + "a,b\n1,2\n".encode("utf-16-le"))
        self.assertEqual(self.read("latin1.csv"), b"name\nJos\xe9\nM\xfcller\n")

    def test_line_endings_survive(self):
        self.assertEqual(self.read("crlf.csv"), b"a,b\r\n1,2\r\n3,4\r\n")
        self.assertEqual(self.read("no_trailing_newline.csv"), b"a,b\n1,2")
        self.assertIn(b'"line one\nline two"', self.read("quoted_newlines.csv"))

    def test_edge_shapes_present(self):
        self.assertEqual(self.read("empty.csv"), b"")
        self.assertEqual(self.read("leading_zeros.csv"), b"zip,code\n02134,00123\n07030,000\n")
        expected = {
            "bom.csv", "crlf.csv", "empty.csv", "header_only.csv", "latin1.csv",
            "leading_zeros.csv", "no_trailing_newline.csv", "pipe.csv", "quoted_newlines.csv",
            "ragged.csv", "semicolon.csv", "tab.tsv", "utf16le.csv",
        }
        self.assertEqual({p.name for p in self.dir.iterdir()}, expected)


if __name__ == "__main__":
    unittest.main()
