# Verifying changes

Tests cover what they can reach.
For the rest, the agent writes a short brief, and the human checks it in `hx`.

## Agents must not

- Run `hx` interactively, or drive it through tmux, a pty, or `script`.
- Claim a picker, a keymap, or anything drawn on screen works because the tests passed.
- Ask the human to "try the picker" without saying what to press and what should happen.

## Agents must

1. Run the tests from `agents/TESTING.md` (Running) for the crates the change touched.
2. Hand back a change to what the user sees or presses with a Manual verify block.
3. For a change the user can't see, like a moved body or a type alias, write `Manual verify: none — <why>` instead.

## Manual verify block

Put it in the message that hands the work back.
Usually 3–7 paths, not a tour of the editor.

````markdown
## Manual verify

- Build: `cargo install --path helix-term --locked`
- Config, if the change needs one, in `~/.config/helix/config.toml`:
  ```toml
  [keys.picker]
  C-n = "move_next"
  ```
- Paths:
  - <starting state> → <keys> → <what you should see>
  - <one cancel or negative path, when the change adds a guard>
- Skip if: <when a path doesn't apply>
````

How to write a path:

- Start from a state the human can reach: a file to open, or a repository with something staged.
  Give the shell commands that set it up, when that isn't obvious.
- Name the keys, like `Space g`, `C-g`, or `:yank-diagnostics<ret>`, not the command's function.
- Name what's on screen: the picker's status label, the message in the status line, the text after `"+p`.
- Prefer a path that would fail if the bug were real.
