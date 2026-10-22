#!/usr/bin/env bash
# Lands main on origin/main. It runs the checks, then fast-forwards
# origin/main. It never force-pushes, and nothing else pushes main.
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

[ "$(git symbolic-ref --quiet --short HEAD)" = main ] || die "check out main first."
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "commit or stash the changes first."
# WHY: an untracked source file can make the checks pass on code the commits
# don't have. Untracked markdown, like a draft plan, can't.
untracked=$(git ls-files --others --exclude-standard -- . ':(exclude)*.md')
[ -z "$untracked" ] || die "untracked files could change the build:"$'\n'"$untracked"

git fetch --quiet origin
if [ "$push" = 1 ]; then
  git merge-base --is-ancestor origin/main HEAD ||
    die "main has diverged from origin/main. Rebase onto it and land again."
  [ -n "$(git rev-list origin/main..HEAD)" ] || die "origin/main already has every commit."
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

# WHY: origin/main can move while the checks run.
git fetch --quiet origin
git merge-base --is-ancestor origin/main HEAD ||
  die "origin/main moved during the checks. Rebase onto it and land again."
run git push origin HEAD:refs/heads/main
