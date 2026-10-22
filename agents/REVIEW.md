# Reviewing diffs

How an agent reviews a diff on `main`, such as `origin/main..main` or one commit.
This isn't an authoring guide. The guides it cites are.

For any review, read `AGENTS.md`, `agents/HELIX.md`, and this file.
Also read `agents/TESTING.md` when the diff has tests or should have them, and the feature's plan in `fork/` when there is one.
A finding can only cite a guide you've read.

## Scope

Comment on changed lines and their neighbors only.
Never audit whole files, and never review code the diff doesn't touch.
If code outside the diff breaks a rule, mention it once, under Out of scope.

## Every finding cites a rule

Acceptable anchors:

- A guide and section: `AGENTS.md` (Commits), `agents/HELIX.md` (Comments), `agents/TESTING.md` (Banned).
- A ledger entry: `FORK.md` (picker-keys).
- A plan item: `fork/<feature>.md`, with the item and what it says.
- A book page the behavior contradicts: `book/src/keymap.md`.
- A clippy lint, or a check `tools/fork/land.sh` runs.

With no anchor, there's no finding.

## Tiers

### Blocking

Must be fixed before the change lands:

- Wrong behavior, a broken invariant, or behavior that contradicts the book or the plan.
- A newly touched upstream file, new config, or changed behavior without an update to the feature's `FORK.md` entry: `AGENTS.md` (Ledger).
- A commit that does more than one thing: `AGENTS.md` (Commits).
- Changed behavior without a test where `agents/TESTING.md` (Layers) puts it.
- A user-visible change without its book page, or with stale `book/src/generated/`: `agents/HELIX.md` (Upstream docs).
- A non-obvious line without a tagged comment, or a comment that narrates: `agents/HELIX.md` (Comments).
- A std API newer than the MSRV: `agents/HELIX.md` (Upstream docs).
- A user-visible change handed back without a Manual verify block: `agents/VERIFY.md`.

### Nit

Optional taste. The author can take it or leave it.
Never raise an ignored nit again as Blocking.
Keep nits fewer than the diff's size warrants.

### Out of scope

A heads-up about code outside the diff, listed at the end, not as an action item.

## Noise: don't raise

These look like good practice but don't belong in a review.

- Cleanup, renames, reformatting, or modernizing of code the diff doesn't need: `agents/HELIX.md` (Change discipline).
- A bug the diff didn't cause. Out of scope at most.
- Folding a soft-fork wrapper or copy back into the code it came from, when the diff has no other reason to: `AGENTS.md` (Fork).
- Shims or compatibility wrappers: `AGENTS.md` (Fork).
- Parameters or branches for cases nobody asked for: `agents/HELIX.md` (Change discipline).
- Doc comments on ordinary functions.
- Running the change in `hx`: `agents/VERIFY.md`.

## Writing a finding

One finding per comment.
Start with the tier, `[Blocking]`, `[Nit]`, or `[Out of scope]`, then the location, then the anchor.
Then the problem and the fix, in two sentences at most.

```
[Blocking] helix-term/src/ui/picker.rs:1206 — agents/HELIX.md (Comments).
The early return looks redundant next to the check in `replace_items`, but it keeps the status label. Add a `// WHY:` line above it.
```

Don't restate the diff, explain the code, or guess at intent.

## Checklist

- [ ] Every finding has a tier and an anchor.
- [ ] Nothing on code outside the diff.
- [ ] Nothing from the noise list.
- [ ] Every Blocking finding can be fixed in this diff.
