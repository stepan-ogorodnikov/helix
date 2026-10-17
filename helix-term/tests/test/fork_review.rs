//! FORK: review-stream. `:review` over a real repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;
use helix_term::application::Application;
use helix_view::document::Mode;
use helix_view::input::parse_macro;
use helix_view::review::Stream;
use helix_view::{current_ref, doc, view};
use tempfile::TempDir;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[cfg(windows)]
use crossterm::event::{Event, KeyEvent};
#[cfg(not(windows))]
use termina::event::{Event, KeyEvent};

use super::*;

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

/// A repository with one commit of `committed`, then `changes` written over it.
/// A `None` change deletes the file.
fn repo(committed: &[(&str, &str)], changes: &[(&str, Option<&str>)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "--quiet"]);
    for (name, text) in committed {
        std::fs::write(repo.path().join(name), text).unwrap();
    }
    git(repo.path(), &["add", "--all"]);
    git(repo.path(), &["commit", "--quiet", "--message", "base"]);
    for (name, text) in changes {
        let path = repo.path().join(name);
        match text {
            Some(text) => std::fs::write(path, text).unwrap(),
            None => std::fs::remove_file(path).unwrap(),
        }
    }
    repo
}

fn app_on(file: PathBuf) -> anyhow::Result<Application> {
    AppBuilder::new().with_file(file, None).build()
}

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
        // WHY: `:review` reads the files in the background, past the first idle.
        // Going idle clears the timer, and a cleared timer never ends the loop.
        app.editor.reset_idle_timer();
        app.event_loop_until_idle(&mut rx).await;
        if done(app) {
            // WHY: the next run of the loop may get no events.
            app.editor.reset_idle_timer();
            return Ok(());
        }
    }
    bail!("`{keys}` never finished")
}

async fn start_review(app: &mut Application) -> anyhow::Result<()> {
    run_until(app, ":review<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await
}

fn stream(app: &Application) -> &Stream {
    app.editor.review.stream().unwrap()
}

fn stream_text(app: &Application) -> String {
    app.editor
        .document(stream(app).doc)
        .unwrap()
        .text()
        .to_string()
}

fn in_stream(app: &Application) -> bool {
    doc!(app.editor).id() == stream(app).doc
}

fn shown(path: PathBuf) -> String {
    helix_stdx::path::get_relative_path(path)
        .display()
        .to_string()
}

fn status(app: &Application) -> String {
    app.editor
        .get_status()
        .map(|(message, _)| message.to_string())
        .unwrap_or_default()
}

fn selected(app: &Application) -> String {
    let (view, doc) = current_ref!(app.editor);
    doc.selection(view.id)
        .primary()
        .fragment(doc.text().slice(..))
        .into_owned()
}

fn cursor(app: &Application) -> (usize, usize) {
    let (view, doc) = current_ref!(app.editor);
    let text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    let line = text.char_to_line(cursor);
    (line, cursor - text.line_to_char(line))
}

const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

#[tokio::test(flavor = "multi_thread")]
async fn review_shows_every_changed_file_in_one_read_only_buffer() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", TEN), ("gone.txt", "old\n")],
        &[
            ("a.txt", Some("1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n")),
            ("gone.txt", None),
            ("new.txt", Some("fresh\n")),
        ],
    );
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert!(in_stream(app));
            let path = |name| shown(repo.path().join(name));
            assert_eq!(
                stream_text(app),
                format!(
                    "{}  modified\n\
                     ⋯ 3 unchanged lines\n\
                     4\n5\n6\n7\nseven\n8\n9\n10\n\
                     {}  deleted\n\
                     old\n\
                     {}  new file\n\
                     fresh\n",
                    path("a.txt"),
                    path("gone.txt"),
                    path("new.txt"),
                )
            );
            let doc = doc!(app.editor);
            assert!(doc.readonly);
            assert!(!doc.is_modified());
            assert_eq!(
                view!(app.editor).gutters.line_numbers.min_width,
                stream(app).gutter_width()
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_shows_unsaved_edits_of_an_open_buffer() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", TEN)],
        &[("a.txt", Some("1\n2\n3\n4\n5\n6\n7\n8\n9\nTEN\n"))],
    );
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    run_until(&mut app, "ggOzero<esc>", |app| {
        doc!(app.editor).is_modified()
    })
    .await?;
    start_review(&mut app).await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert_eq!(
                stream_text(app),
                format!(
                    "{}  modified\n\
                     zero\n1\n2\n3\n\
                     ⋯ 3 unchanged lines\n\
                     7\n8\n9\n10\nTEN\n",
                    shown(file.clone())
                )
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_says_when_nothing_changed() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", TEN)], &[]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review<ret>", |app| {
        status(app) == "There are no changes to review"
    })
    .await?;
    assert!(app.editor.review.stream().is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_in_the_review_are_put_back() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", TEN)], &[("a.txt", Some("1\n2\nthree\n4\n"))]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;
    let before = stream_text(&app);
    let unchanged = |app: &Application| {
        assert!(in_stream(app));
        assert_eq!(stream_text(app), before);
        assert!(!doc!(app.editor).is_modified());
        assert_eq!(app.editor.mode, Mode::Normal);
    };

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("jjjxd"),
                Some(&|app| {
                    unchanged(app);
                    assert_eq!(selected(app), "3\n", "the selection comes back");
                    assert_eq!(status(app), "The review is read-only. ret opens the file.");
                }),
            ),
            (
                Some("u"),
                Some(&|app| {
                    unchanged(app);
                    assert_eq!(status(app), "Already at oldest change");
                }),
            ),
            (Some("ihello"), Some(&unchanged)),
            (Some("<esc>:line-ending crlf<ret>"), Some(&unchanged)),
            (Some("ra"), Some(&unchanged)),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn ret_opens_the_file_at_the_row_and_c_o_comes_back() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", TEN)],
        &[("a.txt", Some("1\n2\n3\n4\n5\n6\n7\nnew\n8\n9\nTEN\n"))],
    );
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    let in_file = |app: &Application| doc!(app.editor).path() == Some(&file);

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/new<ret><ret>"),
                Some(&|app| {
                    assert!(in_file(app));
                    assert_eq!(
                        cursor(app),
                        (7, 2),
                        "an added row opens its line, at the column"
                    );
                    assert_eq!(view!(app.editor).gutters.line_numbers.min_width, 3);
                }),
            ),
            (
                Some("<C-o>"),
                Some(&|app| {
                    assert!(in_stream(app));
                    assert_eq!(
                        view!(app.editor).gutters.line_numbers.min_width,
                        stream(app).gutter_width()
                    );
                }),
            ),
            (
                Some("/^10<ret><ret>"),
                Some(&|app| {
                    assert!(in_file(app));
                    assert_eq!(
                        cursor(app),
                        (10, 0),
                        "a removed row opens the line below it"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

/// `a.txt` with its seventh line changed, as the review shows it. Row 1 is the
/// gap, row 6 `seven`.
fn seventh_changed() -> TempDir {
    repo(
        &[("a.txt", TEN)],
        &[("a.txt", Some("1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n"))],
    )
}

fn review_of(file: &Path, rows: &str) -> String {
    format!("{}  modified\n{rows}", shown(file.to_path_buf()))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_in_the_file_shows_when_you_come_back() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/seven<ret><ret>Ix<esc><C-o>"),
                Some(&|app| {
                    assert!(in_stream(app));
                    assert_eq!(
                        stream_text(app),
                        review_of(&file, "⋯ 3 unchanged lines\n4\n5\n6\n7\nxseven\n8\n9\n10\n")
                    );
                    assert_eq!(cursor(app).0, 6, "the cursor stays on the edited line");
                }),
            ),
            (
                Some("/^5$<ret><ret>Ix<esc><C-o>"),
                Some(&|app| {
                    assert_eq!(
                        stream_text(app),
                        review_of(
                            &file,
                            "⋯ 1 unchanged line\n2\n3\n4\n5\nx5\n6\n7\nxseven\n8\n9\n10\n"
                        ),
                        "an edited context line gets its old version above it"
                    );
                    assert_eq!(cursor(app).0, 6, "the cursor stays on the edited line");
                }),
            ),
            (
                Some("/xseven<ret><ret>xc7<esc>/x5<ret>xc5<esc>:review<ret>"),
                Some(&|app| {
                    assert_eq!(
                        stream_text(app),
                        review_of(&file, "").replace("modified\n", "modified, no text changes\n"),
                        "lines put back as they were in HEAD lose their rows"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn ret_on_a_gap_shows_its_lines_and_they_stay_shown() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    let opened = |seventh: &str| {
        review_of(
            &file,
            &format!("1\n2\n3\n4\n5\n6\n7\n{seventh}\n8\n9\n10\n"),
        )
    };

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("j<ret>"),
                Some(&|app| {
                    assert!(in_stream(app));
                    assert_eq!(stream_text(app), opened("seven"));
                    assert_eq!(
                        cursor(app),
                        (1, 0),
                        "the cursor lands on the first line shown"
                    );
                }),
            ),
            (
                Some("/seven<ret><ret>Ix<esc><C-o>"),
                Some(&|app| assert_eq!(stream_text(app), opened("xseven"))),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_switches_to_the_open_review() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;

    test_key_sequence(
        &mut app,
        Some("jjj<ret>:review<ret>"),
        Some(&|app| {
            assert!(in_stream(app));
            assert_eq!(cursor(app), (3, 0));
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_refresh_reads_the_changes_again_and_keeps_the_place() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let mut app = app_on(repo.path().join("b.txt"))?;
    start_review(&mut app).await?;
    run_until(&mut app, "j<ret>", |app| !stream_text(app).contains('⋯')).await?;
    std::fs::write(
        repo.path().join("a.txt"),
        "1\n2\n3\n4\n5\n6\nseven\n8\n9\nten\n",
    )?;
    std::fs::write(repo.path().join("new.txt"), "fresh\n")?;
    run_until(&mut app, ":review-refresh<ret>", |app| {
        stream_text(app).contains("fresh")
    })
    .await?;

    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert!(in_stream(app));
            assert_eq!(
                stream_text(app),
                format!(
                    "{}  modified\n1\n2\n3\n4\n5\n6\n7\nseven\n8\n9\n10\nten\n\
                     {}  new file\nfresh\n",
                    shown(repo.path().join("a.txt")),
                    shown(repo.path().join("new.txt")),
                ),
                "the opened gap stays open"
            );
            assert_eq!(cursor(app), (1, 0));
        }),
        false,
    )
    .await
}
