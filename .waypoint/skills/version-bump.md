---
name: version-bump
description: >
  Determine the next semantic version and prepare a release: analyze what's
  changed since the last release (or, for the very first release, decide
  the fixed starting version) and update Cargo.toml's workspace version and
  CHANGELOG.md's heading accordingly. Use when the developer asks to bump
  the version, cut a release, or prepare a release — or proactively, once
  a release has ever been cut, when a substantial amount of unreleased work
  has accumulated and it's worth flagging.
---

# Skill: Version Bump

## Purpose

Keep `Cargo.toml`'s `[workspace.package] version` and `CHANGELOG.md`'s
release heading in sync with what has actually shipped, using a
consistent, repeatable semver judgment instead of an ad hoc guess each
time. This skill only edits those two files — it never commits, tags, or
pushes. Creating the git tag is what actually triggers
`.github/workflows/release.yml`'s build-and-publish pipeline, and that's
the developer's own deliberate action to take, not a side effect of
running this skill.

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

### Step 3 — Decide and apply

- First release: **1.0.0**, fixed — not a judgment call.
- Otherwise: the baseline version bumped per the highest tier from
  Step 2 (e.g. `1.2.3` plus a new feature, no breaking changes → `1.3.0`).
- Update `Cargo.toml`'s `[workspace.package] version` to the decided
  version.
- Rename `CHANGELOG.md`'s `## Unreleased` heading to `## v<version>`, and
  add a fresh, empty `## Unreleased` heading above it — this is exactly
  what `.github/workflows/release.yml`'s release job reads from once a
  tag is actually pushed later.

### Step 4 — Report, don't commit

Tell the developer plainly: the decided version, the single strongest
reason driving the bump tier (not an exhaustive changelog re-summary —
they already wrote the changelog), and that the two files are edited but
unstaged/uncommitted, ready for their own review. Do not `git commit`,
`git tag`, or `git push`.

## Notes

- The very first invocation (no `v*` tag exists yet) is a fixed decision
  (`v1.0.0`), not a judgment call — don't overthink it or look for
  reasons to pick something else.
- If the evidence genuinely shows nothing breaking or feature-shaped
  since the baseline (a pure bugfix window), a patch bump is the right,
  unglamorous answer — resist rounding up.
- This skill's tier judgment is exactly that — a judgment call, not a
  mechanical calculation. If the developer disagrees with the decided
  tier, defer to them and apply the number they want instead.
