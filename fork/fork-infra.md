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
| 2 | Done | Give the cycle-changes hook a type alias. | `ui/picker.rs`, with `Patch: changed-files-views`. Workspace clippy passes with `-D warnings`. |
| 3 | Done | Write down the fork's git rules. | `AGENTS.md`: branches, no contact with upstream, commit style, the `Patch:` trailer, opt-in features. `FORK.md`: branches, setup, build, and `--grep` in place of commit lists. |
| 4 | Done | Add commit hooks for formatting, clippy, and the Patch trailer. | `tools/fork/hooks/pre-commit` and `commit-msg`, and their setup line in `FORK.md`. |
| 5 | Done | Add a land script that gates and fast-forwards fork. | `tools/fork/land.sh`, and the push rule in `AGENTS.md`. |
| 6 | Done | Add the upstream conventions guide. | `agents/HELIX.md`: upstream docs and CI checks, MSRV, where to start, comment tags with `FORK:`, change discipline. `document.rs` joins the hot list. |
| 7 | Done | Add the testing and manual-verify guides. | `agents/TESTING.md` and `agents/VERIFY.md`. |
| 8 | Done | Add the review guide. | `agents/REVIEW.md`. |
| 9 | Done | Add the rebase playbook. | `agents/REBASE.md`. |
| 10 | Done | Write down how a plan is split and approved. | `AGENTS.md` §Plans, from koloda's `IMPLEMENTATION-PLAN.md`. |

Next, out of this plan's scope:

- An integration test for `picker-keys`, the first task under `TESTING.md`.
- `// FORK:` markers on the existing seams, which the ledger lists under Gaps.
- The diff review plans, `fork/diff-review.md` and `fork/review-stream.md`.

## Found

- Item 0: the combined move commit split cleanly, because its halves touch disjoint files. The first half builds alone.
  Replayed commits keep their author dates. The tree after the rewrite matched the old one exactly.
- `git log --grep '^Patch: <slug>$'` matches only the trailer line, because `--grep` anchors at line boundaries.
- Item 2's commit carries `Patch: changed-files-views`, so item 3 marked it done here.
- Ledger entries became `###` headings under `## Patches`, so the branch, setup, and build sections sit beside them.
- `pre-commit` runs `cargo fmt -p` on the touched crates instead of `rustfmt` on the staged files.
  `rustfmt` needs an `--edition`, and `helix-lsp-types` is on 2018 while the rest is on 2021.
  It also formats the modules a file declares, so it reached past the staged files anyway.
- `land.sh` refuses untracked files except markdown. An untracked source file could let the checks pass on code the commits lack, and draft plans stay in `fork/` untracked.
- `land.sh` skips upstream's `query-check`, `indent-check`, `highlight-check`, and `theme-check`. They only cover `runtime/`, which no fork feature touches. `HELIX.md` says when to run them.
- `land.sh --check` passed on the stack at item 5: fmt, clippy, docs, 177 integration tests, and a clean docgen.
- A seam marker names its feature, `// FORK: <slug>`, so dropping a feature can find its seams. No seam had a marker yet, so nothing needed rewriting.
- `document.rs` joined the hot list: 16 upstream commits in the year to 2026-10-17, as many as `editor.rs`.
- Item 10 was missing from the first version of this plan. The decisions put koloda's plan rules in `AGENTS.md`, but no item did it.
- The report said `test_key_sequences` waits up to 500 ms. That limit only covers the final quit. Each sequence runs until idle, 250 ms after the last key.
- The harness notes in `TESTING.md` about the idle timer and the working directory came from the dropped diff review's integration tests.
- `REVIEW.md` doesn't make a missing integration test for picker behavior Blocking, as the report proposed.
  The harness can't reach a picker's state or change the working directory, so the rule is the test layer `TESTING.md` names.
- The rebase happens on `fork` itself, not on the report's `rebase/YYYY-MM` branch. The `pre-rebase-YYYY-MM` tag already keeps the old stack.
- `land.sh --check` checks trailers on all of `master..fork` and skips the `origin/fork` ancestry check, so it works on a rebased stack before the force-push.
- `REBASE.md` keeps `master` and the tags local while Actions are on for the GitHub fork. Pushing either would start upstream's workflows there.
- `AGENTS.md` §Plans keeps koloda's approval step, one commit per phase, and risk-first ordering.
  It drops the task file's Status, Guides, and Flow lines, and the `Commit:` line for each item.
