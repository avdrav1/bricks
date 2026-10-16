"""Tests for mac_licenses.py. Run: python3 -m unittest discover -s scripts"""
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mac_licenses  # noqa: E402


class Licences(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.cellar = root / "Cellar"
        keg = self.cellar / "glib" / "2.86.0_1"
        (keg / "lib").mkdir(parents=True)
        (keg / "lib" / "libglib-2.0.0.dylib").write_bytes(b"lib")
        (keg / "COPYING").write_text("LGPL")
        (keg / "README").write_text("not a licence")
        (keg / "share" / "doc" / "glib").mkdir(parents=True)
        (keg / "share" / "doc" / "glib" / "LICENSE.md").write_text("doc licence")
        (root / "opt").mkdir()
        os.symlink(keg, root / "opt" / "glib")
        self.keg = keg
        self.root = root

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_file_maps_to_its_formula_and_version_through_opt(self):
        lib = self.root / "opt" / "glib" / "lib" / "libglib-2.0.0.dylib"
        self.assertEqual(mac_licenses.cellar_formula(lib, self.cellar), ("glib", "2.86.0_1"))
        self.assertEqual(mac_licenses.cellar_formula(self.root / "opt" / "glib", self.cellar), ("glib", "2.86.0_1"))
        self.assertIsNone(mac_licenses.cellar_formula("/usr/lib/libSystem.B.dylib", self.cellar))
        self.assertIsNone(mac_licenses.cellar_formula(self.cellar, self.cellar))

    def test_licence_files_are_found_in_the_keg_and_its_docs(self):
        found = [os.path.relpath(p, self.keg) for p in mac_licenses.licence_files(self.keg)]
        self.assertEqual(found, ["COPYING", os.path.join("share", "doc", "glib", "LICENSE.md")])
        for name in ["COPYING.LIB", "LICENSE", "licence.txt", "NOTICE", "COPYRIGHT"]:
            self.assertTrue(mac_licenses.is_licence(name), name)
        for name in ["README", "AUTHORS", "INSTALL", "ChangeLog"]:
            self.assertFalse(mac_licenses.is_licence(name), name)

    def test_a_source_tarballs_licences_are_its_top_level_and_reuse_files(self):
        import io
        import tarfile
        tarball = self.root / "glib-2.88.3.tar.xz"
        with tarfile.open(tarball, "w:xz") as tar:
            for name, data in [
                ("glib-2.88.3/COPYING", b"see LICENSES"),
                ("glib-2.88.3/README.md", b"readme"),
                ("glib-2.88.3/LICENSES/LGPL-2.1-or-later.txt", b"LGPL text"),
                ("glib-2.88.3/gio/COPYING", b"deeper, not taken"),
            ]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
        self.assertEqual(
            mac_licenses.source_licences(tarball),
            [("COPYING", b"see LICENSES"), ("LGPL-2.1-or-later.txt", b"LGPL text")],
        )

    def test_the_notice_names_each_formula_its_licence_source_and_files(self):
        entries = {
            "glib": {
                "version": "2.86.0_1", "stable": "2.86.0", "spdx": "LGPL-2.1-or-later",
                "source": "https://download.gnome.org/sources/glib/2.86/glib-2.86.0.tar.xz",
                "recipe": "https://formulae.brew.sh/formula/glib", "notes": [],
                "files": ["Frameworks/libglib-2.0.0.dylib"], "licences": ["glib/COPYING"],
            },
            "cairo": {
                "version": "1.18.4", "stable": None, "spdx": "LGPL-2.1-only OR MPL-1.1",
                "source": "https://cairographics.org/releases/cairo-1.18.4.tar.xz",
                "recipe": mac_licenses.CAIRO_RECIPE,
                "notes": ["Built for Slate.app without lzo (GPL-2.0-or-later), with meson -Dlzo=disabled"],
                "files": ["Frameworks/libcairo.2.dylib"], "licences": ["cairo/COPYING"],
            },
            "pango": {
                "version": "1.58.0", "stable": "1.58.2", "spdx": "LGPL-2.0-or-later",
                "source": "https://download.gnome.org/sources/pango/1.58/pango-1.58.2.tar.xz",
                "recipe": "https://formulae.brew.sh/formula/pango", "notes": [],
                "files": ["Frameworks/libpango-1.0.0.dylib"], "licences": ["pango/COPYING"],
            },
        }
        text = mac_licenses.render("Slate", ["LICENSE", "LICENSE-APACHE"], entries, ["OFL-Carlito.txt"])
        self.assertIn("Third-party software in Slate.app", text)
        self.assertIn("../LICENSE, ../LICENSE-APACHE", text)
        self.assertIn("codesign --force --deep -s - Slate.app", text)
        self.assertIn("glib 2.86.0_1: LGPL-2.1-or-later\n", text)
        self.assertIn("pango 1.58.0 (formula's current release 1.58.2): LGPL-2.0-or-later", text)
        self.assertIn("cairo 1.18.4: LGPL-2.1-only OR MPL-1.1\n  Built for Slate.app without lzo", text)
        self.assertIn("  Recipe:   scripts/mac_cairo.sh in https://github.com/avdrav1/cronkite", text)
        self.assertIn("  Source:   https://download.gnome.org/sources/glib/2.86/glib-2.86.0.tar.xz", text)
        self.assertIn("  Recipe:   https://formulae.brew.sh/formula/glib", text)
        self.assertIn("  In Slate.app: Frameworks/libglib-2.0.0.dylib", text)
        self.assertIn("../fonts/OFL-Carlito.txt", text)
        self.assertLess(text.index("cairo 1.18.4"), text.index("glib 2.86.0_1"), "sorted by formula")

    def test_a_licence_passes_only_if_it_can_be_met_without_the_gpl(self):
        ok = [
            "LGPL-2.1-or-later", "MIT", "0BSD", "LGPL-2.1-only OR MPL-1.1",
            "LGPL-3.0-or-later OR CC-BY-SA-3.0",
            "MIT OR MPL-2.0 OR LGPL-2.1-or-later OR GPL-2.0-or-later",
            "(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT",
            "HPND-sell-variant AND Unicode-3.0 AND MIT-Modern-Variant AND MIT AND LicenseRef-Homebrew-public-domain",
            "IJG AND Zlib AND BSD-3-Clause",
        ]
        gpl = [
            "GPL-2.0-or-later", "GPL-2.0-only", "AGPL-3.0-only",
            "0BSD AND GPL-2.0-or-later", "GPL-3.0-or-later AND LGPL-2.1-or-later",
            "(MIT AND GPL-2.0-only) OR GPL-3.0-only",
            "GPL-2.0-or-later WITH Classpath-exception-2.0",
        ]
        for expr in ok:
            self.assertTrue(mac_licenses.without_gpl(expr), expr)
        for expr in gpl:
            self.assertFalse(mac_licenses.without_gpl(expr), expr)
        with self.assertRaises(ValueError):
            mac_licenses.without_gpl("(MIT OR GPL-2.0-only")


if __name__ == "__main__":
    unittest.main()
