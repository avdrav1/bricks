"""Tests for scripts/office_fonts.py (WIN-4)."""

import re
import tempfile
import unittest
import zipfile
from pathlib import Path

import office_fonts


def part(path, name):
    with zipfile.ZipFile(path) as z:
        return z.read(name).decode()


class OfficeFonts(unittest.TestCase):
    def test_the_document_names_the_office_fonts_or_their_stand_ins(self):
        with tempfile.TemporaryDirectory() as tmp:
            office, stand_ins = Path(tmp, "office.docx"), Path(tmp, "stand-ins.docx")
            office_fonts.docx(office)
            office_fonts.docx(stand_ins, stand_ins=True)
            office_xml = part(office, "word/document.xml")
            stand_in_xml = part(stand_ins, "word/document.xml")
            for font, stand_in in office_fonts.STAND_INS.items():
                self.assertIn(f'w:ascii="{font}"', office_xml)
                self.assertNotIn(f'w:ascii="{font}"', stand_in_xml)
                self.assertIn(f'w:ascii="{stand_in}"', stand_in_xml)
            # The same text either way: only the fonts differ.
            text = lambda xml: re.findall(r"<w:t[^>]*>([^<]*)</w:t>", xml)
            self.assertEqual(text(office_xml), text(stand_in_xml))
            self.assertEqual(len(text(office_xml)), 6)

    def test_the_deck_is_the_blank_one_with_its_title_slide_filled(self):
        with tempfile.TemporaryDirectory() as tmp:
            office, stand_ins = Path(tmp, "office.pptx"), Path(tmp, "stand-ins.pptx")
            office_fonts.pptx(office)
            office_fonts.pptx(stand_ins, stand_ins=True)
            with zipfile.ZipFile(office_fonts.BLANK_DECK) as blank, zipfile.ZipFile(office) as deck:
                self.assertEqual(blank.namelist(), deck.namelist())
                for name in blank.namelist():
                    if name != "ppt/slides/slide1.xml":
                        self.assertEqual(blank.read(name), deck.read(name), name)
            office_xml = part(office, "ppt/slides/slide1.xml")
            stand_in_xml = part(stand_ins, "ppt/slides/slide1.xml")
            texts = re.findall(r"<a:t>([^<]*)</a:t>", office_xml)
            self.assertEqual(texts[0], office_fonts.TITLE)
            self.assertEqual(len(texts), 6)
            self.assertEqual(texts, re.findall(r"<a:t>([^<]*)</a:t>", stand_in_xml))
            for font, stand_in in office_fonts.STAND_INS.items():
                self.assertIn(f'typeface="{font}"', office_xml)
                self.assertIn(f'typeface="{stand_in}"', stand_in_xml)
            # Both placeholders sit where the deck says, and nothing shrinks the text.
            self.assertEqual(office_xml.count("<a:noAutofit/>"), 2)
            self.assertNotIn("<p:spPr></p:spPr>", office_xml)


if __name__ == "__main__":
    unittest.main()
