# Feature documentation

As-built records, one file per shipped feature, written when the work closes
(OPORD §3d). These describe what exists and *why it is built that way* —
the constraints, the rejected alternatives, the non-obvious consequences.

They are not:

- a changelog (`CHANGELOG.md` is the user-facing record of what changed),
- a design doc (`.waypoint/design/` holds decisions made *before* building),
- a session narrative (`.waypoint/memory/` holds the blow-by-blow).

The test is whether someone changing this code in six months would make a
worse decision without the document. If the code and its comments already
answer that, no file is owed.

## Coverage

Milestones 0-7 predate this directory and are not retroactively documented.
Their reasoning is recorded in `.waypoint/memory/` and, more durably, in the
doc comments of the code itself, which this project keeps unusually
thorough. Backfilling them from memory would produce documents less
trustworthy than either source.

Files here cover work from 2026-07-28 onward.
