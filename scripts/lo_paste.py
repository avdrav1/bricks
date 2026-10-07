#!/usr/bin/env python3
"""Paste clipboard content into LibreOffice Calc and print the cells it made (CLIP-1).

Starts a headless Calc (UNO), puts the given files on its clipboard under the given
flavors, runs Edit > Paste into A1 of a new sheet, and prints the used area: one line per
row, cells split by tabs, each `kind:text` where kind is value, text, formula, or empty,
and text escapes backslash, tab, and newline as \\\\, \\t, \\n.

Usage: python3 scripts/lo_paste.py html|text|html+text FILE [FILE]
  (one file per flavor, in the same order; needs LibreOffice with Python UNO)
Run by `cargo test -p data-model --test paste_libreoffice -- --ignored`.
"""
import subprocess
import sys
import tempfile
import time

import uno
import unohelper
from com.sun.star.datatransfer import DataFlavor, XTransferable

PORT = 2099


def flavor(mime, name, type_name):
    f = DataFlavor()
    f.MimeType = mime
    f.HumanPresentableName = name
    f.DataType = uno.getTypeByName(type_name)
    return f


def base(mime):
    return mime.split(";")[0]


class Offer(unohelper.Base, XTransferable):
    """Clipboard content: (flavor, value) pairs, offered in order."""

    def __init__(self, items):
        self.items = items

    def getTransferData(self, f):
        for fl, value in self.items:
            if base(fl.MimeType) == base(f.MimeType):
                return value
        raise RuntimeError("unsupported flavor " + f.MimeType)

    def getTransferDataFlavors(self):
        return tuple(fl for fl, _ in self.items)

    def isDataFlavorSupported(self, f):
        return any(base(fl.MimeType) == base(f.MimeType) for fl, _ in self.items)


def escape(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n")


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    items = []
    for kind, path in zip(sys.argv[1].split("+"), sys.argv[2:]):
        data = open(path, "rb").read()
        if kind == "html":
            items.append((flavor("text/html", "HTML", "[]byte"), uno.ByteSequence(data)))
        elif kind == "text":
            items.append((flavor("text/plain;charset=utf-16", "Unicode text", "string"),
                          data.decode("utf-8")))
        else:
            sys.exit(f"unknown flavor {kind}")

    profile = tempfile.mkdtemp(prefix="lo-paste-")
    proc = subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--norestore", "--nologo",
         f"-env:UserInstallation=file://{profile}",
         f"--accept=socket,host=localhost,port={PORT};urp;"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    desktop = None
    try:
        local = uno.getComponentContext()
        resolver = local.ServiceManager.createInstanceWithContext(
            "com.sun.star.bridge.UnoUrlResolver", local)
        ctx = None
        for _ in range(150):
            try:
                ctx = resolver.resolve(
                    f"uno:socket,host=localhost,port={PORT};urp;StarOffice.ComponentContext")
                break
            except Exception:
                time.sleep(0.2)
        if ctx is None:
            sys.exit("LibreOffice did not start")
        smgr = ctx.ServiceManager
        desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
        doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, ())
        clip = smgr.createInstanceWithContext(
            "com.sun.star.datatransfer.clipboard.SystemClipboard", ctx)
        clip.setContents(Offer(items), None)
        dispatcher = smgr.createInstanceWithContext("com.sun.star.frame.DispatchHelper", ctx)
        dispatcher.executeDispatch(doc.getCurrentController().getFrame(), ".uno:Paste", "", 0, ())
        sheet = doc.Sheets.getByIndex(0)
        cursor = sheet.createCursor()
        cursor.gotoEndOfUsedArea(False)
        end = cursor.getRangeAddress()
        kinds = {"EMPTY": "empty", "VALUE": "value", "TEXT": "text", "FORMULA": "formula"}
        for r in range(end.EndRow + 1):
            cells = []
            for c in range(end.EndColumn + 1):
                cell = sheet.getCellByPosition(c, r)
                cells.append(f"{kinds[cell.getType().value]}:{escape(cell.getString())}")
            print("\t".join(cells))
        doc.close(True)
    finally:
        if desktop is not None:
            try:
                desktop.terminate()
            except Exception:
                pass
        proc.terminate()
        proc.wait(timeout=30)
        subprocess.run(["rm", "-rf", profile], check=False)


main()
