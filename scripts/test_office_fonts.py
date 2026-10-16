"""Tests for scripts/office_fonts.py (WIN-4)."""

import tempfile
import unittest
import zipfile
from pathlib import Path

import office_fonts

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "apps/frame/crates/pptx/tests/data/fixture.pptx"


def fonts_in(path, part):
    with zipfile.ZipFile(path) as z:
        return z.read(part).decode()


class OfficeFonts(unittest.TestCase):
    def test_the_document_names_the_office_fonts_or_their_stand_ins(self):
        with tempfile.TemporaryDirectory() as tmp:
            office = Path(tmp, "office.docx")
            stand_ins = Path(tmp, "stand-ins.docx")
            office_fonts.docx(office)
            office_fonts.docx(stand_ins, stand_ins=True)
            office_xml = fonts_in(office, "word/document.xml")
            stand_in_xml = fonts_in(stand_ins, "word/document.xml")
            for font, stand_in in office_fonts.STAND_INS.items():
                self.assertIn(f'w:ascii="{font}"', office_xml)
                self.assertNotIn(f'w:ascii="{font}"', stand_in_xml)
                self.assertIn(f'w:ascii="{stand_in}"', stand_in_xml)
            # The same text either way: only the fonts differ.
            self.assertEqual(office_xml.count("<w:p>"), stand_in_xml.count("<w:p>"))

    def test_the_deck_keeps_every_part_with_the_fonts_renamed(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp, "deck.pptx")
            office_fonts.pptx(FIXTURE, out)
            with zipfile.ZipFile(FIXTURE) as a, zipfile.ZipFile(out) as b:
                self.assertEqual(a.namelist(), b.namelist())
                for name in a.namelist():
                    before, after = a.read(name), b.read(name)
                    if not name.endswith(".xml"):
                        self.assertEqual(before, after, name)
                        continue
                    for font, stand_in in office_fonts.STAND_INS.items():
                        self.assertNotIn(f'typeface="{font}"'.encode(), after, name)
                        self.assertEqual(before.count(f'typeface="{font}"'.encode())
                                         + before.count(f'typeface="{stand_in}"'.encode()),
                                         after.count(f'typeface="{stand_in}"'.encode()), name)
            self.assertIn('typeface="Liberation Sans"', fonts_in(out, "ppt/slides/slide1.xml"))


if __name__ == "__main__":
    unittest.main()
