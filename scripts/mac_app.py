#!/usr/bin/env python3
"""Wrap an app's release binary in a minimal macOS app bundle (MAC-5).

    python3 scripts/mac_app.py APP BIN_DIR DEST     # e.g. slate target/release out/apps

makes DEST/<Name>.app with:
- Contents/Info.plist: CFBundleIdentifier dev.cronkite.<Name> (the folder names of
  MAC-2's ~/Library folders), the executable, and CFBundleDocumentTypes for the file
  types in the app's Linux desktop entry (packaging/dev.cronkite.<Name>.desktop), so
  Launch Services opens those files with it (`open -a`, Finder's Open With);
- Contents/MacOS/<app>: the binary, still linked to Homebrew's GTK;
- Contents/Resources/fonts: target/fonts (scripts/fetch_fonts.py), if it's there.

It prints the bundle's path. Enough for Launch Services and MAC-5's smoke test
(scripts/check_mac_open.sh); MAC-7 makes the real, self-contained bundles.
"""

import plistlib
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The MIME types of the desktop entries as Apple's uniform type identifiers.
UTIS = {
    "text/csv": "public.comma-separated-values-text",
    "text/tab-separated-values": "public.tab-separated-values-text",
    "text/plain": "public.plain-text",
    "text/markdown": "net.daringfireball.markdown",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": "org.openxmlformats.spreadsheetml.sheet",
    "application/vnd.ms-excel.sheet.macroEnabled.12": "org.openxmlformats.spreadsheetml.sheet.macroenabled",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "org.openxmlformats.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": "org.openxmlformats.presentationml.presentation",
    "application/vnd.ms-powerpoint.presentation.macroEnabled.12": "org.openxmlformats.presentationml.presentation.macroenabled",
    "image/png": "public.png",
    "image/jpeg": "public.jpeg",
    "image/bmp": "com.microsoft.bmp",
}


def desktop_entry(app):
    """The app's display name and MIME types, from its desktop entry."""
    for path in (ROOT / "packaging").glob("dev.cronkite.*.desktop"):
        fields = dict(
            line.split("=", 1) for line in path.read_text().splitlines() if "=" in line
        )
        if fields.get("Exec", "").split()[0] == app:
            mimes = [m for m in fields.get("MimeType", "").split(";") if m]
            return fields["Name"], mimes
    raise SystemExit(f"mac_app: no desktop entry runs {app!r}")


def version(app):
    """The app's crate version, or the workspace's when it inherits that."""
    for toml in (ROOT / "apps" / app / "Cargo.toml", ROOT / "Cargo.toml"):
        for line in toml.read_text().splitlines():
            if line.startswith("version = "):
                return line.split('"')[1]
    raise SystemExit(f"mac_app: no version for {app!r}")


def info_plist(app):
    name, mimes = desktop_entry(app)
    return {
        "CFBundleExecutable": app,
        "CFBundleIdentifier": f"dev.cronkite.{name}",
        "CFBundleName": name,
        "CFBundleDisplayName": name,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version(app),
        "CFBundleVersion": version(app),
        "LSMinimumSystemVersion": "14.0",
        "NSHighResolutionCapable": True,
        "CFBundleDocumentTypes": [
            {
                "CFBundleTypeName": f"{name} Document",
                "CFBundleTypeRole": "Editor",
                "LSHandlerRank": "Alternate",
                "LSItemContentTypes": [UTIS[m] for m in mimes],
            }
        ],
    }


def bundle(app, bin_dir, dest):
    info = info_plist(app)
    path = Path(dest) / f"{info['CFBundleName']}.app"
    shutil.rmtree(path, ignore_errors=True)
    (path / "Contents" / "MacOS").mkdir(parents=True)
    with open(path / "Contents" / "Info.plist", "wb") as f:
        plistlib.dump(info, f)
    shutil.copy2(Path(bin_dir) / app, path / "Contents" / "MacOS" / app)
    fonts = ROOT / "target" / "fonts"
    if fonts.is_dir():
        shutil.copytree(fonts, path / "Contents" / "Resources" / "fonts")
    return path


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__.split("\n\n")[1])
    print(bundle(*sys.argv[1:]))
