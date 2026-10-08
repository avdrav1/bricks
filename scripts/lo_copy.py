#!/usr/bin/env python3
"""Copy a range in LibreOffice Calc and write the plain text it puts on the clipboard (CLIP-2).

Opens a CSV in a headless Calc (UNO), selects RANGE, runs Edit > Copy, and writes the
clipboard's text flavor to OUT as UTF-8: the TSV a paste from Calc delivers.

Usage: python3 scripts/lo_copy.py CSV RANGE OUT   (e.g. corpus/rows_10mb.csv A2:H10001 out.tsv)
Needs LibreOffice with Python UNO. Run by `cargo test -p commands --test paste_from_libreoffice
-- --ignored`.
"""
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.beans import PropertyValue

PORT = 2098


def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    csv, cells, out = Path(sys.argv[1]).resolve(), sys.argv[2], sys.argv[3]
    profile = tempfile.mkdtemp(prefix="lo-copy-")
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
        # Comma-separated, double quotes, UTF-8 (76), from line 1.
        doc = desktop.loadComponentFromURL(
            uno.systemPathToFileUrl(str(csv)), "_blank", 0,
            (prop("FilterName", "Text - txt - csv (StarCalc)"), prop("FilterOptions", "44,34,76,1")))
        controller = doc.getCurrentController()
        controller.select(doc.Sheets.getByIndex(0).getCellRangeByName(cells))
        dispatcher = smgr.createInstanceWithContext("com.sun.star.frame.DispatchHelper", ctx)
        dispatcher.executeDispatch(controller.getFrame(), ".uno:Copy", "", 0, ())
        clip = smgr.createInstanceWithContext(
            "com.sun.star.datatransfer.clipboard.SystemClipboard", ctx)
        content = clip.getContents()
        text = next(f for f in content.getTransferDataFlavors()
                    if f.MimeType.startswith("text/plain;charset=utf-16"))
        Path(out).write_text(content.getTransferData(text), encoding="utf-8", newline="")
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
