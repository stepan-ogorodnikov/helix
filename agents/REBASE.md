# Moving the fork onto a new upstream

Do this about once a month, or when an upstream change the fork needs lands, and only when the human asks.
The stack gets rewritten, so this is the one time `origin/fork` is force-pushed.

`FORK.md` (Setup) turns on `rerere`, so a conflict resolved once resolves itself the next time.

## Steps

1. Fetch, and read what's new.

   ```sh
   git fetch upstream
   git log --oneline master..upstream/master
   ```

   For each ledger entry, read the new commits that touch its upstream files: `git log master..upstream/master -- <files>`.

2. Tag the old stack, so it can be compared and restored.

   ```sh
   git tag pre-rebase-YYYY-MM fork
   ```

   `FORK.md`'s "Upstream base" line names the commit it sits on. The review in step 7 needs it.

3. Mirror `master`.

   ```sh
   git checkout master
   git merge --ff-only upstream/master
   ```

   Keep `master` and the tags local while Actions are on for the GitHub fork.
   Upstream's `build`, `cachix`, and `gh-pages` workflows run on a push to `master`, and `gh-pages` on any tag.

4. Rebase.

   ```sh
   git checkout fork
   git rebase master
   ```

   A conflict in a hot file should only touch a hook. Take upstream's side and put the hook back.
   If fork logic conflicts, the feature has logic in an upstream file. Note it under the entry's Gaps.
   `rerere` replays known resolutions, but check each one with `git diff` before continuing.
   To start over, run `git rebase --abort`, or `git reset --hard pre-rebase-YYYY-MM` after the rebase.

5. Drop what upstream now does.
   Check each ledger entry's "Drop when" against the new upstream, including the refs it names.
   When it holds:
   - List the feature's commits with `git log master..fork --grep '^Patch: <slug>$' --oneline`.
   - Remove them with `git rebase -i master`, marking each one `drop`.
   - Confirm `rg 'FORK: <slug>'` finds nothing.
   - Remove the entry from `FORK.md` in a `Patch: fork-infra` commit.

6. Update the ledger and the hot list.
   - Set "Upstream base" in `FORK.md` to the new `master`.
   - Fix any entry whose upstream files or seams moved.
   - Recount the hot files, and update `AGENTS.md` if the list changed:

     ```sh
     git log --since=1.year --format= --name-only master -- '*.rs' | sort | uniq -c | sort -rn | head
     ```

   Commit with `Patch: fork-infra`.

7. Review the rewrite.

   ```sh
   git range-diff <old base>..pre-rebase-YYYY-MM master..fork
   ```

   Every old commit should pair with a new one, apart from those dropped in step 5.
   Read every pair whose diff changed beyond its context lines.

8. Run the gate: `tools/fork/land.sh --check`.
   If upstream bumped `rust-toolchain.toml`, the first build installs the new toolchain.

9. Push with a lease, so a push from somewhere else isn't lost.

   ```sh
   git push --force-with-lease origin fork
   ```
