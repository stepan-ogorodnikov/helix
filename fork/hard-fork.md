# Helix fork: a hard fork

Snapshot: `fork` at `00978fad`, on upstream `master` at `ba40e547` (2026-09-29).
Upstream had nothing newer on 2026-10-22, so the fork starts current and needs no last rebase.

Status: items 0–6 are committed. Item 2's steps on GitHub wait for the human's go.

## Bottom line

- Stop rebasing onto upstream. `fork` becomes `main`, the default branch here and on GitHub, and `master` goes away.
  Upstream's fixes come over by cherry-pick, when they're worth it.
- The rules that kept features droppable and rebases cheap go: opt-in features, the hot-file list and hook shapes, hands off upstream code, the `Patch:` trailer, and `FORK:` markers.
- The process stays: plans with approval, one phase per commit, tests and the manual-verify brief, review tiers, the `WHY:`, `INVARIANT:`, and `WORKAROUND:` tags, and `land.sh` as the gate and the only push.
- The name, and everything that carries it, stays Helix's for now. A later plan renames.

## What goes, what stays

| Rule | Where | After |
| --- | --- | --- |
| Soft fork, hot files | `AGENTS.md` (Fork) | Gone. Code goes where it reads best, and a large feature still gets its own module. |
| Opt-in or additive features | `AGENTS.md` (Fork), `REVIEW.md` (Blocking) | Gone. A config key is for a choice the human wants to keep, not for keeping upstream's behavior. |
| Hook shapes: wrappers, `typed/<name>.rs` children, re-exports | `AGENTS.md` (Hooks) | Gone. The existing ones stay until a change has its own reason to fold them back. |
| `// FORK: <slug>` markers | `AGENTS.md` (Hooks), `HELIX.md` (Comments) | Gone, and removed from the code in phase 6. |
| Hands off upstream code | `HELIX.md` (Change discipline), `REVIEW.md` (Noise) | Becomes: change any code the work needs, and leave unrelated code alone. The smallest diff still wins. |
| `Patch:` trailer, one feature per commit | `AGENTS.md` (Commits), `commit-msg`, `land.sh` | The trailer goes. A commit still does one thing. |
| Patch ledger | `FORK.md`, `AGENTS.md` (Ledger) | Becomes a feature list, phase 5. |
| `master` mirror, the rebase playbook, `rerere` | `AGENTS.md` (Git), `REBASE.md`, `FORK.md` | Gone. `agents/UPSTREAM.md` says how to cherry-pick. |
| Upstream's CI, release, and funding files | `.github/` | Gone, phase 1. |
| `fork_` test files | `TESTING.md`, `helix-term/tests/test/` | Gone, renamed in phase 6. |

Unchanged: `agents/VERIFY.md` apart from wording, the MSRV and docgen rules, `pre-commit`, the checks `land.sh` runs, never touching `helix-editor/helix`, and `gh repo set-default`.

## Branches

After phase 2:

- `main` is the fork's line, the default branch on GitHub, and the only branch `land.sh` pushes. It's never force-pushed.
- `master` is deleted here and on `origin`. `origin/fork` is deleted.
- The `upstream` remote stays for cherry-picks, with its push URL disabled.
- `git merge-base main upstream/master` stays `ba40e547`, since cherry-picks don't change ancestry. It always names the fork point, so it needs no tag.
- The local `spike/review-completion` branch and `archive/diff-review` tag stay local.

## Taking from upstream

What `agents/UPSTREAM.md` says, phase 4:

- Only when the human asks: `git fetch upstream`, then read `git log <reviewed>..upstream/master`.
  The guide keeps a "Reviewed through" line with the last upstream commit read.
- Take a commit with `git cherry-pick -x`, one upstream commit per fork commit, so the `(cherry picked from commit …)` line names its source.
- Grammars, queries, themes, and `languages.toml` rarely conflict.
  Run the `cargo xtask` check for what a pick touches under `runtime/`.
- A conflict in code the fork changed: read the feature's entry in `FORK.md` for what it copies or relies on.
- A pick that bumps `rust-toolchain.toml` updates the MSRV line in `HELIX.md`.

## Plan

Each item is one commit. Items 0–2 carry `Patch: fork-infra`, since the trailer goes in item 2. Each commit marks its item done here.

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 0 | Done | Add the hard fork plan. | This file, and the plan line in `FORK.md`'s `fork-infra` entry. |
| 1 | Done | Remove upstream's CI, release, and funding files. | `.github/workflows/`, `.github/actions/`, `dependabot.yml`, `FUNDING.yml`. `HELIX.md` and the comment in `land.sh` stop pointing at `build.yml`. It goes before `main` reaches GitHub, so upstream's nightly `schedule` never runs on the new default branch. |
| 2 | Done | Rename fork to main, and land without trailers. | `land.sh`: branch `main`, `origin/main`, no trailer loop. Delete `commit-msg`. `AGENTS.md` (Git, Commits), `FORK.md` (Branches, Setup). Then, at the human's ask: `git branch -m fork main`, `land.sh --check`, `git push -u origin main` once by hand, since `land.sh` needs `origin/main` to exist, `gh repo edit --default-branch main`, `git push origin --delete fork master`, `git branch -D master`, `git remote set-head origin -a`, `git remote set-url --push upstream no_push`. |
| 3 | Done | Rewrite the rules for a hard fork. | `AGENTS.md` (Fork and Hooks become a short Code section), `HELIX.md` (Comments, Change discipline), `TESTING.md` (test homes, upstream tests are ours), `REVIEW.md` (Blocking, Noise), `VERIFY.md` wording. `AGENTS.md` (Plans): done plans are history, and their rules may be stale. The `align-message` ruling loses "though fork features are otherwise opt-in". |
| 4 | Done | Replace the rebase playbook with a cherry-pick guide. | `agents/UPSTREAM.md`, delete `agents/REBASE.md`, the guide list in `AGENTS.md`, and the `rerere` line in `FORK.md` (Setup). |
| 5 | Done | Turn the patch ledger into a feature list. | `FORK.md`: each entry keeps what it adds, where it lives, what upstream code it copies or relies on, and its rulings. "Status", "Upstream files", "Upstream refs", "Drop when", and the Gaps about markers go. `AGENTS.md` (Ledger) and `REVIEW.md`'s anchors follow. |
| 6 | Done | Remove the FORK markers and the fork_ test prefix. | 57 marker lines in 25 files. Bare marker lines go, and `//! FORK: <slug>.` doc lines keep their text. `fork_review.rs`, `fork_review/`, and `fork_statusline.rs` lose the prefix, with their `mod` lines. |

Item 2 is the one with outward steps, so it goes as early as item 1 allows.
Item 6 is the only one that touches Rust, and it's mechanical.

## Decisions (2026-10-22)

The human approved the plan with the answer it assumed to each question.

1. `FORK.md`: a feature list (assumed), or delete it and rely on the book and the plans?
   It's the only place that holds couplings like "`review_write.rs` copies `write_all_impl`'s save chain", which a cherry-pick needs.
2. The `Patch:` trailer: drop it (assumed), or keep it to group commits by feature?
3. CI: none, with `land.sh` as the gate (assumed, as `fork-infra` decided), or keep `build.yml` on `main`?
4. GitHub: stay a GitHub fork of `helix-editor/helix` until the rename (assumed), or detach now?
   While it's a fork, `gh` defaults to the parent, so the `gh repo set-default` rule stays.
5. Paths named for the fork, `FORK.md`, `fork/`, and `tools/fork/`: keep them until the rename (assumed), or rename them now?

## Not in this plan

- The name, and what carries it: the repo, the `hx` binary, the config, cache, and data dirs in `helix-loader`, `HELIX_RUNTIME`, `contrib/`'s desktop files, the README, and the book.
  The `helix-*` crate names stay even then.
- Folding the existing seams back into the code they hook.
  `review_write.rs`'s copy of the save chain and the review's lends are the likeliest candidates, when a change has its own reason to touch them.
- Rewriting `fork/fork-infra.md` and `fork/review-stream.md`. They're done, and stay as history.

## Found

- Item 1: `docs/CONTRIBUTING.md` named `build.yml`'s `env.MSRV` as one of three places to bump the MSRV. The bullet went with the file.
  `.github/ISSUE_TEMPLATE/` stays for the rename plan. Issues are off on the GitHub repo, so nothing reads it.
  `REBASE.md` still warns that pushing `master` starts upstream's workflows. That holds until item 2 deletes `master`, since `master` still has them.
- Item 2: `land.sh --check` needs no `origin/main`, so it runs before the first push. `land.sh` itself refuses to push until `origin/main` exists.
  The commit that deletes `commit-msg` doesn't run it, since git runs hooks from the working tree.
  `REVIEW.md` changed here too: its scope line named `fork`, and a Blocking item named the trailer.
  Until items 3 and 4, `AGENTS.md`'s intro and Hooks still describe a soft fork, and `REBASE.md` and the `rerere` line still describe rebasing.
- Item 3: `AGENTS.md` names the soft fork's leftover shapes, the wrappers and the copies, so a change knows it may fold one back and a review doesn't ask for it.
  `HELIX.md`'s `WORKAROUND:` tag now covers dependencies only. A quirk in upstream code gets fixed instead.
  `TESTING.md`'s sweep of banned tests now reaches upstream's test files too, when a change already edits one.
  `REVIEW.md`'s ledger item still names "a newly touched upstream file". Item 5 rewrites it with the ledger.
- Item 4: upstream squashes most pull requests into one commit, 26 of its last 30, so `UPSTREAM.md` picks a commit at a time and covers the odd merge commit with `-m 1`.
  A picked commit keeps upstream's message. The fork's commit style is for its own commits.
  `rerere` left the setup. It only paid off when the same conflicts came back at every rebase.
- Item 5: "Upstream files" split three ways. Where a feature plugs in went under Code, couplings under "Relies on", and seam-only lines went: `mod` lines, command rows, `slotmap`, and the `mod fork_*;` lines.
  `changed-files-views` keeps upstream's #9951 and #16252 under "Relies on", since taking either means redoing the staged walk. The other upstream refs went.
  `picker-keys`' ruling that the key table lives outside `editor.rs` went. It only followed from the hot-file rule.
  The `Editor::switch` condition's ruling became a line under Code. It called the condition the one seam in a hot-file function, and only why it exists still holds.
  `AGENTS.md`'s Ledger section is now Features, and says a fix or a cleanup needs no entry. Without the trailer, no commit needs one.
- Item 6: the 57 markers were 44 comment lines, 7 trailing comments on `mod` lines, and 6 `//!` prefixes. Three carried a note instead of a slug, like `// FORK: picker key table.`, and went too.
  The renamed test modules collide with nothing, and rustfmt sorted their `mod` lines. `cargo integration-test -- review statusline` ran 47 tests, with the rust-analyzer one ignored as before.
