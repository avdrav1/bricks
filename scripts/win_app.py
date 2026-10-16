#!/usr/bin/env python3
"""An app's Windows installer settings and icon (WIN-5).

    python3 scripts/win_app.py iss APP OUT       # Inno Setup include for APP -> OUT
    python3 scripts/win_app.py ico OUT PNG...    # PNGs (one per size) -> OUT, a .ico
    python3 scripts/win_app.py name APP          # prints the display name (Brick)
    python3 scripts/win_app.py version APP       # prints the version (0.3.0)

`iss` writes what packaging/windows/setup.iss includes for APP: its name, version, id
(dev.cronkite.<Name>), description and site as #defines, and the [Registry] entries that
register its file types for the user (HKCU), from the MIME types of its Linux desktop
entry (packaging/dev.cronkite.<Name>.desktop):
- a ProgID per extension, Cronkite.<Name>.<ext> (description, icon, open command),
  listed under the extension's OpenWithProgids, so Open with offers the app; the
  extension's own default is never written, so Windows keeps the user's choice;
- Applications\\<app>.exe with FriendlyAppName and SupportedTypes, so "Choose another app"
  lists it by name for those types only;
- Software\\Cronkite\\<Name>\\Capabilities (FileAssociations) and its value under
  RegisteredApplications, so Settings → Default apps lists it.
Each entry carries the flag that undoes it at uninstall: the keys the installer owns go
whole, the values it adds to shared keys go alone, and shared keys go only when left
empty.

`ico` writes a Windows icon whose images are the given PNGs (Windows Vista and later read
PNG images in .ico files); each PNG must be square and at most 256 px.
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mac_app  # noqa: E402  (the desktop entry and version readers, shared with macOS)

# The extensions of each MIME type in the desktop entries, and what Windows calls the type.
TYPES = {
    "text/csv": ([".csv"], "CSV file"),
    "text/tab-separated-values": ([".tsv"], "Tab-separated values file"),
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": ([".xlsx"], "Excel workbook"),
    "application/vnd.ms-excel.sheet.macroEnabled.12": ([".xlsm"], "Excel macro-enabled workbook"),
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": ([".docx"], "Word document"),
    "text/markdown": ([".md", ".markdown"], "Markdown document"),
    "text/plain": ([".txt"], "Text document"),
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": ([".pptx"], "PowerPoint presentation"),
    "application/vnd.ms-powerpoint.presentation.macroEnabled.12": ([".pptm"], "PowerPoint macro-enabled presentation"),
    "image/png": ([".png"], "PNG image"),
    "image/jpeg": ([".jpg", ".jpeg"], "JPEG image"),
    "image/bmp": ([".bmp"], "Bitmap image"),
}

SITES = {"brick": "https://brick.cronkite.cc", "slate": "https://slate.cronkite.cc",
         "frame": "https://frame.cronkite.cc", "etch": "https://etch.cronkite.cc"}


def comment(app):
    """The desktop entry's Comment: the app's one-line description."""
    for path in (mac_app.ROOT / "packaging").glob("dev.cronkite.*.desktop"):
        fields = dict(line.split("=", 1) for line in path.read_text().splitlines() if "=" in line)
        if fields.get("Exec", "").split()[0] == app:
            return fields["Comment"]
    raise SystemExit(f"win_app: no desktop entry runs {app!r}")


def extensions(app):
    """[(extension, description)] for APP's file types, in the desktop entry's order."""
    _, mimes = mac_app.desktop_entry(app)
    out = []
    for mime in mimes:
        if mime not in TYPES:
            raise SystemExit(f"win_app: no Windows extension for {mime!r} ({app})")
        exts, description = TYPES[mime]
        out.extend((ext, description) for ext in exts)
    return out


def iss_string(text):
    """A value for an Inno Setup define or parameter: double quotes doubled."""
    return text.replace('"', '""')


def registry(app, name):
    """The [Registry] section's lines registering APP's file types for the user."""
    exe = f"{app}.exe"
    run = '"""{app}\\bin\\' + exe + '"" ""%1"""'
    icon = '"{app}\\' + app + '.ico"'
    caps = f"Software\\Cronkite\\{name}\\Capabilities"
    lines = ["[Registry]"]

    def key(subkey, flags):
        lines.append(f'Root: HKCU; Subkey: "{subkey}"; Flags: {flags}')

    def value(subkey, vname, data, flags=None, vtype="string"):
        line = f'Root: HKCU; Subkey: "{subkey}"; ValueType: {vtype}; ValueName: "{vname}"; ValueData: {data}'
        lines.append(line + (f"; Flags: {flags}" if flags else ""))

    # Software\Cronkite\<Name> is the app's own; Software\Cronkite goes when it's empty.
    key("Software\\Cronkite", "uninsdeletekeyifempty")
    key(f"Software\\Cronkite\\{name}", "uninsdeletekey")
    value(caps, "ApplicationName", f'"{name}"')
    value(caps, "ApplicationDescription", f'"{iss_string(comment(app))}"')
    value(caps, "ApplicationIcon", '"{app}\\' + app + '.ico,0"')
    value("Software\\RegisteredApplications", f"Cronkite {name}", f'"{caps}"',
          "uninsdeletevalue uninsdeletekeyifempty")
    # Classes\Applications is shared, and new on a fresh profile: it goes when left empty.
    key("Software\\Classes\\Applications", "uninsdeletekeyifempty")
    apps = f"Software\\Classes\\Applications\\{exe}"
    key(apps, "uninsdeletekey")
    value(apps, "FriendlyAppName", f'"{name}"')
    value(f"{apps}\\DefaultIcon", "", icon)
    value(f"{apps}\\shell\\open\\command", "", run)
    for ext, description in extensions(app):
        progid = f"Cronkite.{name}{ext}"
        value(f"{caps}\\FileAssociations", ext, f'"{progid}"')
        value(f"{apps}\\SupportedTypes", ext, '""')
        key(f"Software\\Classes\\{progid}", "uninsdeletekey")
        value(f"Software\\Classes\\{progid}", "", f'"{description}"')
        value(f"Software\\Classes\\{progid}\\DefaultIcon", "", icon)
        value(f"Software\\Classes\\{progid}\\shell\\open\\command", "", run)
        # Shared with every program that opens the type: only our value is ours, and the
        # keys go when nothing else is left in them. Uninstall undoes entries last to first.
        key(f"Software\\Classes\\{ext}", "uninsdeletekeyifempty")
        value(f"Software\\Classes\\{ext}\\OpenWithProgids", progid, '""',
              "uninsdeletevalue uninsdeletekeyifempty", "none")
    return lines


def iss(app):
    """packaging/windows/setup.iss's include for APP."""
    name, _ = mac_app.desktop_entry(app)
    version = mac_app.version(app)
    defines = {
        "App": app,
        "Name": name,
        "Version": version,
        "AppId": f"dev.cronkite.{name}",
        "Description": comment(app),
        "Url": SITES[app],
        "Types": ", ".join(ext for ext, _ in extensions(app)),
    }
    lines = [f"; {name}'s settings for packaging/windows/setup.iss, written by scripts/win_app.py."]
    lines += [f'#define {k} "{iss_string(v)}"' for k, v in defines.items()]
    lines.append("")
    lines += registry(app, name)
    return "\n".join(lines) + "\n"


def png_size(data):
    """(width, height) of PNG bytes."""
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        raise SystemExit("win_app: not a PNG")
    return struct.unpack(">II", data[16:24])


def ico(pngs):
    """A .ico file holding the PNGs, smallest first."""
    images = []
    for data in pngs:
        w, h = png_size(data)
        if w != h or w > 256:
            raise SystemExit(f"win_app: an icon image must be square and at most 256 px, not {w}x{h}")
        images.append((w, data))
    images.sort(key=lambda image: image[0])
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for w, data in images:
        side = 0 if w == 256 else w  # 0 means 256
        out += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(data for _, data in images)


def main(argv):
    if len(argv) >= 3 and argv[0] == "iss":
        Path(argv[2]).write_text(iss(argv[1]), encoding="utf-8")
    elif len(argv) >= 3 and argv[0] == "ico":
        Path(argv[1]).write_bytes(ico([Path(p).read_bytes() for p in argv[2:]]))
    elif len(argv) == 2 and argv[0] == "name":
        print(mac_app.desktop_entry(argv[1])[0])
    elif len(argv) == 2 and argv[0] == "version":
        print(mac_app.version(argv[1]))
    else:
        raise SystemExit(__doc__.split("\n\n")[1])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
