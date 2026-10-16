#!/usr/bin/env python3
"""Third-party licence notices for a Windows app folder (WIN-5).

    python3 scripts/win_licenses.py ROOT SOURCES MSYS_ROOT REBUILT

scripts/win_bundle.sh runs it, with MSYS2 UCRT64's Python, once the libraries are in the
folder. ROOT is the app's folder (<Name>, e.g. Brick); SOURCES has a line per binary copied
into it, its path in ROOT (forward slashes) and where it came from, tab-separated: an MSYS2
path like /ucrt64/bin/libgtk-4-1.dll, or rebuilt:<lib> for a DLL of REBUILT/<lib>.
MSYS_ROOT is MSYS2's root; REBUILT is scripts/win_libs.sh's prefix, whose cairo (without
lzo) and libtiff (without jbig) the folder carries instead of MSYS2's.

Every MSYS2 binary maps to the package whose pacman file list (MSYS_ROOT/var/lib/pacman/
local/*/files) holds it; to those it adds the packages whose data the folder carries
(DATA). Each package's licence texts (its files under ucrt64/share/licenses; for a package
with none, the top level and LICENSES/ of the upstream tarballs in MSYS2's source package,
which it downloads) go to ROOT/licenses/<package>/, and ROOT/licenses/THIRD-PARTY.txt lists
every package: version, licence (pacman's %LICENSE%, as SPDX), MSYS2's source package (the
upstream source and MSYS2's patches), upstream, build recipe, its licence files, and what
of it the app carries. A rebuilt library's entry is its cronkite-build.txt.

It fails when a binary comes from no package, a package has no licence text, or a licence
can't be met without the GPL (the apps ship no GPL code, user decision 2026-10-16; LGPL is
fine). GCC's runtime libraries (libgcc, libstdc++) are GPL with GCC's runtime library
exception, which passes; a licence scoped to documentation doesn't count, as the folder
carries none; a package whose licence names the GPL only for tools the folder leaves out
counts by its library's licence (LIBRARY_LICENCE). It prints one line per package.
"""

import os
import posixpath
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.request

from mac_licenses import read_record, source_licences, without_gpl

PREFIX = "mingw-w64-ucrt-x86_64-"
LICENCES = "ucrt64/share/licenses/"
SOURCES_URL = "https://repo.msys2.org/mingw/sources/"
RECIPES_URL = "https://github.com/msys2/MINGW-packages/tree/master/"

# Packages whose data the folder carries (scripts/win_bundle.sh), where in ROOT, and what.
# hicolor-icon-theme's index.theme is GPL-2.0, so the bundle writes its own.
DATA = {
    "gtk4": ("share/glib-2.0/schemas", "GTK's settings schemas"),
    "adwaita-icon-theme": ("share/icons/Adwaita", "symbolic icons"),
    "fontconfig": ("etc/fonts", "fonts.conf, conf.d"),
    "gdk-pixbuf2": ("lib/gdk-pixbuf-2.0/2.10.0/loaders.cache", "the loaders' list"),
}

# GCC's runtime library exception: libgcc_s_seh-1.dll and libstdc++-6.dll are GPL with it.
RUNTIME_EXCEPTION = "GCC-exception-3.1"

# Packages whose MSYS2 licence includes the GPL for their programs alone: the licence of the
# only files the folder may carry of them, and why.
LIBRARY_LICENCE = {
    "gettext-runtime": ("LGPL-2.1-or-later", {"libintl-8.dll"},
                        "the library libintl is LGPL-2.1-or-later (the GPL is gettext's programs')"),
    "xz": ("0BSD", {"liblzma-5.dll"},
           "the library liblzma is 0BSD (the GPL is the xzgrep and xzdiff scripts')"),
}

UPSTREAM = re.compile(r"\.(tar\.\w+|tgz)$")


def windows(path):
    return path.replace("/", "\\")


def short(pkgname):
    """A package's name without MSYS2's UCRT64 prefix."""
    return pkgname[len(PREFIX):] if pkgname.startswith(PREFIX) else pkgname


def read_sections(path):
    """A pacman db file (desc, files): %SECTION% -> its values."""
    sections, current = {}, None
    with open(path, encoding="utf-8") as f:
        for line in f.read().splitlines():
            if line.startswith("%") and line.endswith("%") and len(line) > 2:
                current = sections.setdefault(line[1:-1], [])
            elif line and current is not None:
                current.append(line)
            else:
                current = None
    return sections


def read_db(msys_root):
    """MSYS2's installed packages: name -> dict(version, base, url, licence, files)."""
    local = os.path.join(msys_root, "var", "lib", "pacman", "local")
    db = {}
    for entry in sorted(os.listdir(local)):
        desc = os.path.join(local, entry, "desc")
        if not os.path.isfile(desc):
            continue
        d = read_sections(desc)
        files = os.path.join(local, entry, "files")
        name = d["NAME"][0]
        db[name] = {
            "version": d["VERSION"][0],
            "base": (d.get("BASE") or d["NAME"])[0],
            "url": (d.get("URL") or [""])[0],
            "licence": d.get("LICENSE", []),
            "files": read_sections(files).get("FILES", []) if os.path.isfile(files) else [],
        }
    return db


def owners(db):
    """Each installed file, lower-cased (Windows), -> the packages that hold it."""
    owner = {}
    for name, pkg in db.items():
        for rel in pkg["files"]:
            if not rel.endswith("/"):
                owner.setdefault(rel.lower(), set()).add(name)
    return owner


def legacy(name):
    """A pacman licence name that isn't SPDX, as an SPDX-like identifier: custom texts become
    LicenseRef-custom(-<x>), the GPL family gets SPDX's dash (GPL2 -> GPL-2, LGPL2.1 ->
    LGPL-2.1; a bare GPL or AGPL GPL-unversioned) so that without_gpl tells GPL from LGPL,
    and other names keep theirs."""
    if name == "custom":
        return "LicenseRef-custom"
    if name.startswith("custom:"):
        return "LicenseRef-custom-" + re.sub(r"[^A-Za-z0-9.]+", "-", name[len("custom:"):]).strip("-")
    m = re.fullmatch(r"(LGPL|AGPL|GPL)-?(\S*)", name, re.IGNORECASE)
    if m:
        family, version = m[1].upper(), m[2]
        if version:
            return f"{family}-{version}"
        return family if family == "LGPL" else f"{family}-unversioned"
    return re.sub(r"\s+", "-", name.strip())


def licence(entries):
    """(SPDX expression, notes) of a package's %LICENSE% entries, which pacman ANDs:
    spdx:<expr> is SPDX, <scope>:spdx:<expr> covers only that part of the package (the
    documentation scope is dropped with a note: the folder carries no documentation), and
    other entries are pacman's legacy names (legacy)."""
    terms, notes = [], []
    for entry in entries:
        m = re.fullmatch(r"(?:([^:\s]+):)?spdx:(.+)", entry.strip())
        if m and m[1] == "documentation":
            notes.append(f"MSYS2 also names {m[2].strip()} for the package's documentation, which isn't carried")
            continue
        if m and m[1]:
            notes.append(f"MSYS2 names {m[2].strip()} for the package's {m[1]}")
        term = m[2].strip() if m else legacy(entry.strip())
        if term not in terms:
            terms.append(term)
    if len(terms) > 1:
        terms = [f"({t})" if " " in t else t for t in terms]
    return " AND ".join(terms) or None, notes


def named(files):
    """(name, bytes) of licence texts given as (path with forward slashes, file): a name that
    clashes gets its folder's name in front."""
    texts = []
    for rel, path in files:
        target = posixpath.basename(rel)
        if target in [t for t, _ in texts]:
            target = f"{posixpath.basename(posixpath.dirname(rel))}-{target}"
        with open(path, "rb") as f:
            texts.append((target, f.read()))
    return texts


def package_texts(msys_root, files):
    """A package's licence texts: its files under ucrt64/share/licenses/, subfolders too."""
    return named(sorted(
        (rel, os.path.join(msys_root, *rel.split("/")))
        for rel in files if not rel.endswith("/") and rel.lower().startswith(LICENCES)))


def folder_texts(folder):
    """The licence texts in a folder and its subfolders."""
    found = []
    for dirpath, _, filenames in os.walk(folder):
        for name in filenames:
            path = os.path.join(dirpath, name)
            found.append((os.path.relpath(path, folder).replace(os.sep, "/"), path))
    return named(sorted(found))


def source_url(pkg):
    """MSYS2's source package of an installed package (pacman's epoch colon is ~ in its name)."""
    return f"{SOURCES_URL}{pkg['base']}-{pkg['version'].replace(':', '~')}.src.tar.zst"


def download(url, path):
    part = path + ".part"
    with urllib.request.urlopen(url, timeout=300) as response, open(part, "wb") as out:
        shutil.copyfileobj(response, out)
    os.replace(part, path)


def source_package_licences(url, fetch, cache):
    """(texts, tarballs): the licence texts of the upstream tarballs in the MSYS2 source
    package at `url` (source_licences: their top level and LICENSES/), and those tarballs'
    names. `fetch(url, path)` downloads it into `cache` unless it's there."""
    path = os.path.join(cache, url.rsplit("/", 1)[1])
    if not os.path.isfile(path):
        fetch(url, path)
    texts, tarballs = [], []
    with tarfile.open(path, "r:zst") as tar:
        for member in tar.getmembers():
            name = member.name.rsplit("/", 1)[-1]
            if not (member.isfile() and UPSTREAM.search(name)):
                continue
            inner = os.path.join(cache, name)
            with open(inner, "wb") as out:
                shutil.copyfileobj(tar.extractfile(member), out)
            for target, data in source_licences(inner):
                if target in [t for t, _ in texts]:
                    target = f"{UPSTREAM.sub('', name)}-{target}"
                texts.append((target, data))
            tarballs.append(name)
    return texts, tarballs


def render(title, own, entries, fonts):
    """THIRD-PARTY.txt. `entries`: name -> dict(version, spdx, source, upstream, recipe,
    files, licences, notes); `own`: the app's own licence files; `fonts`: the fonts
    folder's licence files."""
    out = [
        f"Third-party software in {title}",
        "",
        f"{title} itself is free software under the MIT or Apache-2.0 licence ({', '.join(windows('../' + f) for f in own)}).",
        "The Rust crates compiled into it and their licences are in THIRD-PARTY-RUST.txt, beside this file.",
        f"{title} carries the libraries and data below, built by MSYS2 for Windows (UCRT64, x86_64)",
        "unless an entry says otherwise. The libraries are dynamically linked, each a DLL of its own",
        "in bin\\ or lib\\gdk-pixbuf-2.0\\2.10.0\\loaders\\, so you may replace one with your own build",
        "of the same library.",
        "Each library's licence texts are in this folder, a subfolder per MSYS2 package. Its source is",
        "MSYS2's source package listed, which holds the upstream source and MSYS2's patches, and its",
        "build recipe is the one named.",
        "",
    ]
    for name in sorted(entries):
        e = entries[name]
        out.append(f"{name} {e['version']}: {e['spdx']}")
        out.extend(f"  {note}" for note in e["notes"])
        out.append(f"  Source:   {e['source']}")
        if e["upstream"]:
            out.append(f"  Upstream: {e['upstream']}")
        out.append(f"  Recipe:   {e['recipe']}")
        out.append(f"  Licences: {', '.join(e['licences'])}")
        out.append(f"  In {title}: {', '.join(e['files'])}")
        out.append("")
    if fonts:
        out.append("Fonts (fonts\\): Liberation 2.1.5, Carlito, and Caladea, under the SIL Open Font License 1.1;")
        out.append(f"their licence texts are beside them: {', '.join(windows('../fonts/' + f) for f in fonts)}.")
        out.append("")
    return "\n".join(out)


def main(root, sources, msys_root, rebuilt, fetch=download, cache=None):
    title = os.path.basename(os.path.normpath(root))
    db = read_db(msys_root)
    owner = owners(db)
    carried = {}  # ("msys2", package) or ("rebuilt", lib) -> what the app carries of it
    problems = []

    def add(key, what):
        carried.setdefault(key, []).append(what)

    with open(sources, encoding="utf-8") as f:
        lines = [line for line in f.read().splitlines() if line.strip()]
    for line in lines:
        rel, src = line.split("\t")
        if src.startswith("rebuilt:"):
            add(("rebuilt", src[len("rebuilt:"):]), windows(rel))
            continue
        hits = owner.get(src.replace("\\", "/").lstrip("/").lower(), set())
        if len(hits) != 1:
            problems.append(f"{rel}: {src} is in {'packages ' + ', '.join(sorted(hits)) if hits else 'no MSYS2 package'}")
            continue
        add(("msys2", next(iter(hits))), windows(rel))
    for name, (path, about) in DATA.items():
        if not os.path.exists(os.path.join(root, *path.split("/"))):
            continue
        if PREFIX + name not in db:
            problems.append(f"{name}: not installed, yet {title} carries its {path}")
            continue
        add(("msys2", PREFIX + name), f"{windows(path)} ({about})")

    dest = os.path.join(root, "licenses")
    shutil.rmtree(dest, ignore_errors=True)
    os.makedirs(dest)
    cache = cache or os.path.join(tempfile.gettempdir(), "cronkite-msys2-sources")
    entries = {}
    for (origin, ident), what in sorted(carried.items()):
        if origin == "rebuilt":
            prefix = os.path.join(rebuilt, ident)
            record = os.path.join(prefix, "cronkite-build.txt")
            if not os.path.isfile(record):
                problems.append(f"{ident}: {title} carries {', '.join(what)} rebuilt, but there's no {record}")
                continue
            built = read_record(record)
            name, version, spdx = short(built["package"]), built["version"], built["spdx"]
            notes = [f"Built for {title} without {built['without']}, from MSYS2's source package with {built['options']}"]
            source = f"{built['source']} (sha256 {built['sha256']})"
            upstream = db.get(built["package"], {}).get("url", "")
            recipe = built["recipe"]
            texts = folder_texts(os.path.join(prefix, "share", "licenses", ident))
            where = os.path.join(prefix, "share", "licenses", ident)
        else:
            pkg = db[ident]
            name, version = short(ident), pkg["version"]
            spdx, notes = licence(pkg["licence"])
            source, upstream = source_url(pkg), pkg["url"]
            recipe = RECIPES_URL + pkg["base"]
            where = f"{msys_root}/{LICENCES}"
            try:
                texts = package_texts(msys_root, pkg["files"])
            except OSError as e:
                problems.append(f"{name} {version}: can't read its licence text: {e}")
                continue
            if not texts:
                # Some packages install none; MSYS2's source package has the upstream tarball.
                where += f" or {source}"
                if "zst" not in tarfile.TarFile.OPEN_METH:
                    problems.append(f"{name} {version}: no licence text installed, and Python "
                                    f"{sys.version.split()[0]} can't open its source package (.tar.zst; 3.14 can)")
                    continue
                os.makedirs(cache, exist_ok=True)
                try:
                    texts, tarballs = source_package_licences(source, fetch, cache)
                except (OSError, tarfile.TarError) as e:
                    problems.append(f"{name} {version}: no licence text installed, and its source package: {e}")
                    continue
                if texts:
                    notes.append(f"Licence texts from {', '.join(tarballs)} in MSYS2's source package (the package installs none)")
        if name in entries:
            problems.append(f"{name}: {title} carries it both from MSYS2 and rebuilt")
            continue
        if not texts:
            problems.append(f"{name} {version}: no licence text in {where}")
            continue
        library = LIBRARY_LICENCE.get(name)
        if library and spdx and {w.rsplit("\\", 1)[-1].lower() for w in what} <= library[1]:
            notes.append(f"MSYS2's licence {spdx}, which covers tools {title} doesn't carry; {library[2]}")
            spdx = library[0]
        if not spdx:
            problems.append(f"{name} {version}: no licence in MSYS2's package to check")
        else:
            try:
                ok = without_gpl(spdx, exceptions=frozenset({RUNTIME_EXCEPTION}))
                excepted = ok and not without_gpl(spdx)
            except ValueError as e:
                problems.append(f"{name} {version}: {e}")
                ok = excepted = False
            if not ok:
                problems.append(f"{name} {version}: {spdx} needs the GPL, and {title} ships no GPL code")
            if excepted:
                notes.append(f"The GPL here comes with GCC's runtime library exception ({RUNTIME_EXCEPTION}), which lets "
                             f"programs compiled by GCC use this runtime library under any licence, so its terms don't "
                             f"extend to {title}" + ("; the GFDL is GCC's manuals'" if "GFDL" in spdx else ""))
        os.makedirs(os.path.join(dest, name))
        for target, data in texts:
            with open(os.path.join(dest, name, target), "wb") as out:
                out.write(data)
        entries[name] = {
            "version": version,
            "spdx": spdx,
            "source": source,
            "upstream": upstream,
            "recipe": recipe,
            "notes": notes,
            "files": sorted(what),
            "licences": [f"{name}\\{t}" for t, _ in texts],
        }
    if problems:
        print("win_licenses: " + "\n  ".join(["no notice, or a licence the app can't carry:", *problems]), file=sys.stderr)
        return 1
    own = sorted(f for f in os.listdir(root) if f.startswith("LICENSE") and os.path.isfile(os.path.join(root, f)))
    fonts_dir = os.path.join(root, "fonts")
    fonts = sorted(f for f in os.listdir(fonts_dir) if f.endswith(".txt")) if os.path.isdir(fonts_dir) else []
    with open(os.path.join(dest, "THIRD-PARTY.txt"), "w", encoding="utf-8", newline="\r\n") as out:
        out.write(render(title, own, entries, fonts))
    for name in sorted(entries):
        print(f"licence: {name} {entries[name]['version']}: {entries[name]['spdx']} ({len(entries[name]['licences'])} files)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 5:
        raise SystemExit(__doc__.split("\n\n")[1])
    sys.exit(main(*sys.argv[1:]))
