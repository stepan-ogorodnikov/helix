#!/usr/bin/env bash
# Lands fork on origin/fork. It checks the trailers on master..fork, runs
# upstream's CI checks, then fast-forwards origin/fork.
# It never force-pushes. Moving the stack onto a new upstream does that by hand,
# as agents/REBASE.md says.
#
# Usage: tools/fork/land.sh [--check]
#   --check  run the checks and push nothing.
set -euo pipefail

push=1
case "${1:-}" in
  "") ;;
  --check) push=0 ;;
  *) echo "usage: tools/fork/land.sh [--check]" >&2; exit 2 ;;
esac

die() { echo "land: $*" >&2; exit 1; }
run() { echo "land: $*" >&2; "$@"; }

cd "$(git rev-parse --show-toplevel)"

[ "$(git symbolic-ref --quiet --short HEAD)" = fork ] || die "check out fork first."
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "commit or stash the changes first."
# WHY: an untracked source file can make the checks pass on code the commits
# don't have. Untracked markdown, like a draft plan, can't.
untracked=$(git ls-files --others --exclude-standard -- . ':(exclude)*.md')
[ -z "$untracked" ] || die "untracked files could change the build:"$'\n'"$untracked"

git fetch --quiet origin
if [ "$push" = 1 ]; then
  git merge-base --is-ancestor origin/fork HEAD ||
    die "fork has diverged from origin/fork. Rebase onto it, or push a rebased stack as agents/REBASE.md says."
  [ -n "$(git rev-list origin/fork..HEAD)" ] || die "origin/fork already has every commit."
fi

# WHY: the whole stack, not just what origin/fork lacks, so --check also covers
# a stack just moved onto a new upstream.
problems=()
for commit in $(git rev-list --reverse master..HEAD); do
  subject=$(git log -1 --format='%h %s' "$commit")
  slugs=$(git log -1 --format='%(trailers:key=Patch,valueonly)' "$commit" | sed '/^$/d')
  if [ "$(printf '%s' "$slugs" | grep -c .)" -ne 1 ]; then
    problems+=("$subject: needs exactly one Patch trailer")
  elif ! git show HEAD:FORK.md | grep -qxF "### $slugs"; then
    problems+=("$subject: FORK.md has no '### $slugs' entry")
  fi
done
if [ ${#problems[@]} -gt 0 ]; then
  printf 'land: %s\n' "${problems[@]}" >&2
  exit 1
fi

# INVARIANT: the fork's only CI. These are the checks upstream's build.yml ran
# at the fork point, minus the runtime query and theme checks, which only cover
# runtime/.
run cargo fmt --all --check
run cargo clippy --workspace --all-targets -- -D warnings
run env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --document-private-items
run cargo test --workspace
run cargo integration-test
run cargo xtask docgen
git diff --quiet -- book/src/generated ||
  die "cargo xtask docgen changed book/src/generated. Commit the result and land again."

if [ "$push" = 0 ]; then
  echo "land: checks passed. Nothing pushed." >&2
  exit 0
fi

# WHY: origin/fork can move while the checks run.
git fetch --quiet origin
git merge-base --is-ancestor origin/fork HEAD ||
  die "origin/fork moved during the checks. Rebase onto it and land again."
run git push origin HEAD:refs/heads/fork
