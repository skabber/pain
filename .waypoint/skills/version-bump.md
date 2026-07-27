---
name: version-bump
description: >
  Determine the next semantic version, confirm it with the developer, then
  prepare and ship it: analyze what's changed since the last release (or,
  for the very first release, use the fixed starting version), update
  Cargo.toml's workspace version and CHANGELOG.md's heading, commit, push
  to main, and — if the developer confirms cutting a release now, not just
  preparing the bump — tag and push the tag too. Use when the developer
  asks to bump the version, cut a release, or prepare a release — or
  proactively, once a release has ever been cut, when a substantial amount
  of unreleased work has accumulated and it's worth flagging.
---

# Skill: Version Bump

## Purpose

Keep `Cargo.toml`'s `[workspace.package] version` and `CHANGELOG.md`'s
release heading in sync with what has actually shipped, using a
consistent, repeatable semver judgment instead of an ad hoc guess each
time — and, once confirmed, actually ship it: commit, push to main, and
optionally tag and push the tag (which is what triggers
`.github/workflows/release.yml`'s build-and-publish pipeline).

Pushing to main and pushing a release tag are both genuinely consequential
— they touch the shared remote and, for the tag, trigger real CI and a
public release. Per this project's own standing rule on confirmation
(`.waypoint/opord.md` §3c), this skill always confirms the exact version
and commit message with the developer *before* touching anything — not
after. Nothing in Steps 4+ below happens without that confirmation.

## When to Use

- The developer asks to "bump the version," "cut a release," "prepare a
  release," or similar.
- Proactively, at the start of a session: once a release has ever been
  cut (a `v*` git tag exists), check whether `CHANGELOG.md`'s
  `## Unreleased` section has accumulated a substantial number of entries
  (roughly 5+) or it's been a long stretch since the last release. If so,
  mention it to the developer once, briefly, as an aside — not a
  recurring nag every session, and not at all before the very first
  release exists (an empty/thin project has nothing meaningful to prompt
  about yet).

## Process

### Step 1 — Find the baseline

- Check for existing git tags matching `v*` (`git tag -l 'v*'`).
- **No tag exists yet (first release):** there's no prior baseline to
  diff against, and none is needed — the starting version is fixed at
  **v1.0.0** by the developer's own decision. Skip to Step 3.
- **A tag exists:** the highest one by semver (not just most recent by
  date) is the baseline. Continue to Step 2.

### Step 2 — Analyze what changed since the baseline

Gather real evidence — do not guess:

- `git log <baseline-tag>..HEAD --oneline` for the actual commit history.
- `CHANGELOG.md`'s `## Unreleased` section — this project maintains it
  continuously, so it's usually the clearest single source of what
  shipped, already phrased in user-facing terms.

Classify what's there against this project's semver mapping. This is an
*application*, not a published library — "breaking" means breaking for a
*user*, not an API consumer:

| Bump | Triggered by |
|---|---|
| **Major** | Anything that breaks an existing user's setup or expectations: an incompatible config-file format change, a removed feature, a changed default an existing user would need to notice and adapt to, dropped platform support |
| **Minor** | Any new user-facing capability, additive and backward compatible (a new setting, a new menu action, a newly supported shell/platform) |
| **Patch** | Bug fixes and internal changes only — nothing a user needs to do anything differently for |

Take the **highest** tier triggered by anything in the batch — one
breaking change outweighs ten new features.

### Step 3 — Decide the version and draft the commit message

- First release: **1.0.0**, fixed — not a judgment call.
- Otherwise: the baseline version bumped per the highest tier from
  Step 2 (e.g. `1.2.3` plus a new feature, no breaking changes → `1.3.0`).
- Draft the commit message now, per the standard in
  `.waypoint/opord.md` §3d: one line, plain language —
  `Bump version to <version>`. Nothing more.

This step is read-only. Nothing is edited, staged, or committed yet.

### Step 4 — Confirm before doing anything

Present the developer with exactly two things and stop until they answer:

- The decided version: `v<version>`
- The exact commit message that will be used: `Bump version to <version>`

Ask which of these should happen (`AskUserQuestion`, or plainly in chat if
that tool isn't available):

1. **Bump, commit, and push to main only** — prepares the release but
   doesn't cut it yet.
2. **Bump, commit, push to main, then tag and push the tag** — does all
   of the above and also cuts the release now (this is what triggers the
   release workflow).
3. **Stop** — the developer wants to adjust the version or message first.

Do not proceed past this step without an explicit answer. If the
developer picks a different version or message than proposed, use theirs.

### Step 5 — Apply the file edits

Only after Step 4 is confirmed (option 1 or 2):

- Confirm the current branch is `main` (`git branch --show-current`). If
  it isn't, stop and ask — don't assume this is still the right thing to
  do from a feature branch.
- Update `Cargo.toml`'s `[workspace.package] version` to the decided
  version.
- Also update every in-workspace path dependency's own `version = "..."`
  requirement string (currently in `crates/app`, `crates/router`, and
  `crates/session`'s `Cargo.toml`s — check for others, this list can
  grow) to match. Cargo enforces that requirement even for path
  dependencies, so leaving these at the old version breaks the build
  outright (`failed to select a version for the requirement`) — found
  the hard way the first time this skill ran; do not skip it.
- Update `man/pain.1`'s `.TH` line, which carries both the version and the
  date (`.TH PAIN 1 "<yyyy-mm-dd>" "pain <version>" "User Commands"`).
  Nothing builds this file, so a stale version here is invisible until a
  user runs `man pain` and is told the wrong thing.
- Rename `CHANGELOG.md`'s `## Unreleased` heading to `## v<version>`, and
  add a fresh, empty `## Unreleased` heading above it.
- Run `cargo build --workspace` to confirm the bump didn't break
  anything. If it fails, fix it before continuing — never commit a
  version bump that doesn't build.

### Step 6 — Commit and push to main

- Stage exactly the files this step edited — nothing else the developer
  may have pending.
- Commit with the exact message confirmed in Step 4.
- `git push`. If it's rejected because the remote has moved on (someone
  or something else pushed in the meantime), stop and ask — never force
  push.

### Step 7 — Tag and push, only if the developer chose option 2

- `git tag -a v<version> -m "v<version>"`
- `git push origin v<version>`

This is what actually triggers `.github/workflows/release.yml` and makes
the release public. If the developer chose option 1 instead, stop after
Step 6 — the bump is committed and pushed, but no tag exists yet, and
running this skill again later (once they're ready) picks it up as
Step 1's baseline check will simply find no new tag and nothing else has
changed.

## Notes

- The very first invocation (no `v*` tag exists yet) is a fixed decision
  (`v1.0.0`), not a judgment call — don't overthink it or look for
  reasons to pick something else.
- If the evidence genuinely shows nothing breaking or feature-shaped
  since the baseline (a pure bugfix window), a patch bump is the right,
  unglamorous answer — resist rounding up.
- This skill's tier judgment is exactly that — a judgment call, not a
  mechanical calculation. If the developer disagrees with the decided
  tier, defer to them and use the number they want instead.
- The confirmation in Step 4 happens every time this skill runs — a past
  confirmation doesn't carry forward to a future invocation.
