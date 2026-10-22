# Taking changes from upstream

The fork stopped rebasing at upstream `ba40e547`.
Upstream's later work comes over by cherry-pick, a commit at a time, and only when the human asks.

Reviewed through: `ba40e547`, on 2026-10-22.

## Steps

1. Fetch, and read what's new since the commit above.

   ```sh
   git fetch upstream
   git log --oneline <reviewed>..upstream/master
   ```

   List what looks worth taking, and let the human pick: fixes to code the fork uses, languages, grammars, queries, and themes.
   When upstream ships something a fork feature already does, say so. Taking it means dropping or reworking the feature, which gets a plan of its own.

2. Take each picked commit on `main`.

   ```sh
   git cherry-pick -x <commit>
   ```

   `-x` ends the message with `(cherry picked from commit <commit>)`, so `git log --grep 'cherry picked from'` lists what came over.
   A picked commit keeps upstream's message, `area:` prefix and all.
   Upstream squashes most pull requests into one commit. For one merged as a merge commit, `git cherry-pick -x -m 1 <merge>` takes the whole pull request as one commit.

3. Resolve conflicts.
   - In code a fork feature changed, read the feature's `FORK.md` entry first. It says what the feature copies from upstream or relies on.
   - When upstream's change no longer fits, adapt it, and say how in the body, above the cherry-pick line.
   - A change to a file the fork deleted, like those under `.github/`, stays deleted: `git rm` it.
   - `git cherry-pick --abort` gives up on the commit.

4. Check the commits.
   Run the tests from `agents/TESTING.md` (Running) for the crates they touched.
   A change under `runtime/` needs its `cargo xtask` check, as `agents/HELIX.md` (Upstream docs) says.
   A pick that bumps `rust-toolchain.toml` also updates the MSRV line in `agents/HELIX.md`.

5. Move "Reviewed through" above to the last upstream commit you read, taken or not, in a commit of its own.

6. Land with `tools/fork/land.sh` when the human asks.
