#!/usr/bin/env python3
"""Record how LibreOffice Calc shows values under Excel number format codes (STYLE-2).

Reads crates/formula/tests/data/formats.tsv (columns: code, value, excel, why) and writes
crates/formula/tests/data/formats-libreoffice.tsv (columns: code, value, text, color,
excel, why).

- `value` is a number (set with setValue), an ISO date or date-time (setValue of its serial,
  days since 1899-12-30), TRUE/FALSE (the formula =TRUE()/=FALSE()), or `text:...`
  (setString of the rest).
- The cell's NumberFormat is the code, added for the en-US locale; `text` is the cell's
  getString(). A code LibreOffice rejects (addNew throws) is recorded as `#REJECTED`.
- `color` is the colour LibreOffice's formatter picks for the value (queryColorForNumber /
  queryColorForString) as six hex digits, or empty for none.
- Characters below U+0020 in `text` are written as `\\u{XX}`, and a backslash as `\\\\`.
- `excel` (optional) is the text Excel shows where Bricks deliberately follows Excel instead
  of LibreOffice, and `why` says why; the test checks Bricks against it and that
  LibreOffice's recorded text really differs.

Usage: python3 scripts/lo_formats.py               (re-record the fixture)
       python3 scripts/lo_formats.py --try 'CODE<TAB>VALUE' ...   (print ad-hoc results)
Needs LibreOffice with Python UNO; LO_PORT (default 2100) picks the port.
"""
import datetime
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.lang import Locale

PORT = int(os.environ.get("LO_PORT", "2100"))
ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "crates/formula/tests/data"
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?)?$")
NO_COLOR = 0x123456  # a sentinel default no format code names


def connect():
    profile = tempfile.mkdtemp(prefix="lo-formats-")
    subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--norestore", "--nologo",
         f"-env:UserInstallation=file://{profile}",
         f"--accept=socket,host=localhost,port={PORT};urp;"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", local)
    for _ in range(150):
        try:
            return resolver.resolve(
                f"uno:socket,host=localhost,port={PORT};urp;StarOffice.ComponentContext")
        except Exception:
            time.sleep(0.2)
    sys.exit("LibreOffice did not start")


def serial(iso):
    """Days since 1899-12-30 of an ISO date or date-time, the time as the fraction."""
    iso = iso.replace("T", " ")
    fmt = {10: "%Y-%m-%d", 16: "%Y-%m-%d %H:%M", 19: "%Y-%m-%d %H:%M:%S"}.get(len(iso))
    when = (datetime.datetime.strptime(iso, fmt) if fmt
            else datetime.datetime.strptime(iso, "%Y-%m-%d %H:%M:%S.%f"))
    delta = when - datetime.datetime(1899, 12, 30)
    return delta.days + delta.seconds / 86400 + delta.microseconds / 86400e6


def escape(text):
    out = []
    for c in text:
        if c == "\\":
            out.append("\\\\")
        elif ord(c) < 0x20:
            out.append("\\u{%02X}" % ord(c))
        else:
            out.append(c)
    return "".join(out)


class Recorder:
    def __init__(self, ctx, doc):
        self.doc = doc
        self.formats = doc.NumberFormats
        self.locale = Locale("en", "US", "")
        self.formatter = ctx.ServiceManager.createInstanceWithContext(
            "com.sun.star.util.NumberFormatter", ctx)
        self.formatter.attachNumberFormatsSupplier(doc)
        self.cell = doc.Sheets.getByIndex(0).getCellByPosition(0, 0)

    def key(self, code):
        key = self.formats.queryKey(code, self.locale, False)
        if key == -1:
            key = self.formats.addNew(code, self.locale)
        return key

    def record(self, code, value):
        """(text, color) of `value` shown under `code`."""
        try:
            key = self.key(code)
        except Exception:
            return "#REJECTED", ""
        cell = self.cell
        cell.setString("")
        text_value = None
        if value.startswith("text:"):
            text_value = value[5:]
            cell.setString(text_value)
        elif value in ("TRUE", "FALSE"):
            cell.setFormula(f"={value}()")
        elif DATE.match(value):
            cell.setValue(serial(value))
        else:
            cell.setValue(float(value))
        cell.NumberFormat = key
        self.doc.calculateAll()
        text = cell.getString()
        if text_value is not None:
            color = self.formatter.queryColorForString(key, text_value, NO_COLOR)
        else:
            color = self.formatter.queryColorForNumber(key, cell.getValue(), NO_COLOR)
        return escape(text), "" if color == NO_COLOR else "%06X" % (color & 0xFFFFFF)


def main():
    ctx = connect()
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, ())
        rec = Recorder(ctx, doc)
        if sys.argv[1:2] == ["--try"]:
            for arg in sys.argv[2:]:
                code, _, value = arg.partition("\t")
                text, color = rec.record(code, value)
                print(f"{code}\t{value}\t[{text}]\t{color}")
            return
        rows = []
        lines = (DATA / "formats.tsv").read_text().split("\n")
        for line in lines[1:]:
            if not line:
                continue
            cols = line.split("\t")
            cols += [""] * (4 - len(cols))
            code, value, excel, why = cols[:4]
            text, color = rec.record(code, value)
            rows.append("\t".join([code, value, text, color, excel, why]))
        out = DATA / "formats-libreoffice.tsv"
        out.write_text("code\tvalue\ttext\tcolor\texcel\twhy\n" + "\n".join(rows) + "\n")
        print(f"{len(rows)} cases -> {out.relative_to(ROOT)}")
        doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass


if __name__ == "__main__":
    main()
