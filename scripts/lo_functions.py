#!/usr/bin/env python3
"""Record formula results from LibreOffice Calc (FML-2, FML-5..8).

Reads apps/bricks/crates/formula/tests/data/cases.tsv (columns: group, formula, inputs, excel, why) and
writes apps/bricks/crates/formula/tests/data/cases-libreoffice.tsv (columns: group, formula, inputs,
kind, result, excel, why).

- `formula` is written as Bricks reads it (with `,` between arguments); this script turns
  it into LibreOffice's API grammar (`;` between arguments, `{1;2|3;4}` arrays).
- `inputs` fills cells of a fresh sheet before the formula goes into G1: `A1=2;B1=abc`,
  with `\\;` for a literal semicolon. Each value is entered the way Bricks reads a raw
  cell: the plain number grammar as a number, TRUE/FALSE as booleans, ISO dates as
  dates, `#N/A`-style errors as that error, anything else as text.
- `excel` (optional) is `kind:result` where Bricks deliberately follows Excel instead of
  LibreOffice, and `why` says why; the test checks Bricks against it and that
  LibreOffice's recorded result really differs.
- `kind` is value, text, bool, or error; `result` is repr(float) for values (dates as
  serial numbers), the text for text and errors, TRUE/FALSE for bools.

Usage: python3 scripts/lo_functions.py            (re-record the fixture)
       python3 scripts/lo_functions.py --try F...  (print results of ad-hoc formulas)
Needs LibreOffice with Python UNO.
"""
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno

PORT = int(os.environ.get("LO_PORT", "2099"))  # set it to run several at once
ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "apps/bricks/crates/formula/tests/data"
NUMBER = re.compile(r"^[+-]?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?$")
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?)?$")
ERRORS = {
    "#N/A": "=NA()",
    "#DIV/0!": "=1/0",
    "#VALUE!": '="a"+1',
    "#NUM!": "=SQRT(-1)",
    "#NAME?": "=NOSUCHFUNCTION()",
    "#REF!": "=INDIRECT(\"#\")",
}


def api_grammar(formula):
    """Bricks' `SUM(1,2)` / `{1,2;3,4}` as LibreOffice's `SUM(1;2)` / `{1;2|3;4}`."""
    out, i, depth = [], 0, 0
    while i < len(formula):
        c = formula[i]
        if c in "\"'":
            j = i + 1
            while j < len(formula):
                if formula[j] == c:
                    if j + 1 < len(formula) and formula[j + 1] == c:
                        j += 2
                        continue
                    break
                j += 1
            out.append(formula[i:j + 1])
            i = j + 1
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        if c == ",":
            out.append(";")
        elif c == ";" and depth > 0:
            out.append("|")
        else:
            out.append(c)
        i += 1
    return "".join(out)


def split_inputs(text):
    parts, cur, i = [], "", 0
    while i < len(text):
        if text[i] == "\\" and i + 1 < len(text) and text[i + 1] == ";":
            cur += ";"
            i += 2
            continue
        if text[i] == ";":
            parts.append(cur)
            cur = ""
        else:
            cur += text[i]
        i += 1
    if cur:
        parts.append(cur)
    return [p.split("=", 1) for p in parts]


def put(cell, raw):
    if raw == "":
        return
    if NUMBER.match(raw):
        cell.setValue(float(raw))
    elif raw.upper() in ("TRUE", "FALSE"):
        cell.setFormula("=" + raw.upper() + "()")
    elif DATE.match(raw):
        cell.setFormula(raw.replace("T", " "))
    elif raw.upper() in ERRORS:
        cell.setFormula(ERRORS[raw.upper()])
    else:
        cell.setString(raw)


def connect():
    profile = tempfile.mkdtemp(prefix="lo-functions-")
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


def evaluate(doc, n, formula, inputs):
    """The (kind, result) of `formula` on a fresh sheet holding `inputs`."""
    sheets = doc.Sheets
    name = f"case{n}"
    sheets.insertNewByName(name, sheets.getCount())
    sheet = sheets.getByName(name)
    for ref, raw in split_inputs(inputs):
        put(sheet.getCellRangeByName(ref), raw)
    cell = sheet.getCellByPosition(6, 0)
    cell.setFormula("=" + api_grammar(formula))
    doc.calculateAll()
    kind = cell.FormulaResultType2
    text = cell.getString()
    if cell.getError() != 0:
        result = ("error", text if text.startswith("#") else f"Err:{cell.getError()}")
    elif kind == 2:
        result = ("text", text)
    elif text in ("TRUE", "FALSE") and cell.getValue() in (0.0, 1.0):
        result = ("bool", text)
    else:
        result = ("value", repr(cell.getValue()))
    sheets.removeByName(name)
    return result


def main():
    ctx = connect()
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, ())
        # Excel's comparison rules: case-insensitive text, wildcards, no regular expressions.
        doc.IgnoreCase = True
        doc.RegularExpressions = False
        doc.Wildcards = True
        if sys.argv[1:2] == ["--try"]:
            for n, f in enumerate(sys.argv[2:]):
                formula, _, inputs = f.partition("\t")
                print(f, *evaluate(doc, n, formula, inputs), sep="\t")
            return
        rows = []
        lines = (DATA / "cases.tsv").read_text().splitlines()
        for n, line in enumerate(lines[1:]):
            if not line.strip() or line.startswith("#"):
                continue
            cols = line.split("\t")
            group, formula = cols[0], cols[1]
            cols += [""] * (5 - len(cols))
            inputs, excel, why = cols[2], cols[3], cols[4]
            kind, result = evaluate(doc, n, formula, inputs)
            rows.append("\t".join([group, formula, inputs, kind, result, excel, why]))
        out = DATA / "cases-libreoffice.tsv"
        out.write_text("group\tformula\tinputs\tkind\tresult\texcel\twhy\n" + "\n".join(rows) + "\n")
        print(f"{len(rows)} cases -> {out.relative_to(ROOT)}")
        doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass


if __name__ == "__main__":
    main()
