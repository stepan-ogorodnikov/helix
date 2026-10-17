# Helix fork: agent docs and fork tooling

Source: `FORK-DOCS-REPORT.md`, written against koloda `main` at `20db0e4` and fork `fork` at `1d1a31e0`.
Reconciled 2026-10-17 against fork `fork` at `da828299`, upstream `master` at `ba40e547`, and koloda at `447056b5`.

## Bottom line

- Adopt koloda's process: plans with approval, scoped tests and a manual-verify brief, review tiers, comment tags, a commit trailer, one landing path.
- Don't adopt its surface: no task files, no index, no specs or decision records, no crate READMEs.
- Add what a soft fork needs and koloda doesn't: the patch ledger, a `Patch:` trailer that groups commits by feature, and a rebase playbook.
- Everything lives in paths upstream has never had: `AGENTS.md`, `FORK.md`, `fork/`, `agents/`, `tools/fork/`.

## Since the report

- `AGENTS.md` has the hot-file hook rule, the ledger rule, and the plans rule.
- `FORK.md` has the ledger, with entries for `picker-keys`, `changed-files-views`, `yank-diagnostics`, and `fork-infra`.
- The report's side finding is done: the changed-file picker's logic moved to `helix-term/src/commands/changed_file.rs`.
- Feature plans live in `fork/<feature>.md` and their commits go on `fork`.
  That replaces the report's task files on `task/<slug>` branches.
- The diff review feature was dropped on 2026-10-17. Its plan stays out of the tree until it's reworked.
- koloda since `20db0e4`: agents route their own guides, task branches stay local until they land, and a `commit-msg` hook checks the trailer.
- `cargo clippy --workspace --all-targets -- -D warnings` fails on the fork's `on_cycle_changes` field in `ui/picker.rs` (`type_complexity`).
  No gate can pass until that's fixed.
- Actions are enabled on the GitHub fork, but it lists no workflows and has never run one.
  Upstream's `build`, `cachix`, and `gh-pages` run on pushes to `master`, and `gh-pages` on any tag.

## Decisions (2026-10-17)

- Plans stay in `fork/<feature>.md`, and commits go on `fork`. No task files or task branches.
- `tools/fork/land.sh` gates the stack and fast-forwards `origin/fork`. Only an upstream rebase pushes any other way.
- Every commit carries one `Patch: <slug>` trailer that names its `FORK.md` entry.
  The ledger drops its commit lists, which went stale at every rebase.
  The existing commits get the trailer now, which costs one force-push of `origin/fork`.
- Guides: `agents/HELIX.md`, `TESTING.md`, `VERIFY.md`, `REVIEW.md`, and `REBASE.md`.
  `AGENTS.md` routes to them, so there's no `INDEX.md`.
  The rules from koloda's `IMPLEMENTATION-PLAN.md` go in `AGENTS.md` §Plans.
- No fork CI for now. The land gate runs upstream's CI checks locally.

Not adopted: `CLAUDE.md`, `agents/INDEX.md`, `TASKS.md`, `IMPLEMENTATION-PLAN.md`, `MARKDOWN.md`, `AUDIT.md`, `DECISIONS.md`, `docs/decisions/`, specs, `ADD-*` playbooks, crate READMEs, a branch ruleset, and `.github/workflows/fork.yml`.

## Plan

Each item is one commit with `Patch: fork-infra`, unless it says otherwise.
Each commit updates the `fork-infra` ledger entry and marks its item done here.

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 0 | Done | None, a history rewrite | `Patch:` trailers on every commit. Split "Move fork command logic out of the hot upstream files." into one commit per feature. Reword the two `picker:` subjects. Force-push the rewritten copies of the three pushed commits. |
| 1 | Done | Add the fork infrastructure plan. | This file. |
| 2 | | Give the cycle-changes hook a type alias. | `ui/picker.rs`, with `Patch: changed-files-views`. Workspace clippy passes with `-D warnings`. |
| 3 | | Write down the fork's git rules. | `AGENTS.md`: branches, no contact with upstream, commit style, the `Patch:` trailer, opt-in features. `FORK.md`: branches, setup, build, and `--grep` in place of commit lists. |
| 4 | | Add commit hooks for formatting, clippy, and the Patch trailer. | `tools/fork/hooks/pre-commit` and `commit-msg`, and their setup line in `FORK.md`. |
| 5 | | Add a land script that gates and fast-forwards fork. | `tools/fork/land.sh`, and the push rule in `AGENTS.md`. |
| 6 | | Add the upstream conventions guide. | `agents/HELIX.md`: upstream docs and CI checks, MSRV, where to start, comment tags with `FORK:`, change discipline. `document.rs` joins the hot list. |
| 7 | | Add the testing and manual-verify guides. | `agents/TESTING.md` and `agents/VERIFY.md`. |
| 8 | | Add the review guide. | `agents/REVIEW.md`. |
| 9 | | Add the rebase playbook. | `agents/REBASE.md`. |

Next, out of this plan's scope:

- An integration test for `picker-keys`, the first task under `TESTING.md`.
- `// FORK:` markers on the existing seams, which the ledger lists under Gaps.
- The diff review plans, `fork/diff-review.md` and `fork/review-stream.md`.

## Found

- Item 0: the combined move commit split cleanly, because its halves touch disjoint files. The first half builds alone.
  Replayed commits keep their author dates. The tree after the rewrite matched the old one exactly.
- `git log --grep '^Patch: <slug>$'` matches only the trailer line, because `--grep` anchors at line boundaries.
