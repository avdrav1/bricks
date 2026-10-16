#!/usr/bin/env python3
"""Third-party licence notices for a macOS app bundle (MAC-7).

    python3 scripts/mac_licenses.py CONTENTS LOADER_SOURCES CAIRO_PREFIX

scripts/mac_bundle.sh runs it once the libraries are in the bundle, before signing. CONTENTS
is the bundle's Contents folder; LOADER_SOURCES has a line per gdk-pixbuf loader, its name
and the Homebrew file it was copied from, tab-separated; CAIRO_PREFIX is
scripts/mac_cairo.sh's cairo, built without lzo, which the bundle carries instead of
Homebrew's.

For every dylib in Contents/Frameworks and every loader it finds the Homebrew formula the
file came from (the formula's copy under $(brew --prefix)/opt/*/lib, resolved into the
Cellar; cairo's libraries are ours), and adds the formulas whose data the bundle carries
(DATA). Each formula's licence files (COPYING, LICENSE, NOTICE, ... in its keg, its
share/doc, or its share/licenses; for a keg with none, the top level and LICENSES/ of the
formula's source tarball, which `brew fetch --build-from-source` downloads) go to
Contents/Resources/licenses/<formula>/, and Contents/Resources/licenses/THIRD-PARTY.txt
lists every formula: version, SPDX licence (`brew info`), upstream source, build recipe,
its licence files, and what of it the app carries.

It fails when a library comes from no formula, a formula has no licence file, or a
licence can't be met without the GPL (the bundles ship no GPL code, user decision
2026-10-16; LGPL is fine). A formula whose Homebrew licence names the GPL only for tools
the bundle leaves out counts by its library's licence (LIBRARY_LICENCE). It prints one
line per formula.
"""

import glob
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile

# Formulas whose data the bundle carries (scripts/mac_bundle.sh), and what. hicolor's
# index.theme is GPL-2.0, so the bundle writes its own.
DATA = {
    "gtk4": "Resources/share/glib-2.0/schemas (GTK's settings schemas)",
    "adwaita-icon-theme": "Resources/share/icons/Adwaita (symbolic icons)",
    "fontconfig": "Resources/etc/fonts (fonts.conf, conf.d)",
}

# Formulas whose Homebrew licence includes the GPL for their programs alone: the licence of
# the only files the bundle may carry of them, and why.
LIBRARY_LICENCE = {
    "fribidi": ("LGPL-2.1-or-later", {"libfribidi.0.dylib"},
                "the library libfribidi is LGPL-2.1-or-later (the GPL is the fribidi program's getopt)"),
    "gettext": ("LGPL-2.1-or-later", {"libintl.8.dylib"},
                "the library libintl is LGPL-2.1-or-later (the GPL is gettext's programs')"),
    "xz": ("0BSD", {"liblzma.5.dylib"},
           "the library liblzma is 0BSD (the GPL is the xzgrep and xzdiff scripts')"),
}

# scripts/mac_cairo.sh's cairo.
CAIRO_SPDX = "LGPL-2.1-only OR MPL-1.1"
CAIRO_RECIPE = "scripts/mac_cairo.sh in https://github.com/avdrav1/cronkite"

LICENCE = re.compile(r"^(copying|licen[cs]e|notice|copyright|unlicense)", re.IGNORECASE)


def cellar_formula(path, cellar):
    """(formula, version) of a file or folder that resolves into the Cellar, else None."""
    rel = os.path.relpath(os.path.realpath(path), os.path.realpath(cellar))
    parts = rel.split(os.sep)
    if rel.startswith("..") or len(parts) < 2:
        return None
    return parts[0], parts[1]


def is_licence(name):
    return bool(LICENCE.match(name))


def licence_files(keg):
    """A keg's licence files: in its root, share/doc/*, and share/licenses/*."""
    dirs = [keg, *sorted(glob.glob(os.path.join(keg, "share", "doc", "*"))),
            *sorted(glob.glob(os.path.join(keg, "share", "licenses", "*")))]
    found = []
    for d in dirs:
        if not os.path.isdir(d):
            continue
        for name in sorted(os.listdir(d)):
            path = os.path.join(d, name)
            if os.path.isfile(path) and is_licence(name):
                found.append(path)
    return found


def source_licences(tarball):
    """(name, bytes) of the licence files at a source tarball's top level and in its
    LICENSES/ folder (REUSE's layout, which GLib uses)."""
    found = []
    with tarfile.open(tarball) as tar:
        for member in tar.getmembers():
            parts = member.name.strip("/").split("/")
            top = len(parts) == 2 and is_licence(parts[1])
            reuse = len(parts) == 3 and parts[1] == "LICENSES"
            if member.isfile() and (top or reuse):
                found.append((parts[-1], tar.extractfile(member).read()))
    return sorted(found)


def render(title, own, entries, fonts):
    """THIRD-PARTY.txt. `entries`: name -> dict(version, spdx, source, recipe, files,
    licences, stable, notes); `own`: the app's own licence files; `fonts`: the fonts
    folder's licence files."""
    out = [
        f"Third-party software in {title}.app",
        "",
        f"{title} itself is free software under the MIT or Apache-2.0 licence ({', '.join('../' + f for f in own)}).",
        f"{title}.app carries the libraries and data below, built by Homebrew for macOS 14 (arm64)",
        "unless an entry says otherwise. The libraries are dynamically linked, each a file of its",
        "own in Contents/Frameworks or Contents/Resources/lib, so you may replace one with your",
        f"own build of the same library and then sign the app again: codesign --force --deep -s - {title}.app",
        "Each library's licence texts are in this folder, a subfolder per Homebrew formula. Its",
        "source is the upstream release listed, and its build recipe is the one named.",
        "",
    ]
    for name in sorted(entries):
        e = entries[name]
        version = e["version"]
        if e["stable"] and not version.split("_")[0] == e["stable"]:
            version += f" (formula's current release {e['stable']})"
        out.append(f"{name} {version}: {e['spdx']}")
        out.extend(f"  {note}" for note in e["notes"])
        out.append(f"  Source:   {e['source']}")
        out.append(f"  Recipe:   {e['recipe']}")
        out.append(f"  Licences: {', '.join(e['licences'])}")
        out.append(f"  In {title}.app: {', '.join(e['files'])}")
        out.append("")
    if fonts:
        out.append("Fonts (Resources/fonts): Liberation 2.1.5, Carlito, and Caladea, under the SIL Open Font")
        out.append(f"License 1.1; their licence texts are beside them: {', '.join('../fonts/' + f for f in fonts)}.")
        out.append("")
    return "\n".join(out)


def spdx_tokens(expr):
    return re.findall(r"\(|\)|[^\s()]+", expr)


def without_gpl(expr):
    """Whether SPDX expression `expr` can be met without the GPL: a GPL or AGPL licence
    (LGPL is fine) fails a term, AND needs every term, OR one of them."""
    tokens = spdx_tokens(expr)
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else None

    def take():
        nonlocal pos
        if pos >= len(tokens):
            raise ValueError(f"SPDX expression {expr!r} ends early")
        pos += 1
        return tokens[pos - 1]

    def atom():
        if peek() == "(":
            take()
            ok = any_of()
            if take() != ")":
                raise ValueError(f"unbalanced SPDX expression {expr!r}")
            return ok
        name = take()
        if peek() == "WITH":
            take()
            take()
        return not name.upper().startswith(("GPL-", "AGPL-"))

    def all_of():
        ok = atom()
        while peek() == "AND":
            take()
            ok = atom() and ok
        return ok

    def any_of():
        ok = all_of()
        while peek() == "OR":
            take()
            ok = all_of() or ok
        return ok

    ok = any_of()
    if pos != len(tokens):
        raise ValueError(f"can't read SPDX expression {expr!r}")
    return ok


def brew(*args):
    return subprocess.run(["brew", *args], capture_output=True, text=True, check=True).stdout


def read_record(path):
    """scripts/mac_cairo.sh's cronkite-build.txt: key=value lines."""
    return dict(line.split("=", 1) for line in open(path).read().splitlines() if "=" in line)


def main(contents, sources, cairo):
    prefix = brew("--prefix").strip()
    cellar = brew("--cellar").strip()
    info = {f["name"]: f for f in json.loads(brew("info", "--json=v2", "--installed"))["formulae"]}
    res = os.path.join(contents, "Resources")
    title = os.path.basename(os.path.dirname(os.path.abspath(contents)))[: -len(".app")]
    built = read_record(os.path.join(cairo, "cronkite-build.txt"))
    files = {}  # (formula, version) -> what the app carries of it
    problems = []

    def add(formula, what):
        files.setdefault(formula, []).append(what)

    frameworks = os.path.join(contents, "Frameworks")
    for base in sorted(os.listdir(frameworks)):
        if os.path.exists(os.path.join(cairo, "lib", base)):
            add(("cairo", built["version"]), f"Frameworks/{base}")
            continue
        hits = {cellar_formula(p, cellar) for p in glob.glob(os.path.join(prefix, "opt", "*", "lib", base))}
        hits.discard(None)
        if not hits:
            hits = {cellar_formula(p, cellar) for p in glob.glob(os.path.join(cellar, "*", "*", "lib", "**", base), recursive=True)}
            hits.discard(None)
        if len(hits) != 1:
            problems.append(f"Frameworks/{base}: {'formulas ' + str(sorted(hits)) if hits else 'from no Homebrew formula'}")
            continue
        add(hits.pop(), f"Frameworks/{base}")
    for line in open(sources).read().splitlines():
        base, src = line.split("\t")
        formula = cellar_formula(src, cellar)
        if formula is None:
            problems.append(f"loader {base}: {src} is in no Homebrew formula")
            continue
        add(formula, f"Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/{base}")
    for name, what in DATA.items():
        formula = cellar_formula(os.path.join(prefix, "opt", name), cellar)
        if formula is None:
            problems.append(f"{name}: not installed, yet the bundle carries its {what}")
            continue
        add(formula, what)

    dest = os.path.join(res, "licenses")
    shutil.rmtree(dest, ignore_errors=True)
    entries = {}
    for (name, version), what in files.items():
        ours = name == "cairo" and version == built["version"] and any(
            os.path.exists(os.path.join(cairo, "lib", os.path.basename(w))) for w in what)
        keg = os.path.join(cairo, "share", "licenses", "cairo") if ours else os.path.join(cellar, name, version)
        texts = []
        for path in ([os.path.join(keg, n) for n in sorted(os.listdir(keg))] if ours else licence_files(keg)):
            target = os.path.basename(path)
            if target in [t for t, _ in texts]:
                target = f"{os.path.basename(os.path.dirname(path))}-{target}"
            texts.append((target, open(path, "rb").read()))
        if not texts:
            # Homebrew installs only top-level licence files; take them from the source.
            subprocess.run(["brew", "fetch", "--quiet", "--build-from-source", name], check=False,
                           stdout=subprocess.DEVNULL)
            tarball = brew("--cache", "--build-from-source", name).strip()
            if os.path.isfile(tarball) and tarfile.is_tarfile(tarball):
                texts = source_licences(tarball)
        if not texts:
            problems.append(f"{name} {version}: no licence file in {keg} or its source")
            continue
        os.makedirs(os.path.join(dest, name))
        copied = []
        for target, data in texts:
            with open(os.path.join(dest, name, target), "wb") as out:
                out.write(data)
            copied.append(target)
        f = info.get(name, {})
        spdx = f.get("license")
        notes = []
        if ours:
            spdx = spdx or CAIRO_SPDX
            notes.append(f"Built for {title}.app without lzo (GPL-2.0-or-later), with meson {built['options']}")
            source, recipe = built["source"] + f" (sha256 {built['sha256']})", CAIRO_RECIPE
        else:
            source = ((f.get("urls") or {}).get("stable") or {}).get("url") or f.get("homepage") or "unknown"
            recipe = f"https://formulae.brew.sh/formula/{name}"
        library = LIBRARY_LICENCE.get(name)
        if library and spdx and {os.path.basename(w) for w in what} <= library[1]:
            notes.append(f"Formula's licence {spdx}, which covers tools {title}.app doesn't carry; {library[2]}")
            spdx = library[0]
        if not spdx:
            problems.append(f"{name} {version}: no SPDX licence in Homebrew to check")
        elif not without_gpl(spdx):
            problems.append(f"{name} {version}: {spdx} needs the GPL, and {title}.app ships no GPL code")
        entries[name] = {
            "version": version,
            "stable": None if ours else (f.get("versions") or {}).get("stable"),
            "spdx": spdx,
            "source": source,
            "recipe": recipe,
            "notes": notes,
            "files": sorted(what),
            "licences": [f"{name}/{t}" for t in copied],
        }
    if problems:
        print("mac_licenses: " + "\n  ".join(["no notice, or a licence the bundle can't carry:", *problems]), file=sys.stderr)
        return 1
    own = sorted(f for f in os.listdir(res) if f.startswith("LICENSE"))
    fonts_dir = os.path.join(res, "fonts")
    fonts = sorted(f for f in os.listdir(fonts_dir) if f.endswith(".txt")) if os.path.isdir(fonts_dir) else []
    with open(os.path.join(dest, "THIRD-PARTY.txt"), "w") as out:
        out.write(render(title, own, entries, fonts))
    for name in sorted(entries):
        print(f"licence: {name} {entries[name]['version']}: {entries[name]['spdx']} ({len(entries[name]['licences'])} files)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__.split("\n\n")[1])
    sys.exit(main(*sys.argv[1:]))
