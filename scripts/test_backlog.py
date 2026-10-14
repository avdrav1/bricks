"""Tests for backlog.py: each app's milestones unlock in their own chain."""
import tempfile
import unittest
from pathlib import Path

import backlog

HEAD = """# Backlog

## Gates

{gates}

## Stories

| ID | MS | Pri | Status | Depends | Story | Acceptance |
| --- | --- | --- | --- | --- | --- | --- |
{rows}
"""


def row(sid, ms, status, depends="-", pri="P0"):
    return f"| {sid} | {ms} | {pri} | {status} | {depends} | story {sid} | proof |"


class ChainTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = Path(self.dir.name) / "BACKLOG.md"
        self.saved = backlog.BACKLOG
        backlog.BACKLOG = self.path

    def tearDown(self):
        backlog.BACKLOG = self.saved
        self.dir.cleanup()

    def write(self, gates, rows):
        self.path.write_text(HEAD.format(gates="\n".join(gates), rows="\n".join(rows)))

    def test_an_open_bricks_gate_does_not_lock_slate(self):
        self.write(
            ["- [ ] M4: brick gate", "- [ ] SL0: slate viewer"],
            [row("PKG-9", "M4", "done"), row("SLATE-1", "SL0", "todo")],
        )
        self.assertEqual(backlog.next_story("brick")["action"], "gate")
        got = backlog.next_story("slate")
        self.assertEqual((got["action"], got["id"]), ("start", "SLATE-1"))

    def test_a_later_milestone_waits_for_the_earlier_gate_of_its_own_app(self):
        self.write(
            ["- [ ] FR0: frame viewer", "- [ ] FR1: frame editor"],
            [row("FRAME-1", "FR0", "done"), row("FRAME-6", "FR1", "todo")],
        )
        got = backlog.next_story("frame")
        self.assertEqual((got["action"], got["milestone"]), ("gate", "FR0"))
        backlog.check_gate("FR0")
        got = backlog.next_story("frame")
        self.assertEqual((got["action"], got["id"]), ("start", "FRAME-6"))

    def test_resume_stays_within_the_app(self):
        self.write(
            ["- [ ] SL0: slate viewer", "- [ ] FR0: frame viewer"],
            [row("SLATE-1", "SL0", "in-progress"), row("FRAME-1", "FR0", "todo")],
        )
        self.assertEqual(backlog.next_story("slate")["action"], "resume")
        got = backlog.next_story("frame")
        self.assertEqual((got["action"], got["id"]), ("start", "FRAME-1"))

    def test_dependencies_order_stories_in_a_milestone(self):
        self.write(
            ["- [ ] SL0: slate viewer"],
            [row("SLATE-2", "SL0", "todo", "SLATE-1"), row("SLATE-1", "SL0", "todo")],
        )
        self.assertEqual(backlog.next_story("slate")["id"], "SLATE-1")

    def test_etch_gates_unlock_its_chain(self):
        self.write(
            ["- [ ] FR1: frame editor", "- [ ] ET0: etch canvas", "- [ ] ET1: etch paint"],
            [row("FRAME-6", "FR1", "todo"), row("ETCH-1", "ET0", "done"), row("ETCH-6", "ET1", "todo")],
        )
        got = backlog.next_story("etch")
        self.assertEqual((got["action"], got["milestone"]), ("gate", "ET0"))
        backlog.check_gate("ET0")
        got = backlog.next_story("etch")
        self.assertEqual((got["action"], got["id"]), ("start", "ETCH-6"))


if __name__ == "__main__":
    unittest.main()
