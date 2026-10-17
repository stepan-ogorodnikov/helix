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
    bail!(
        "`{keys}` never finished. The status says: {:?}",
        status(app)
    )
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
async fn review_shows_every_changed_file_in_one_buffer() -> anyhow::Result<()> {
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
            assert!(!doc.readonly, "only a staged review shows as read-only");
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
async fn edits_outside_a_files_lines_are_put_back() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", TEN)], &[("a.txt", Some("1\n2\nthree\n4\n"))]);
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    let before = stream_text(&app);
    let unchanged = |app: &Application| {
        assert!(in_stream(app));
        assert_eq!(stream_text(app), before);
        assert!(!doc!(app.editor).is_modified());
        assert_eq!(app.editor.mode, Mode::Normal);
        assert_eq!(buffer_text(app, &file).as_deref(), Some("1\n2\nthree\n4\n"));
    };

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("jjjxd"),
                Some(&|app| {
                    unchanged(app);
                    assert_eq!(selected(app), "3\n", "the selection comes back");
                    assert_eq!(
                        status(app),
                        "Removed lines, headers, and gaps can't be edited"
                    );
                }),
            ),
            (
                Some("u"),
                Some(&|app| {
                    unchanged(app);
                    assert_eq!(status(app), "Already at oldest change");
                }),
            ),
            (Some("ih"), Some(&unchanged)),
            (Some(":line-ending crlf<ret>"), Some(&unchanged)),
            (Some("ra"), Some(&unchanged)),
            (Some("ggih"), Some(&unchanged)),
            (
                Some("/three<ret>o"),
                Some(&|app| {
                    unchanged(app);
                    assert_eq!(status(app), "The review can't add or remove lines yet");
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn typing_in_a_row_edits_the_files_buffer_and_the_rows_follow() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    let original = "1\n2\n3\n4\n5\n6\nseven\n8\n9\n10\n";
    let edited = |app: &Application| {
        assert_eq!(
            buffer_text(app, &file).as_deref(),
            Some("1\n2\n3\n4\n5xy\n6\nseven\n8\n9\n10\n")
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            original,
            "and doesn't save it"
        );
        assert!(in_stream(app));
        assert_eq!(
            stream_text(app),
            review_of(
                &file,
                "⋯ 1 unchanged line\n2\n3\n4\n5\n5xy\n6\n7\nseven\n8\n9\n10\n"
            ),
            "the edited line gets its old version above it"
        );
        assert_eq!(cursor(app).0, 6, "the cursor stays on the edited line");
        assert!(!doc!(app.editor).is_modified());
    };

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("/^5<ret>Axy"),
                Some(&|app| {
                    edited(app);
                    assert_eq!(app.editor.mode, Mode::Insert, "while you type");
                }),
            ),
            (Some("<esc>"), Some(&edited)),
            (
                Some("<ret>u"),
                Some(&|app| {
                    assert_eq!(buffer_text(app, &file).as_deref(), Some(original));
                    assert_eq!(
                        cursor(app),
                        (4, 1),
                        "one insert is one undo step, and it puts the cursor where the edit was"
                    );
                }),
            ),
            (
                Some("<C-o>"),
                Some(&|app| {
                    assert_eq!(
                        stream_text(app),
                        review_of(&file, "⋯ 3 unchanged lines\n4\n5\n6\n7\nseven\n8\n9\n10\n")
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_loads_a_file_that_isnt_open() -> anyhow::Result<()> {
    let repo = three_hunks();
    let b = repo.path().join("b.txt");
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;

    test_key_sequence(
        &mut app,
        Some("/^B<ret>iX<esc>"),
        Some(&|app| {
            assert_eq!(buffer_text(app, &b).as_deref(), Some("XB\n"));
            let doc = app.editor.document_by_path(&b).unwrap();
            assert!(doc.is_modified());
            assert!(
                app.editor
                    .tree
                    .views()
                    .all(|(view, _)| view.doc != doc.id()),
                "in the background"
            );
            assert_eq!(std::fs::read_to_string(&b).unwrap(), "B\n");
            assert!(stream_text(app).ends_with("b.txt  modified\nb\nXB\n"));
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_completion_in_a_row_reaches_the_file_once() -> anyhow::Result<()> {
    let repo = tempfile::tempdir()?;
    git(repo.path(), &["init", "--quiet"]);
    let dir = repo.path().join("files");
    std::fs::create_dir(&dir)?;
    std::fs::write(dir.join("only.txt"), "")?;
    let file = repo.path().join("a.txt");
    let line = format!("{}/", dir.display());
    std::fs::write(&file, format!("{line}\nold\n"))?;
    git(repo.path(), &["add", "--all"]);
    git(repo.path(), &["commit", "--quiet", "--message", "base"]);
    std::fs::write(&file, format!("{line}\nnew\n"))?;

    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;
    let completed = format!("{line}only.txt\nnew\n");

    test_key_sequences(
        &mut app,
        vec![
            (Some("jA<C-x>"), None),
            (
                Some("<C-n>"),
                Some(&|app| {
                    assert_eq!(buffer_text(app, &file), Some(completed.clone()));
                }),
            ),
            (
                Some("<ret>"),
                Some(&|app| {
                    assert_eq!(
                        app.editor.mode,
                        Mode::Insert,
                        "the completion isn't put back: {}",
                        status(app)
                    );
                    assert_eq!(buffer_text(app, &file), Some(completed.clone()));
                }),
            ),
            (
                Some("<esc>"),
                Some(&|app| {
                    assert_eq!(buffer_text(app, &file), Some(completed.clone()));
                    assert_eq!(
                        stream_text(app),
                        review_of(&file, &format!("{line}\nold\n{line}only.txt\nnew\n")),
                        "the rows follow once the menu closes"
                    );
                }),
            ),
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

/// Two hunks in `a.txt`, one in `b.txt`. Hunks are rows 2–3, 10–11, and 14–15,
/// headers rows 0 and 13.
fn three_hunks() -> TempDir {
    repo(
        &[("a.txt", TEN), ("b.txt", "b\n")],
        &[
            ("a.txt", Some("1\ntwo\n3\n4\n5\n6\n7\n8\nnine\n10\n")),
            ("b.txt", Some("B\n")),
        ],
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn hunk_and_file_motions_cross_files() -> anyhow::Result<()> {
    let repo = three_hunks();
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("]g"),
                Some(&|app| assert_eq!(selected(app), "2\ntwo\n")),
            ),
            (
                Some("]g"),
                Some(&|app| assert_eq!(selected(app), "9\nnine\n")),
            ),
            (
                Some("[g"),
                Some(&|app| assert_eq!(selected(app), "2\ntwo\n", "the hunk above the cursor")),
            ),
            (
                Some("2]g"),
                Some(&|app| assert_eq!(selected(app), "b\nB\n", "into the next file")),
            ),
            (
                Some("[G"),
                Some(&|app| assert_eq!(selected(app), "2\ntwo\n")),
            ),
            (Some("]G"), Some(&|app| assert_eq!(selected(app), "b\nB\n"))),
            (Some("gg]F"), Some(&|app| assert_eq!(cursor(app), (13, 0)))),
            (
                Some("j[F"),
                Some(&|app| assert_eq!(cursor(app), (13, 0), "a row's previous header is its own")),
            ),
            (Some("[F"), Some(&|app| assert_eq!(cursor(app), (0, 0)))),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_custom_key_for_goto_next_change_moves_by_review_hunk() -> anyhow::Result<()> {
    use helix_core::hashmap;
    use helix_term::keymap;

    let repo = three_hunks();
    let mut config = test_config();
    config.keys = hashmap! {
        Mode::Normal => keymap!({ "Normal mode"
            "C-n" => goto_next_change,
        }),
    };
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(repo.path().join("a.txt"), None)
        .build()?;
    start_review(&mut app).await?;

    test_key_sequence(
        &mut app,
        Some("<C-n><C-n>"),
        Some(&|app| assert_eq!(selected(app), "9\nnine\n")),
        false,
    )
    .await
}

/// `a.txt` with line 2 changed and staged, then line 9 changed and not.
fn staged_and_unstaged() -> TempDir {
    let repo = repo(
        &[("a.txt", TEN)],
        &[("a.txt", Some("1\ntwo\n3\n4\n5\n6\n7\n8\n9\n10\n"))],
    );
    git(repo.path(), &["add", "a.txt"]);
    std::fs::write(
        repo.path().join("a.txt"),
        "1\ntwo\n3\n4\n5\n6\n7\n8\nnine\n10\n",
    )
    .unwrap();
    repo
}

#[tokio::test(flavor = "multi_thread")]
async fn review_scopes_diff_head_the_index_and_the_files() -> anyhow::Result<()> {
    let repo = staged_and_unstaged();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    let status_is = |message: &'static str| move |app: &Application| status(app) == message;

    run_until(
        &mut app,
        ":review unstaged<ret>",
        status_is("Reviewing unstaged changes"),
    )
    .await?;
    assert_eq!(
        stream_text(&app),
        review_of(&file, "⋯ 5 unchanged lines\n6\n7\n8\n9\nnine\n10\n"),
        "the index against the file"
    );
    run_until(
        &mut app,
        ":review staged<ret>",
        status_is("Reviewing staged changes"),
    )
    .await?;
    assert_eq!(
        stream_text(&app),
        review_of(&file, "1\n2\ntwo\n3\n4\n5\n⋯ 5 unchanged lines\n"),
        "HEAD against the index"
    );
    run_until(
        &mut app,
        ":review all<ret>",
        status_is("Reviewing all changes"),
    )
    .await?;
    assert_eq!(
        stream_text(&app),
        review_of(&file, "1\n2\ntwo\n3\n4\n5\n6\n7\n8\n9\nnine\n10\n"),
        "HEAD against the file"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_staged_review_takes_no_edits() -> anyhow::Result<()> {
    let repo = staged_and_unstaged();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    run_until(&mut app, ":review staged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;
    let before = stream_text(&app);

    test_key_sequence(
        &mut app,
        Some("/two<ret>ix"),
        Some(&|app| {
            assert_eq!(stream_text(app), before);
            assert_eq!(app.editor.mode, Mode::Normal);
            assert!(doc!(app.editor).readonly);
            assert_eq!(
                status(app),
                "A staged review is read-only. ret opens the file."
            );
            assert_eq!(
                buffer_text(app, &file).as_deref(),
                Some("1\ntwo\n3\n4\n5\n6\n7\n8\nnine\n10\n")
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_staged_review_ignores_edits_to_the_file() -> anyhow::Result<()> {
    let repo = staged_and_unstaged();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    run_until(&mut app, ":review staged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;
    let before = stream_text(&app);

    test_key_sequence(
        &mut app,
        Some("/two<ret><ret>Ix<esc><C-o>"),
        Some(&|app| {
            assert!(in_stream(app));
            assert_eq!(stream_text(app), before);
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_names_the_scopes_it_knows() -> anyhow::Result<()> {
    let repo = staged_and_unstaged();
    test_key_sequence(
        &mut app_on(repo.path().join("a.txt"))?,
        Some(":review head<ret>"),
        Some(&|app| {
            assert_eq!(
                status(app),
                "'review': 'head' is not one of all, unstaged, or staged"
            );
        }),
        false,
    )
    .await
}

/// The index version of `name`, or `None` when the index doesn't have it.
fn index_text(repo: &TempDir, name: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["show", &format!(":{name}")])
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap())
}

/// `a.txt` with its first and last lines changed: two hunks.
fn ends_changed() -> TempDir {
    repo(
        &[("a.txt", "a1\na2\na3\n")],
        &[("a.txt", Some("A1\na2\nA3\n"))],
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn staging_takes_the_selected_hunk_out_of_an_unstaged_review() -> anyhow::Result<()> {
    let repo = ends_changed();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    run_until(&mut app, ":review unstaged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;
    run_until(&mut app, "]g:stage-hunk<ret>", |app| {
        status(app) == "Staged 1 change" && !stream_text(app).contains("a1")
    })
    .await?;
    assert_eq!(index_text(&repo, "a.txt").unwrap(), "A1\na2\na3\n");
    assert_eq!(stream_text(&app), review_of(&file, "A1\na2\na3\nA3\n"));

    test_key_sequence(
        &mut app,
        Some("gg:stage-hunk<ret>"),
        Some(&|app| {
            assert_eq!(
                status(app),
                "'stage-hunk': There are no unstaged changes under any selection"
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn unstaging_puts_back_the_head_text() -> anyhow::Result<()> {
    let repo = ends_changed();
    git(repo.path(), &["add", "a.txt"]);
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    run_until(&mut app, ":review staged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;
    let staged_first_line = review_of(&file, "a1\nA1\na2\na3\n");
    run_until(&mut app, "]G:unstage-hunk<ret>", |app| {
        stream_text(app) == staged_first_line
    })
    .await?;
    assert_eq!(status(&app), "Unstaged 1 change");
    assert_eq!(index_text(&repo, "a.txt").unwrap(), "A1\na2\na3\n");

    // WHY: wait for the read, or `[G` runs in the staged review.
    run_until(&mut app, ":review all<ret>", |app| {
        status(app) == "Reviewing all changes"
    })
    .await?;
    run_until(&mut app, "[G:unstage-hunk<ret>", |app| {
        status(app) == "Unstaged 1 change"
    })
    .await?;
    assert_eq!(
        index_text(&repo, "a.txt").unwrap(),
        "a1\na2\na3\n",
        "an `all` review unstages too"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_file_can_be_staged_and_unstaged() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", "a1\n")], &[("c.txt", Some("c1\nc2\n"))]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review unstaged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;
    run_until(&mut app, "]g:stage-hunk<ret>", |app| {
        status(app) == "There are no unstaged changes to review"
    })
    .await?;
    assert_eq!(index_text(&repo, "c.txt").unwrap(), "c1\nc2\n");
    assert_eq!(stream_text(&app), "");

    run_until(&mut app, ":review staged<ret>", |app| {
        stream_text(app).contains("c1")
    })
    .await?;
    run_until(&mut app, "]g:unstage-hunk<ret>", |app| {
        status(app) == "There are no staged changes to review"
    })
    .await?;
    assert_eq!(index_text(&repo, "c.txt"), None);
    Ok(())
}

fn buffer_text(app: &Application, path: &Path) -> Option<String> {
    app.editor
        .document_by_path(path)
        .map(|doc| doc.text().to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn review_restore_puts_the_base_back_in_each_buffer() -> anyhow::Result<()> {
    let repo = three_hunks();
    let (a, b) = (repo.path().join("a.txt"), repo.path().join("b.txt"));
    let mut app = app_on(a.clone())?;
    start_review(&mut app).await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("]G:review-restore<ret>"),
                Some(&|app| {
                    assert_eq!(status(app), "Restored 1 change");
                    assert_eq!(
                        buffer_text(app, &b).as_deref(),
                        Some("b\n"),
                        "a file that isn't open gets a buffer"
                    );
                    assert_eq!(
                        std::fs::read_to_string(&b).unwrap(),
                        "B\n",
                        "and isn't saved"
                    );
                    assert!(!stream_text(app).contains("B\n"), "the review follows it");
                }),
            ),
            (
                Some("gg]g]g:review-restore<ret>"),
                Some(&|app| {
                    assert_eq!(
                        buffer_text(app, &a).as_deref(),
                        Some("1\ntwo\n3\n4\n5\n6\n7\n8\n9\n10\n"),
                        "only the selected hunk"
                    );
                    assert!(app.editor.document_by_path(&a).unwrap().is_modified());
                }),
            ),
            (
                Some("/^4<ret><ret>u"),
                Some(&|app| {
                    assert_eq!(
                        buffer_text(app, &a).as_deref(),
                        Some("1\ntwo\n3\n4\n5\n6\n7\n8\nnine\n10\n")
                    );
                    assert_eq!(
                        cursor(app),
                        (8, 0),
                        "u in the file goes to the restored hunk"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_restore_leaves_the_index_to_unstage_hunk() -> anyhow::Result<()> {
    let repo = staged_and_unstaged();
    let mut app = app_on(repo.path().join("a.txt"))?;
    run_until(&mut app, ":review staged<ret>", |app| {
        app.editor.review.stream().is_some()
    })
    .await?;

    test_key_sequence(
        &mut app,
        Some("]g:review-restore<ret>"),
        Some(&|app| {
            assert_eq!(
                status(app),
                "'review-restore': A staged review shows the index, not a file. :unstage-hunk takes a hunk back out"
            );
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn review_context_sets_the_lines_around_each_hunk() -> anyhow::Result<()> {
    let repo = seventh_changed();
    let file = repo.path().join("a.txt");
    let mut app = app_on(file.clone())?;
    start_review(&mut app).await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":review-context 1<ret>"),
                Some(&|app| {
                    assert_eq!(
                        stream_text(app),
                        review_of(
                            &file,
                            "⋯ 5 unchanged lines\n6\n7\nseven\n8\n⋯ 2 unchanged lines\n"
                        )
                    );
                }),
            ),
            (
                Some(":review-context<ret>"),
                Some(&|app| assert_eq!(status(app), "The review shows 1 line of context")),
            ),
            (
                Some(":review-context many<ret>"),
                Some(&|app| {
                    assert_eq!(
                        status(app),
                        "'review-context': 'many' is not a number of lines"
                    )
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn focus_file_puts_the_cursor_on_a_files_header() -> anyhow::Result<()> {
    let repo = three_hunks();
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app).await?;

    assert!(helix_view::review::focus_file(
        &mut app.editor,
        &repo.path().join("b.txt")
    ));
    assert_eq!(cursor(&app), (13, 0));
    assert!(!helix_view::review::focus_file(
        &mut app.editor,
        &repo.path().join("c.txt")
    ));
    assert_eq!(
        cursor(&app),
        (13, 0),
        "a file the review lacks moves nothing"
    );
    Ok(())
}
