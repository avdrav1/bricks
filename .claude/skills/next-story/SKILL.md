---
name: next-story
description: Pick the next story from BACKLOG.md and build it end to end (branch, failing test, implementation, verification, backlog update, local commit). Use whenever the user says "next story", "build the next thing", "what's next", "keep going", "work the backlog", names a story ID like ENG-3 or SORT-1, or asks to resume unfinished work on the spreadsheet app, even if they don't mention the backlog.
---

# next-story

Build exactly one backlog story per run, prove its acceptance criterion, and leave the repo clean. `BACKLOG.md` is the queue; `scripts/backlog.py` reads and writes it so status changes stay consistent.

## 1. Pick the story

```sh
python3 scripts/backlog.py next
```

Act on the `action` field:

- `start`: build this story.
- `resume`: a story is `in-progress` or `review`. Check `git status` and the story branch, then finish it before anything else.
- `gate`: every story in a milestone is done but its gate is unchecked. Verify the gate (step 7) and stop.
- `stuck`: the milestone's remaining stories wait on unfinished dependencies. Report the `blocked` list and stop.
- `complete`: say so and stop.

If the user named a story ID, use `python3 scripts/backlog.py show <ID>` instead. If its dependencies are not `done` or its milestone is locked, tell the user what blocks it and ask whether to proceed anyway.

## 2. Load context

Read, in this order: `CLAUDE.md` (invariants), the ADRs in `docs/decisions/` that touch this area, `docs/BENCHMARKS.md` if the criterion has a number, then the code in the owning crate. Read `docs/SPEC.md` only when the story's intent is unclear.

Stop and ask the user if:

- the acceptance criterion is ambiguous enough that two reasonable implementations would both pass it,
- the story needs a decision that has no accepted ADR,
- building it would break an invariant in `CLAUDE.md` or pull in a V0.1 non-goal.

## 3. Start

```sh
git checkout -b story/<id-lowercase>-<short-slug>
python3 scripts/backlog.py set <ID> in-progress
```

State a plan in five lines or fewer: owning crate, the test that proves the criterion, the approach, and any risk.

## 4. Build

**Decision stories (`DEC-*`)** are spikes, not features:

1. Build each option's spike in `spikes/<id>/<option>/`. Keep them throwaway and small.
2. Measure exactly what the criterion names. Record hardware, corpus file, and numbers.
3. Write `docs/decisions/NNNN-<slug>.md` from `0000-template.md` with status `proposed`.
4. Set the story to `review`, summarize the recommendation, and stop. A human accepts the ADR. Only then mark it `done` and apply the choice (for DEC-1, add the toolkit to `app/Cargo.toml` and its dev packages to CI).

**Feature stories:**

1. Write the test first. Turn the acceptance criterion into a failing test in the owning crate.
   - Behavior criteria become unit or integration tests.
   - Performance criteria become a benchmark (once BENCH-1 exists) or an `#[ignore]` test that reads from `corpus/` and asserts the target, run with `cargo test -- --ignored`.
   - If `corpus/` is missing, run `python3 scripts/gen_corpus.py`.
2. Implement in the owning crate. Keep crate boundaries from `CLAUDE.md`; add a dependency only when the story needs it, and name it in the summary.
3. Keep the diff to this story. Note unrelated problems in the BACKLOG.md Notes section instead of fixing them.

## 5. Verify

All must pass before the story is done:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace -- --ignored   # when the story has perf tests
```

For perf criteria, report the measured number next to the target. If the machine is not the reference box in `docs/BENCHMARKS.md`, say so; the number still counts as evidence but the gate check waits for the reference box. Update the "Latest" column in `docs/BENCHMARKS.md` when a benchmark ran.

If verification fails and the fix is not clear after two attempts, set the story to `blocked`, write what failed in the Notes section, and stop.

## 6. Finish

```sh
python3 scripts/backlog.py set <ID> done
git add -A
git commit -m "<ID>: <imperative summary>"
```

Add a Notes entry in BACKLOG.md only for decisions made mid-story or follow-ups worth tracking. Do not push or merge; ask the user.

Report in this shape:

```
<ID> done: <one-line summary>
Proof: <test name or benchmark> -> <measured result> (target <target>)
Changed: <crates/files>
Follow-ups: <none | items added to Notes>
Next: <output of backlog.py next, one line>
```

## 7. Gates

When `next` returns `gate`, check each claim in that gate's line in BACKLOG.md against tests, benchmarks, and the app. Report pass or fail per claim with evidence. Never check the gate box yourself; if the user approves, run `python3 scripts/backlog.py gate <MS>`.

## Keep going

If the user asked for several stories ("do the next three", "keep going"), repeat from step 1 after each commit. Stop early at any `gate`, `stuck`, `blocked`, or `review` result, or when a question needs the user.
