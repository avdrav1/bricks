---
name: next-story
description: Pick the next story from BACKLOG.md and build it end to end (branch, failing test, implementation, verification, backlog update, local commit) for any Cronkite app: Brick (spreadsheet), Slate (word processor), Frame (presentations), or Etch (image editor). Use whenever the user says "next story", "build the next thing", "what's next", "keep going", "work the backlog", names a story ID like ENG-3, SLATE-12, FRAME-6, or ETCH-2, or asks to resume unfinished work on any of the apps, even if they don't mention the backlog.
---

# next-story

Build exactly one backlog story per run, prove its acceptance criterion, and leave the repo clean. `BACKLOG.md` is the queue for every app; `scripts/backlog.py` reads and writes it so status changes stay consistent. Each app has its own milestone chain, so one app's open gate never blocks another's stories.

| App | `backlog.py next` | Stories | Milestones | Spec and decisions | GUI check |
| --- | --- | --- | --- | --- | --- |
| Brick | `brick` | ENG, GRID, FML, XLSX, STYLE, PKG, … | S0, M0–M5 | `apps/brick/docs/SPEC.md`, `apps/brick/docs/decisions/`, `apps/brick/docs/BENCHMARKS.md` | `scripts/check_*.sh` named in the story |
| Slate | `slate` | SLATE-* | SL0–SL4 | `docs/specs/slate.md`, ADRs 0003, 0005, 0007–0009 | `scripts/check_slate.sh` |
| Frame | `frame` | FRAME-* | FR0, FR1 | `docs/specs/frame.md`, ADRs 0004, 0006 | `scripts/check_frame.sh` |
| Etch | `etch` | ETCH-* | ET0–ET3 | `docs/specs/etch.md`, ADR 0010 | `scripts/check_etch.sh` |

## 1. Pick the story

```sh
python3 scripts/backlog.py next [brick|slate|frame|etch]
```

Pass the app the user is working on; without one, it walks the apps in the table's order and returns the first with work. Act on the `action` field:

- `start`: build this story.
- `resume`: a story is `in-progress` or `review`. Check `git status` and the story branch, then finish it before anything else.
- `gate`: every story in a milestone is done but its gate is unchecked. Verify the gate (step 7) and stop.
- `stuck`: the milestone's remaining stories wait on unfinished dependencies. Report the `blocked` list and stop.
- `complete`: say so and stop.

If the user named a story ID, use `python3 scripts/backlog.py show <ID>` instead. If its dependencies are not `done` or its milestone is locked, tell the user what blocks it and ask whether to proceed anyway.

## 2. Load context

Read, in this order: `CLAUDE.md` (invariants, suite rules, and the crates table), the app's spec and the ADRs from the table that touch this area, `apps/brick/docs/BENCHMARKS.md` if a Brick criterion has a number, the story's neighbours and recent Notes in `BACKLOG.md` (they often carry decisions the next story builds on), then the code in the owning crate.

Stop and ask the user if:

- the acceptance criterion is ambiguous enough that two reasonable implementations would both pass it,
- the story needs a decision that has no accepted ADR,
- building it would break an invariant in `CLAUDE.md` or pull in a non-goal from the app's spec.

## 3. Start

Branch from `main` (it may be checked out in another worktree: `git worktree list`; then `git checkout -b <branch> main` still works). When the user asked for several stories in a row and earlier ones aren't merged yet, branch from the previous story's branch and say the branches are stacked.

```sh
git checkout -b story/<id-lowercase>-<short-slug> main
python3 scripts/backlog.py set <ID> in-progress
```

State a plan in five lines or fewer: owning crate, the test that proves the criterion, the approach, and any risk.

## 4. Build

**Decision stories (`DEC-*`)** are spikes, not features:

1. Build each option's spike in `spikes/<id>/<option>/`. Keep them throwaway and small.
2. Measure exactly what the criterion names. Record hardware, corpus file, and numbers.
3. Write the decision with status `proposed`: Brick's in `apps/brick/docs/decisions/NNNN-<slug>.md` from `0000-template.md`, suite-wide or another app's in `docs/adr/NNNN-<slug>.md` in the shape of the existing ADRs.
4. Set the story to `review`, summarize the recommendation, and stop. A human accepts the ADR. Only then mark it `done` and apply the choice.

**Feature stories:**

1. Write the test first. Turn the acceptance criterion into a failing test in the owning crate.
   - Behavior criteria become unit or integration tests in the toolkit-free crate that owns the behaviour (`image-model`, `document-edit`, `deck-edit`, `data-model`, …).
   - Criteria about what the window shows or how it reacts become steps in the app's GUI check script (table above): it runs the release binary in an Arch container under headless sway and reads the app through the accessibility tree and screenshots. Add the step and its comment in the script's header, beside the earlier stories'.
   - Performance criteria become a benchmark (Brick's `cargo bench`), an app's `--bench-*` mode, or an `#[ignore]` test asserting the target, run with `cargo test --release -- --ignored`.
   - If `corpus/` is missing and Brick's test needs it, run `python3 scripts/gen_corpus.py`.
2. Implement in the owning crate. Keep crate boundaries from `CLAUDE.md`; add a dependency only when the story needs it, and name it in the summary.
3. Keep the diff to this story. Note unrelated problems in the BACKLOG.md Notes section instead of fixing them.
4. Update what the story changed: the app's spec (what's built), `CLAUDE.md`'s crates table when a crate's responsibilities grow, and the Commands block when a script or mode is added.

## 5. Verify

All must pass before the story is done:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --release --workspace -- --ignored   # when the story has perf tests
cargo build --release -p <app> && scripts/check_<app>.sh   # when the story changes the window (needs docker)
```

For perf criteria, report the measured number next to the target. If the machine is not the reference box in `apps/brick/docs/BENCHMARKS.md`, say so; the number still counts as evidence but the gate check waits for the reference box. Update the "Latest" column in `apps/brick/docs/BENCHMARKS.md` when a Brick benchmark ran.

If verification fails and the fix is not clear after two attempts, set the story to `blocked`, write what failed in the Notes section, and stop.

## 6. Finish

```sh
python3 scripts/backlog.py set <ID> done
git add -A
git commit -m "<ID>: <imperative summary>"
```

Add a Notes entry in BACKLOG.md (newest first) for decisions made mid-story, what the code now offers the next story, or follow-ups worth tracking. Do not push or merge; ask the user.

Report in this shape:

```
<ID> done: <one-line summary>
Proof: <test name, check step, or benchmark> -> <measured result> (target <target>)
Changed: <crates/files>
Follow-ups: <none | items added to Notes>
Next: <output of backlog.py next <app>, one line>
```

## 7. Gates

When `next` returns `gate`, check each claim in that gate's line in BACKLOG.md against tests, benchmarks, GUI checks, and the app. Report pass or fail per claim with evidence. Never check the gate box yourself; if the user approves, run `python3 scripts/backlog.py gate <MS>`. A passed gate is usually the moment to offer a release (`ship-it`).

## Keep going

If the user asked for several stories ("do the next three", "keep going"), repeat from step 1 after each commit. Stop early at any `gate`, `stuck`, `blocked`, or `review` result, or when a question needs the user.
