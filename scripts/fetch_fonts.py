#!/usr/bin/env python3
"""Fetch the metric-compatible fonts the apps carry on macOS (MAC-6).

Liberation (Arial, Times New Roman, Courier New), Carlito (Calibri), and Caladea
(Cambria) come from pinned upstream releases, each file checked against its SHA-256,
into one flat folder with their licence texts:

    python3 scripts/fetch_fonts.py [DEST]     # DEST defaults to target/fonts

On macOS each app looks for this folder next to its executable: `../Resources/fonts`
inside an app bundle (MAC-7 copies it there), else `../fonts`, which is `target/fonts`
for `target/{debug,release}/<app>`. Linux uses the system's fonts and never reads it.
Files already in DEST with the right checksum aren't fetched again.
"""

import hashlib
import io
import sys
import tarfile
import urllib.request
from pathlib import Path

# google/fonts commits that last changed ofl/carlito and ofl/caladea.
CARLITO = "https://raw.githubusercontent.com/google/fonts/3dd78844021e948ceb633d1dcee3f7885561b5d9/ofl/carlito/"
CALADEA = "https://raw.githubusercontent.com/google/fonts/13c010f766052c13fa9cc8c656008cee55fb4775/ofl/caladea/"
LIBERATION = "https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz"
LIBERATION_SHA256 = "7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0"
LIBERATION_DIR = "liberation-fonts-ttf-2.1.5/"

# Downloaded one by one: name in DEST -> (URL, SHA-256).
FILES = {
    "Carlito-Regular.ttf": (CARLITO + "Carlito-Regular.ttf", "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595"),
    "Carlito-Bold.ttf": (CARLITO + "Carlito-Bold.ttf", "bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db"),
    "Carlito-Italic.ttf": (CARLITO + "Carlito-Italic.ttf", "0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c"),
    "Carlito-BoldItalic.ttf": (CARLITO + "Carlito-BoldItalic.ttf", "b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517"),
    "OFL-Carlito.txt": (CARLITO + "OFL.txt", "58402f82a7c332a700294988fe7554fbb0a63a8d27ccc1ee3bbc640311990a00"),
    "Caladea-Regular.ttf": (CALADEA + "Caladea-Regular.ttf", "f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd"),
    "Caladea-Bold.ttf": (CALADEA + "Caladea-Bold.ttf", "ae3cb2dcbc925809dd29d2a44e9802211cab66be541bacbfc9c08c74b27c3742"),
    "Caladea-Italic.ttf": (CALADEA + "Caladea-Italic.ttf", "4359a8e24f748b6447b1ff6d7a174febe70961d29f8bb8634b56dacd740a3deb"),
    "Caladea-BoldItalic.ttf": (CALADEA + "Caladea-BoldItalic.ttf", "ccabaa7b7e2fdf253d2b1a5fa699dd8a3df8d835a9eb285ad82631a677eb76c0"),
    "OFL-Caladea.txt": (CALADEA + "OFL.txt", "ccdab61d371d8c8683a128a92cd7d498dbdb1d37689f7cb21f1bf6b16658d213"),
}

# Taken from the Liberation tarball: name in DEST -> (member, SHA-256).
LIBERATION_FILES = {
    "LiberationSans-Regular.ttf": ("LiberationSans-Regular.ttf", "76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8"),
    "LiberationSans-Bold.ttf": ("LiberationSans-Bold.ttf", "788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173"),
    "LiberationSans-Italic.ttf": ("LiberationSans-Italic.ttf", "e5bae5c4cde31f22142753855f4f8fb86da6ff39955ed3c0a11248b0d16948b0"),
    "LiberationSans-BoldItalic.ttf": ("LiberationSans-BoldItalic.ttf", "698da70fc191cc5f33ad4d6d3fe830fe4624b898ea2e3169955928b7c491f1ee"),
    "LiberationSerif-Regular.ttf": ("LiberationSerif-Regular.ttf", "058ea80864aef09a23f45cbec2bb5400bc3dfbdea01c3f10538a21fcb497fb74"),
    "LiberationSerif-Bold.ttf": ("LiberationSerif-Bold.ttf", "d754ba427cfe0bca54ae052384baa8f842da5bd6550ad4da024ac441e7a7d5ce"),
    "LiberationSerif-Italic.ttf": ("LiberationSerif-Italic.ttf", "0e3dea9f8d613e006ccfa62201f33e265d19167bd0907725c3e145368b04fc2e"),
    "LiberationSerif-BoldItalic.ttf": ("LiberationSerif-BoldItalic.ttf", "f17db8af71e24d2066b587546021d4f0b296be389512b658dec3c09affeb11a7"),
    "LiberationMono-Regular.ttf": ("LiberationMono-Regular.ttf", "f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1"),
    "LiberationMono-Bold.ttf": ("LiberationMono-Bold.ttf", "bd62a0672d0b9b6710b01df434c80ad54fa5f0835207eb7b17b7a761463067bb"),
    "LiberationMono-Italic.ttf": ("LiberationMono-Italic.ttf", "605c01c711b44480a7508d349dfbf3264e81fa43d69e61cfa7d10b86e764c4d1"),
    "LiberationMono-BoldItalic.ttf": ("LiberationMono-BoldItalic.ttf", "79451f3c09fe25116098853b7a2ca6e2436220ccc11af022979adbcf195be130"),
    "LICENSE-Liberation.txt": ("LICENSE", "93fed46019c38bbe566b479d22148e2e8a1e85ada614accb0211c37b2c61c19b"),
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def checked(name, data, expected):
    """`data` if its SHA-256 is `expected`; otherwise stop with an error naming `name`."""
    got = sha256(data)
    if got != expected:
        raise SystemExit(f"fetch_fonts: {name}: SHA-256 {got}, expected {expected}")
    return data


def present(path, expected):
    """Whether `path` already holds the file with SHA-256 `expected`."""
    return path.is_file() and sha256(path.read_bytes()) == expected


def download(url):
    with urllib.request.urlopen(url, timeout=60) as reply:
        return reply.read()


def write(path, data):
    tmp = path.with_name(path.name + ".part")
    tmp.write_bytes(data)
    tmp.replace(path)


def fetch(dest, get=download):
    """Fill `dest` with every font and licence; returns how many files were written."""
    dest.mkdir(parents=True, exist_ok=True)
    written = 0
    for name, (url, expected) in FILES.items():
        if not present(dest / name, expected):
            write(dest / name, checked(name, get(url), expected))
            written += 1
    missing = {n: v for n, v in LIBERATION_FILES.items() if not present(dest / n, v[1])}
    if missing:
        archive = checked("Liberation tarball", get(LIBERATION), LIBERATION_SHA256)
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
            for name, (member, expected) in missing.items():
                data = tar.extractfile(LIBERATION_DIR + member).read()
                write(dest / name, checked(name, data, expected))
                written += 1
    return written


def main(argv):
    if len(argv) > 2 or (len(argv) == 2 and argv[1].startswith("-")):
        raise SystemExit("usage: fetch_fonts.py [DEST]")
    dest = Path(argv[1] if len(argv) == 2 else "target/fonts")
    written = fetch(dest)
    total = len(FILES) + len(LIBERATION_FILES)
    print(f"fetch_fonts: {dest}: {total} files, {written} fetched")


if __name__ == "__main__":
    main(sys.argv)
