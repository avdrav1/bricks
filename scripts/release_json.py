#!/usr/bin/env python3
"""An app's latest.json, which install.sh, install.ps1, and the download page read.

    release_json.py write URL VERSION LINUX_TAR MAC_TAR MAC_DMG WIN_SETUP > latest.json
    release_json.py check LATEST_JSON DIST

`write` prints one line: each platform's download under "platforms" ("linux-x86_64": url,
sha256, bytes; "macos-arm64": the same for the tarball the curl installer takes, and dmg,
dmg_sha256, dmg_bytes for the DMG, MAC-7; "windows-x86_64": url, sha256, bytes of the
Setup.exe install.ps1 runs, WIN-6), then Linux's version, date, url, sha256, and bytes at
the top level, last. Installers from before MAC-7 read those top-level fields with a sed
that takes a key's last match on the line, so the order matters. URL is the release's
folder (.../releases/vX.Y.Z); a macOS or Windows file that isn't there is left out.

`check` compares LATEST_JSON with the files in DIST: every download it names is there, with
its sha256 and size, and the top-level fields are Linux's.
"""
import datetime
import hashlib
import json
import os
import sys


def _file(url, path):
    data = open(path, "rb").read()
    return {"url": f"{url}/{os.path.basename(path)}", "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}


def release(url, version, linux_tar, mac_tar, mac_dmg, win_setup, date):
    """The release as a dict in latest.json's key order."""
    linux = _file(url, linux_tar)
    platforms = {"linux-x86_64": linux}
    if os.path.isfile(mac_tar) and os.path.isfile(mac_dmg):
        dmg = _file(url, mac_dmg)
        platforms["macos-arm64"] = dict(_file(url, mac_tar), dmg=dmg["url"], dmg_sha256=dmg["sha256"], dmg_bytes=dmg["bytes"])
    if os.path.isfile(win_setup):
        platforms["windows-x86_64"] = _file(url, win_setup)
    return {"platforms": platforms, "version": version, "date": date, **linux}


def dumps(rel):
    return json.dumps(rel, separators=(",", ":"))


def check(rel, dist):
    """Problems with `rel` against the files in `dist`, as messages; none if it's right."""
    problems = []
    platforms = rel.get("platforms", {})
    if list(rel)[-3:] != ["url", "sha256", "bytes"]:
        problems.append("the top-level url, sha256, and bytes aren't last")
    linux = platforms.get("linux-x86_64")
    if not linux:
        problems.append("no linux-x86_64")
    elif any(rel.get(k) != linux[k] for k in ("url", "sha256", "bytes")):
        problems.append("the top-level url, sha256, and bytes aren't linux-x86_64's")
    files = []
    for name, p in platforms.items():
        files.append((name, p.get("url"), p.get("sha256"), p.get("bytes")))
        if "dmg" in p:
            files.append((name + " dmg", p["dmg"], p.get("dmg_sha256"), p.get("dmg_bytes")))
    for name, url, sha, size in files:
        if not url or not sha or not isinstance(size, int):
            problems.append(f"{name}: url, sha256, or bytes missing")
            continue
        if f"/v{rel.get('version')}/" not in url:
            problems.append(f"{name}: {url} isn't in v{rel.get('version')}'s folder")
        path = os.path.join(dist, os.path.basename(url))
        if not os.path.isfile(path):
            problems.append(f"{name}: {path} missing")
            continue
        data = open(path, "rb").read()
        if hashlib.sha256(data).hexdigest() != sha or len(data) != size:
            problems.append(f"{name}: {path} doesn't match its sha256 and size")
    return problems


def main(argv):
    if len(argv) == 8 and argv[1] == "write":
        date = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
        print(dumps(release(*argv[2:], date)))
        return 0
    if len(argv) == 4 and argv[1] == "check":
        rel = json.load(open(argv[2]))
        problems = check(rel, argv[3])
        for p in problems:
            print(f"FAIL {argv[2]}: {p}")
        if problems:
            return 1
        print(f"ok   {argv[2]}: {rel['version']} for {', '.join(rel['platforms'])}")
        return 0
    print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
