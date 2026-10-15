//! FORK: diff review. Review sessions over a real repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;
use helix_view::{input::parse_macro, view};
use tempfile::TempDir;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[cfg(windows)]
use crossterm::event::{Event, KeyEvent};
#[cfg(not(windows))]
use termina::event::{Event, KeyEvent};

use super::*;

mod stage;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env("GIT_TERMINAL_PROMPT", "false")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_AUTHOR_NAME", "author")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .env("GIT_COMMITTER_NAME", "committer")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_0", "false")
        .output()
        .unwrap_or_else(|err| panic!("`git {args:?}` failed to run: {err}"));
    assert!(
        output.status.success(),
        "`git {args:?}` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(repo: &TempDir, name: &str, text: Option<&str>) {
    let path = repo.path().join(name);
    match text {
        Some(text) => std::fs::write(path, text).unwrap(),
        None => std::fs::remove_file(path).unwrap(),
    }
}

/// A repository with one commit of `committed`, then `changes` written over it.
/// A `None` change deletes the file.
fn repo(committed: &[(&str, &str)], changes: &[(&str, Option<&str>)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "--quiet"]);
    for (name, text) in committed {
        write(&repo, name, Some(text));
    }
    git(repo.path(), &["add", "--all"]);
    git(repo.path(), &["commit", "--quiet", "--message", "base"]);
    for (name, text) in changes {
        write(&repo, name, *text);
    }
    repo
}

fn config() -> Config {
    let mut config = test_config();
    config.keys = hashmap! {
        Mode::Normal => keymap!({"Normal mode"
            "C-j" => review_next_hunk,
            "C-k" => review_prev_hunk,
            "C-l" => review_next_file,
            "C-h" => review_prev_file,
        }),
    };
    config
}

fn app_on(file: PathBuf) -> anyhow::Result<Application> {
    AppBuilder::new()
        .with_config(config())
        .with_file(file, None)
        .build()
}

/// Sends `keys`, then runs the event loop until `done` holds. `:review` lists
/// the changed files in the background.
async fn run_until(
    app: &mut Application,
    keys: &str,
    done: impl Fn(&Application) -> bool,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut rx = UnboundedReceiverStream::new(rx);
    for key in parse_macro(keys)? {
        tx.send(Ok(Event::Key(KeyEvent::from(key))))?;
    }
    for _ in 0..20 {
        // Going idle clears the timer, and a cleared timer never ends the loop.
        app.editor.reset_idle_timer();
        app.event_loop_until_idle(&mut rx).await;
        if done(app) {
            // Arm it for the next run of the loop, which may get no events.
            app.editor.reset_idle_timer();
            return Ok(());
        }
    }
    bail!("`{keys}` never finished")
}

async fn start_review(app: &mut Application, command: &str) -> anyhow::Result<()> {
    run_until(app, command, |app| view!(app.editor).review.is_some()).await
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn current_file(app: &Application) -> String {
    file_name(doc!(app.editor).path().unwrap())
}

fn review_files(app: &Application) -> Vec<String> {
    let session = view!(app.editor).review.as_ref().unwrap();
    session
        .files
        .iter()
        .map(|change| file_name(change.path()))
        .collect()
}

fn review_index(app: &Application) -> usize {
    view!(app.editor).review.as_ref().unwrap().current
}

fn selected(app: &Application) -> String {
    let (view, doc) = current_ref!(app.editor);
    doc.selection(view.id)
        .primary()
        .fragment(doc.text().slice(..))
        .into_owned()
}

fn diff_base(app: &Application) -> String {
    let diff = doc!(app.editor).diff_handle().unwrap().load();
    diff.diff_base().to_string()
}

fn status(app: &Application) -> String {
    app.editor
        .get_status()
        .map(|(message, _)| message.to_string())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn review_walks_files_and_hunks_in_path_order() -> anyhow::Result<()> {
    let repo = repo(
        &[
            ("a.txt", "a1\na2\na3\na4\na5\n"),
            ("b.txt", "b1\nb2\n"),
            ("d.txt", "d1\n"),
            ("e.txt", "e1\ne2\ne3\ne4\n"),
        ],
        &[
            ("a.txt", Some("a1\nA2\na3\na4\nA5\n")),
            ("b.txt", Some("b1\nB2\n")),
            ("c.txt", Some("c1\n")),
            ("d.txt", None),
            ("e.txt", None),
            ("f.txt", Some("e1\ne2\ne3\nE4\n")),
        ],
    );
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app, ":review<ret>").await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                None,
                Some(&|app| {
                    assert_eq!(
                        review_files(app),
                        ["a.txt", "b.txt", "c.txt", "d.txt", "f.txt"]
                    );
                    assert!(view!(app.editor).inline_diff.enabled);
                    assert_eq!(current_file(app), "a.txt");
                    assert_eq!(selected(app), "A2\n");
                    assert!(status(app).starts_with("Review 1/5: "));
                }),
            ),
            (
                Some("<C-j>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "a.txt");
                    assert_eq!(selected(app), "A5\n");
                }),
            ),
            // Past the last hunk, the next file opens at its first hunk.
            (
                Some("<C-j>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "b.txt");
                    assert_eq!(review_index(app), 1);
                    assert_eq!(selected(app), "B2\n");
                }),
            ),
            // Before the first hunk, the previous file opens at its last hunk.
            (
                Some("<C-k>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "a.txt");
                    assert_eq!(review_index(app), 0);
                    assert_eq!(selected(app), "A5\n");
                }),
            ),
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "b.txt");
                    assert_eq!(selected(app), "B2\n");
                }),
            ),
            // An untracked file diffs against an empty base.
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "c.txt");
                    assert_eq!(selected(app), "c1\n");
                    assert!(status(app).ends_with("c.txt (new)"));
                }),
            ),
            // A deleted file opens empty, with its HEAD text as the base.
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "d.txt");
                    assert!(status(app).ends_with("d.txt (deleted)"));
                    assert_eq!(diff_base(app), "d1\n");
                }),
            ),
            // A renamed file diffs against its old path.
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "f.txt");
                    assert_eq!(selected(app), "E4\n");
                    assert!(status(app).ends_with("e.txt)"));
                }),
            ),
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "f.txt");
                    assert_eq!(status(app), "This is the last file in the review");
                }),
            ),
            (
                Some("<C-j>"),
                Some(&|app| {
                    assert_eq!(selected(app), "E4\n");
                    assert_eq!(status(app), "There are no more changes in the review");
                }),
            ),
            (
                Some("2<C-h>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "c.txt");
                    assert_eq!(review_index(app), 2);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_refreshes_a_base_that_a_commit_made_stale() -> anyhow::Result<()> {
    let repo = repo(&[("b.txt", "b1\nb2\n")], &[("b.txt", Some("b1\nB2\nb3\n"))]);
    let mut app = app_on(repo.path().join("b.txt"))?;
    // Commit part of the change after b.txt loaded its base.
    write(&repo, "b.txt", Some("b1\nB2\n"));
    git(
        repo.path(),
        &["commit", "--quiet", "--all", "--message", "B2"],
    );
    write(&repo, "b.txt", Some("b1\nB2\nb3\n"));
    start_review(&mut app, ":review<ret>").await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert_eq!(current_file(app), "b.txt");
            assert_eq!(selected(app), "b3\n");
            assert_eq!(diff_base(app), "b1\nB2\n");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_staged_lists_only_staged_files() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", "a1\n"), ("b.txt", "b1\n")],
        &[("a.txt", Some("A1\n")), ("b.txt", Some("B1\n"))],
    );
    git(repo.path(), &["add", "b.txt"]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review bogus<ret>", |app| {
        !status(app).is_empty()
    })
    .await?;
    assert_eq!(
        status(&app),
        "'review': 'bogus' is not one of unstaged, staged, or all"
    );
    start_review(&mut app, ":review staged<ret>").await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert_eq!(review_files(app), ["b.txt"]);
            assert_eq!(current_file(app), "b.txt");
            assert_eq!(selected(app), "B1\n");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_without_changes_says_so() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", "a1\n")], &[]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review<ret>", |app| !status(app).is_empty()).await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert_eq!(status(app), "There are no changes to review");
            assert!(view!(app.editor).review.is_none());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_review_hunk_steps_stay_in_the_file() -> anyhow::Result<()> {
    let mut app = AppBuilder::new()
        .with_config(config())
        .with_input_text(TEXT)
        .build()?;
    set_diff_base(&mut app, BASE).await;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-j><C-j>"),
                Some(&|app| {
                    assert_eq!(selected(app), "line two\n");
                    assert_eq!(status(app), "");
                }),
            ),
            (
                Some("<C-l>"),
                Some(&|app| {
                    assert_eq!(
                        status(app),
                        "This view has no review. Start one with :review"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}
