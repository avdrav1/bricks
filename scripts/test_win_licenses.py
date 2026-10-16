"""Tests for win_licenses.py. Run: python3 -m unittest discover -s scripts"""
import contextlib
import io
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mac_licenses  # noqa: E402
import win_licenses  # noqa: E402

P = win_licenses.PREFIX
ZST = "zst" in tarfile.TarFile.OPEN_METH

# name -> (version, base, %LICENSE%, binaries, licence texts), as MSYS2 installs them.
PACKAGES = {
    "glib2": ("2.86.0-1", "mingw-w64-glib2", ["spdx:LGPL-2.1-or-later"],
              ["bin/libglib-2.0-0.dll", "bin/gdbus.exe"],
              {"glib2/COPYING": "see LICENSES", "glib2/LICENSES/LGPL-2.1-or-later.txt": "LGPL text",
               "glib2/gio/COPYING": "gio's"}),
    "libgcc": ("15.2.0-8", "mingw-w64-gcc", ["spdx:GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later"],
               ["bin/libgcc_s_seh-1.dll"], {"libgcc/COPYING.RUNTIME": "runtime exception"}),
    "gettext-runtime": ("0.26-1", "mingw-w64-gettext", ["spdx:GPL-3.0-or-later", "spdx:LGPL-2.1-or-later"],
                        ["bin/libintl-8.dll", "bin/libgettextlib-0-26.dll"], {"gettext-runtime/COPYING": "GPL and LGPL"}),
    "libiconv": ("1.18-1", "mingw-w64-libiconv", ["spdx:LGPL-2.1-or-later", "documentation:spdx:GPL-3.0-or-later"],
                 ["bin/libiconv-2.dll"], {"libiconv/COPYING.LIB": "LGPL"}),
    "pango": ("1.57.0-1", "mingw-w64-pango", ["LGPL-2.1"], ["bin/libpango-1.0-0.dll"], {"pango/COPYING": "LGPL"}),
    "libjpeg-turbo": ("3.1.2-1", "mingw-w64-libjpeg-turbo", ["custom:BSD-like"], ["bin/libjpeg-8.dll"],
                      {"libjpeg-turbo/LICENSE.md": "BSD-like"}),
    "gdk-pixbuf2": ("2.44.4-1", "mingw-w64-gdk-pixbuf2", ["spdx:LGPL-2.1-or-later"],
                    ["bin/libgdk_pixbuf-2.0-0.dll", "lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.dll"],
                    {"gdk-pixbuf2/COPYING": "LGPL"}),
    "gtk4": ("4.20.2-1", "mingw-w64-gtk4", ["spdx:LGPL-2.1-or-later"], ["bin/libgtk-4-1.dll"],
             {"gtk4/COPYING": "LGPL"}),
    "adwaita-icon-theme": ("49.0-1", "mingw-w64-adwaita-icon-theme", ["spdx:LGPL-3.0-or-later OR CC-BY-SA-3.0"], [],
                           {"adwaita-icon-theme/COPYING": "LGPL or CC"}),
    "fontconfig": ("2.17.1-1", "mingw-w64-fontconfig", ["custom"], ["bin/libfontconfig-1.dll"],
                   {"fontconfig/COPYING": "MIT-like"}),
    "hicolor-icon-theme": ("0.18-1", "mingw-w64-hicolor-icon-theme", ["spdx:GPL-2.0-only"], [],
                           {"hicolor-icon-theme/COPYING": "GPL"}),
    "cairo": ("1.18.6-2", "mingw-w64-cairo", ["spdx:LGPL-2.1-or-later OR MPL-1.1"], ["bin/libcairo-2.dll"],
              {"cairo/COPYING": "LGPL or MPL"}),
    "lzo2": ("2.10-2", "mingw-w64-lzo2", ["spdx:GPL-2.0-or-later"], ["bin/liblzo2-2.dll"], {"lzo2/COPYING": "GPL"}),
    "jbigkit": ("2.1-5", "mingw-w64-jbigkit", ["spdx:GPL-2.0"], ["bin/libjbig-0.dll"], {"jbigkit/COPYING": "GPL"}),
    "gstreamer": ("1.26.7-1", "mingw-w64-gstreamer", ["spdx:LGPL-2.0-or-later"], ["bin/libgstreamer-1.0-0.dll"], {}),
}

# What the Brick folder carries of them: its path -> source.
CARRIED = {
    "bin/libglib-2.0-0.dll": "/ucrt64/bin/libglib-2.0-0.dll",
    "bin/gdbus.exe": "/ucrt64/bin/gdbus.exe",
    "bin/libgcc_s_seh-1.dll": "/ucrt64/bin/libgcc_s_seh-1.dll",
    "bin/libintl-8.dll": "/ucrt64/bin/libintl-8.dll",
    "bin/libiconv-2.dll": "/ucrt64/bin/libiconv-2.dll",
    "bin/libpango-1.0-0.dll": "/ucrt64/bin/libpango-1.0-0.dll",
    "bin/libjpeg-8.dll": "/ucrt64/bin/libjpeg-8.dll",
    "bin/libgdk_pixbuf-2.0-0.dll": "/ucrt64/bin/libgdk_pixbuf-2.0-0.dll",
    "bin/libgtk-4-1.dll": "/ucrt64/bin/libgtk-4-1.dll",
    "bin/libfontconfig-1.dll": "/ucrt64/bin/libfontconfig-1.dll",
    "bin/libcairo-2.dll": "rebuilt:cairo",
    "lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.dll":
        "/ucrt64/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-png.dll",
}

CAIRO_RECORD = """package=mingw-w64-ucrt-x86_64-cairo
version=1.18.6-2
spdx=LGPL-2.1-or-later OR MPL-1.1
without=lzo (GPL-2.0-or-later)
options=meson -Dlzo=disabled
source=https://repo.msys2.org/mingw/sources/mingw-w64-cairo-1.18.6-2.src.tar.zst
sha256=ff7124c2155b1169fe102a1dcba1cd7a21f69dace3adf88f1dd8f85ef7ad1e19
recipe=scripts/win_libs.sh in https://github.com/avdrav1/cronkite
"""


def tar_bytes(members, mode):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode=mode) as tar:
        for name, data in members:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


class Licences(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        tmp = Path(self.tmp.name)
        self.msys = tmp / "msys64"
        for name, (version, base, licence, binaries, texts) in PACKAGES.items():
            files = [f"ucrt64/{b}" for b in binaries] + [f"ucrt64/share/licenses/{t}" for t in texts]
            for rel in files:
                (self.msys / rel).parent.mkdir(parents=True, exist_ok=True)
                (self.msys / rel).write_text(texts.get(rel.removeprefix("ucrt64/share/licenses/"), "binary"))
            db = self.msys / "var/lib/pacman/local" / f"{P}{name}-{version}"
            db.mkdir(parents=True)
            (db / "desc").write_text("\n".join([
                "%NAME%", P + name, "", "%VERSION%", version, "", "%BASE%", base, "",
                "%URL%", f"https://example.org/{name}", "", "%LICENSE%", *licence, "", "%DEPENDS%", "zlib", "", ""]))
            dirs = sorted({rel.rsplit("/", 1)[0] + "/" for rel in files})
            (db / "files").write_text("\n".join(["%FILES%", *dirs, *files, ""]))
        (self.msys / "var/lib/pacman/local/ALPM_DB_VERSION").write_text("9")

        self.rebuilt = tmp / "win-libs"
        cairo = self.rebuilt / "cairo"
        (cairo / "share/licenses/cairo").mkdir(parents=True)
        (cairo / "cronkite-build.txt").write_text(CAIRO_RECORD)
        (cairo / "share/licenses/cairo/COPYING").write_text("cairo")
        (cairo / "share/licenses/cairo/COPYING-MPL-1.1").write_text("MPL")

        self.root = tmp / "Brick"
        for d in ["bin", "lib/gdk-pixbuf-2.0/2.10.0/loaders", "share/glib-2.0/schemas", "share/icons/Adwaita",
                  "share/icons/hicolor", "etc/fonts", "fonts"]:
            (self.root / d).mkdir(parents=True)
        (self.root / "lib/gdk-pixbuf-2.0/2.10.0/loaders.cache").write_text("cache")
        for name in ["LICENSE", "LICENSE-APACHE", "fonts/OFL-Carlito.txt", "fonts/LICENSE-Liberation.txt",
                     "fonts/Carlito-Regular.ttf"]:
            (self.root / name).write_text("text")
        (self.root / "licenses").mkdir()
        (self.root / "licenses" / "stale.txt").write_text("from an earlier run")
        self.cache = tmp / "cache"
        self.fetched = []

    def tearDown(self):
        self.tmp.cleanup()

    def fetch(self, url, path):
        self.fetched.append(url)
        upstream = tar_bytes([("gstreamer-1.26.7/COPYING", b"LGPL from upstream"),
                              ("gstreamer-1.26.7/README.md", b"readme"),
                              ("gstreamer-1.26.7/gst/COPYING", b"deeper, not taken")], "w:xz")
        Path(path).write_bytes(tar_bytes([("mingw-w64-gstreamer/PKGBUILD", b"pkgname=gstreamer"),
                                          ("mingw-w64-gstreamer/0001-fix.patch", b"patch"),
                                          ("mingw-w64-gstreamer/gstreamer-1.26.7.tar.xz", upstream)], "w:zst"))

    def run_main(self, carried):
        sources = Path(self.tmp.name) / "sources.tsv"
        sources.write_text("".join(f"{rel}\t{src}\n" for rel, src in carried.items()))
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = win_licenses.main(str(self.root), str(sources), self.msys.as_posix(), str(self.rebuilt),
                                     fetch=self.fetch, cache=str(self.cache))
        return code, out.getvalue(), err.getvalue()

    def notice(self):
        return (self.root / "licenses" / "THIRD-PARTY.txt").read_text()

    def test_the_pacman_db_is_read_with_each_packages_files(self):
        db = win_licenses.read_db(self.msys)
        glib = db[P + "glib2"]
        self.assertEqual((glib["version"], glib["base"], glib["url"]),
                         ("2.86.0-1", "mingw-w64-glib2", "https://example.org/glib2"))
        self.assertEqual(db[P + "libiconv"]["licence"], ["spdx:LGPL-2.1-or-later", "documentation:spdx:GPL-3.0-or-later"])
        self.assertIn("ucrt64/bin/gdbus.exe", glib["files"])
        self.assertIn("ucrt64/share/licenses/glib2/", glib["files"])
        owner = win_licenses.owners(db)
        self.assertEqual(owner["ucrt64/bin/libglib-2.0-0.dll"], {P + "glib2"})
        self.assertNotIn("ucrt64/bin/", owner)
        self.assertEqual(win_licenses.source_url(glib),
                         "https://repo.msys2.org/mingw/sources/mingw-w64-glib2-2.86.0-1.src.tar.zst")

    def test_the_licence_field_becomes_an_spdx_expression(self):
        cases = [
            (["spdx:LGPL-2.1-or-later"], "LGPL-2.1-or-later"),
            (["spdx:0BSD", "spdx:LGPL-2.1-or-later OR MPL-1.1"], "0BSD AND (LGPL-2.1-or-later OR MPL-1.1)"),
            (["custom"], "LicenseRef-custom"),
            (["custom:BSD-like"], "LicenseRef-custom-BSD-like"),
            (["LGPL"], "LGPL"),
            (["LGPL2.1"], "LGPL-2.1"),
            (["LGPL-2.1"], "LGPL-2.1"),
            (["GPL"], "GPL-unversioned"),
            (["GPL3"], "GPL-3"),
            (["spdx:MIT", "spdx:MIT"], "MIT"),
        ]
        for entries, expr in cases:
            self.assertEqual(win_licenses.licence(entries)[0], expr, entries)
        for passes in ["LGPL", "LGPL-2.1", "LicenseRef-custom-BSD-like"]:
            self.assertTrue(mac_licenses.without_gpl(passes), passes)
        for fails in ["GPL-unversioned", "GPL-3", "AGPL-unversioned"]:
            self.assertFalse(mac_licenses.without_gpl(fails), fails)
        self.assertEqual(win_licenses.licence(["AGPL"])[0], "AGPL-unversioned")

        expr, notes = win_licenses.licence(["spdx:LGPL-2.1-or-later", "documentation:spdx:GPL-3.0-or-later"])
        self.assertEqual(expr, "LGPL-2.1-or-later")
        self.assertEqual(notes, ["MSYS2 also names GPL-3.0-or-later for the package's documentation, which isn't carried"])
        expr, notes = win_licenses.licence(["spdx:MIT", "tools:spdx:GPL-2.0-only"])
        self.assertEqual(expr, "MIT AND GPL-2.0-only", "a scope other than documentation counts")
        self.assertEqual(notes, ["MSYS2 names GPL-2.0-only for the package's tools"])
        self.assertEqual(win_licenses.licence([]), (None, []))

    def test_gccs_runtime_exception_passes_only_when_asked(self):
        gcc = "GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later"
        self.assertFalse(mac_licenses.without_gpl(gcc), "the Mac's check is unchanged")
        self.assertTrue(mac_licenses.without_gpl(gcc, exceptions=frozenset({"GCC-exception-3.1"})))
        self.assertFalse(mac_licenses.without_gpl("GPL-2.0-or-later WITH Classpath-exception-2.0",
                                                  exceptions=frozenset({"GCC-exception-3.1"})))
        self.assertFalse(mac_licenses.without_gpl("GPL-3.0-or-later AND (GPL-2.0-only WITH GCC-exception-3.1)",
                                                  exceptions=frozenset({"GCC-exception-3.1"})))

    def test_the_folders_notice_names_each_package_its_licence_source_and_files(self):
        code, out, err = self.run_main(CARRIED)
        self.assertEqual(code, 0, err)
        text = self.notice()
        self.assertIn("Third-party software in Brick\n", text)
        self.assertIn("(..\\LICENSE, ..\\LICENSE-APACHE)", text)
        self.assertIn("THIRD-PARTY-RUST.txt, beside this file", text)
        self.assertIn("built by MSYS2 for Windows (UCRT64, x86_64)", text)
        self.assertIn("glib2 2.86.0-1: LGPL-2.1-or-later\n"
                      "  Source:   https://repo.msys2.org/mingw/sources/mingw-w64-glib2-2.86.0-1.src.tar.zst\n"
                      "  Upstream: https://example.org/glib2\n"
                      "  Recipe:   https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-glib2\n"
                      "  Licences: glib2\\COPYING, glib2\\LGPL-2.1-or-later.txt, glib2\\gio-COPYING\n"
                      "  In Brick: bin\\gdbus.exe, bin\\libglib-2.0-0.dll\n", text)
        self.assertIn("libgcc 15.2.0-8: GPL-3.0-or-later WITH GCC-exception-3.1 AND GFDL-1.3-or-later\n"
                      "  The GPL here comes with GCC's runtime library exception (GCC-exception-3.1)", text)
        self.assertIn("so its terms don't extend to Brick; the GFDL is GCC's manuals'", text)
        self.assertIn("  Recipe:   https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-gcc", text)
        self.assertIn("gettext-runtime 0.26-1: LGPL-2.1-or-later\n"
                      "  MSYS2's licence GPL-3.0-or-later AND LGPL-2.1-or-later, which covers tools Brick doesn't carry;"
                      " the library libintl is LGPL-2.1-or-later", text)
        self.assertIn("libiconv 1.18-1: LGPL-2.1-or-later\n"
                      "  MSYS2 also names GPL-3.0-or-later for the package's documentation, which isn't carried", text)
        self.assertIn("pango 1.57.0-1: LGPL-2.1\n", text)
        self.assertIn("libjpeg-turbo 3.1.2-1: LicenseRef-custom-BSD-like\n", text)
        self.assertIn("fontconfig 2.17.1-1: LicenseRef-custom\n", text)
        self.assertIn("  In Brick: bin\\libfontconfig-1.dll, etc\\fonts (fonts.conf, conf.d)\n", text)
        self.assertIn("  In Brick: bin\\libgtk-4-1.dll, share\\glib-2.0\\schemas (GTK's settings schemas)\n", text)
        self.assertIn("adwaita-icon-theme 49.0-1: LGPL-3.0-or-later OR CC-BY-SA-3.0\n", text)
        self.assertIn("  In Brick: share\\icons\\Adwaita (symbolic icons)\n", text)
        self.assertIn("  In Brick: bin\\libgdk_pixbuf-2.0-0.dll, lib\\gdk-pixbuf-2.0\\2.10.0\\loaders.cache (the loaders' list),"
                      " lib\\gdk-pixbuf-2.0\\2.10.0\\loaders\\libpixbufloader-png.dll\n", text)
        self.assertIn("cairo 1.18.6-2: LGPL-2.1-or-later OR MPL-1.1\n"
                      "  Built for Brick without lzo (GPL-2.0-or-later), from MSYS2's source package with meson -Dlzo=disabled\n"
                      "  Source:   https://repo.msys2.org/mingw/sources/mingw-w64-cairo-1.18.6-2.src.tar.zst"
                      " (sha256 ff7124c2155b1169fe102a1dcba1cd7a21f69dace3adf88f1dd8f85ef7ad1e19)\n"
                      "  Upstream: https://example.org/cairo\n"
                      "  Recipe:   scripts/win_libs.sh in https://github.com/avdrav1/cronkite\n"
                      "  Licences: cairo\\COPYING, cairo\\COPYING-MPL-1.1\n", text)
        self.assertIn("..\\fonts\\LICENSE-Liberation.txt, ..\\fonts\\OFL-Carlito.txt.", text)
        self.assertNotIn("hicolor", text)
        self.assertLess(text.index("cairo 1.18.6-2"), text.index("glib2 2.86.0-1"), "sorted by package")
        self.assertIn(b"\r\n", (self.root / "licenses" / "THIRD-PARTY.txt").read_bytes(), "Windows line ends")

        licences = self.root / "licenses"
        self.assertEqual(sorted(p.name for p in (licences / "glib2").iterdir()),
                         ["COPYING", "LGPL-2.1-or-later.txt", "gio-COPYING"])
        self.assertEqual((licences / "glib2" / "gio-COPYING").read_text(), "gio's")
        self.assertEqual((licences / "cairo" / "COPYING").read_text(), "cairo", "the rebuilt cairo's texts")
        self.assertFalse((licences / "stale.txt").exists(), "written afresh")
        self.assertFalse((licences / "hicolor-icon-theme").exists())
        self.assertIn("licence: glib2 2.86.0-1: LGPL-2.1-or-later (3 files)\n", out)
        self.assertIn("licence: cairo 1.18.6-2: LGPL-2.1-or-later OR MPL-1.1 (2 files)\n", out)
        self.assertEqual(len(out.splitlines()), 11)
        self.assertEqual(self.fetched, [], "every package installed its texts")

    def test_data_the_folder_doesnt_carry_isnt_listed(self):
        for d in ["share/glib-2.0", "share/icons/Adwaita"]:
            for p in sorted((self.root / d).rglob("*"), reverse=True):
                p.rmdir()
            (self.root / d).rmdir()
        code, _, err = self.run_main(CARRIED)
        self.assertEqual(code, 0, err)
        self.assertNotIn("adwaita-icon-theme", self.notice())
        self.assertNotIn("schemas", self.notice())
        self.assertIn("  In Brick: bin\\libgtk-4-1.dll\n", self.notice())

    def test_gpl_libraries_fail(self):
        code, out, err = self.run_main({**CARRIED, "bin/liblzo2-2.dll": "/ucrt64/bin/liblzo2-2.dll",
                                        "bin/libjbig-0.dll": "/ucrt64/bin/libjbig-0.dll"})
        self.assertEqual(code, 1)
        self.assertIn("lzo2 2.10-2: GPL-2.0-or-later needs the GPL, and Brick ships no GPL code", err)
        self.assertIn("jbigkit 2.1-5: GPL-2.0 needs the GPL, and Brick ships no GPL code", err)
        self.assertEqual(out, "")
        self.assertFalse((self.root / "licenses" / "THIRD-PARTY.txt").exists())

    def test_gettext_counts_by_its_gpl_when_the_folder_carries_more_than_libintl(self):
        code, _, err = self.run_main({**CARRIED, "bin/libgettextlib-0-26.dll": "/ucrt64/bin/libgettextlib-0-26.dll"})
        self.assertEqual(code, 1)
        self.assertIn("gettext-runtime 0.26-1: GPL-3.0-or-later AND LGPL-2.1-or-later needs the GPL", err)

    def test_a_binary_from_no_package_fails(self):
        code, _, err = self.run_main({**CARRIED, "bin/libmystery.dll": "/ucrt64/bin/libmystery.dll"})
        self.assertEqual(code, 1)
        self.assertIn("bin/libmystery.dll: /ucrt64/bin/libmystery.dll is in no MSYS2 package", err)

    def test_sources_map_case_insensitively(self):
        code, _, err = self.run_main({"bin/LIBGLIB-2.0-0.DLL": "/UCRT64/bin/LibGLib-2.0-0.dll"})
        self.assertEqual(code, 0, err)
        self.assertIn("  In Brick: bin\\LIBGLIB-2.0-0.DLL\n", self.notice())

    def test_a_rebuilt_library_needs_its_record_and_cant_also_come_from_msys2(self):
        code, _, err = self.run_main({"bin/libtiff-6.dll": "rebuilt:libtiff"})
        self.assertEqual(code, 1)
        self.assertIn("libtiff: Brick carries bin\\libtiff-6.dll rebuilt, but there's no", err)
        code, _, err = self.run_main({**CARRIED, "bin/libcairo-gobject-2.dll": "/ucrt64/bin/libcairo-2.dll"})
        self.assertEqual(code, 1)
        self.assertIn("cairo: Brick carries it both from MSYS2 and rebuilt", err)

    @unittest.skipUnless(ZST, "this Python's tarfile can't write .tar.zst")
    def test_a_package_without_licence_texts_takes_them_from_its_source_package(self):
        carried = {**CARRIED, "bin/libgstreamer-1.0-0.dll": "/ucrt64/bin/libgstreamer-1.0-0.dll"}
        code, out, err = self.run_main(carried)
        self.assertEqual(code, 0, err)
        self.assertEqual(self.fetched, ["https://repo.msys2.org/mingw/sources/mingw-w64-gstreamer-1.26.7-1.src.tar.zst"])
        self.assertEqual(sorted(p.name for p in (self.root / "licenses" / "gstreamer").iterdir()), ["COPYING"])
        self.assertEqual((self.root / "licenses" / "gstreamer" / "COPYING").read_text(), "LGPL from upstream")
        self.assertIn("gstreamer 1.26.7-1: LGPL-2.0-or-later\n"
                      "  Licence texts from gstreamer-1.26.7.tar.xz in MSYS2's source package (the package installs none)\n",
                      self.notice())
        self.assertIn("licence: gstreamer 1.26.7-1: LGPL-2.0-or-later (1 files)", out)
        self.assertEqual(self.run_main(carried)[0], 0)
        self.assertEqual(len(self.fetched), 1, "the source package is cached")

    @unittest.skipUnless(ZST, "this Python's tarfile can't write .tar.zst")
    def test_a_source_package_without_licence_texts_fails(self):
        def fetch(url, path):
            upstream = tar_bytes([("gstreamer-1.26.7/README.md", b"readme")], "w:gz")
            Path(path).write_bytes(tar_bytes([("mingw-w64-gstreamer/gstreamer-1.26.7.tar.gz", upstream)], "w:zst"))
        self.fetch = fetch
        code, _, err = self.run_main({"bin/libgstreamer-1.0-0.dll": "/ucrt64/bin/libgstreamer-1.0-0.dll"})
        self.assertEqual(code, 1)
        self.assertIn("gstreamer 1.26.7-1: no licence text in", err)

    def test_a_download_that_fails_is_a_problem(self):
        def fetch(url, path):
            raise OSError("HTTP Error 404: Not Found")
        self.fetch = fetch
        code, _, err = self.run_main({"bin/libgstreamer-1.0-0.dll": "/ucrt64/bin/libgstreamer-1.0-0.dll"})
        self.assertEqual(code, 1)
        if ZST:
            self.assertIn("gstreamer 1.26.7-1: no licence text installed, and its source package: HTTP Error 404", err)

    def test_a_python_without_zstd_says_so(self):
        meths = {k: v for k, v in tarfile.TarFile.OPEN_METH.items() if k != "zst"}
        with mock.patch.object(tarfile.TarFile, "OPEN_METH", meths):
            code, _, err = self.run_main({"bin/libgstreamer-1.0-0.dll": "/ucrt64/bin/libgstreamer-1.0-0.dll"})
        self.assertEqual(code, 1)
        self.assertIn("can't open its source package (.tar.zst; 3.14 can)", err)
        self.assertEqual(self.fetched, [])


if __name__ == "__main__":
    unittest.main()
