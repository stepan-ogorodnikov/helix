//! FORK: diff review. The index as a review base, and staging.

use helix_loader::workspace_trust::WorkspaceTrust;
use helix_view::review::ReviewBase;

use super::*;

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

fn review_base(app: &Application) -> Option<ReviewBase> {
    view!(app.editor)
        .review
        .as_ref()
        .map(|session| session.base)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unstaged_review_diffs_against_the_index() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", "a1\na2\na3\n")],
        &[("a.txt", Some("A1\na2\na3\n"))],
    );
    git(repo.path(), &["add", "a.txt"]);
    write(&repo, "a.txt", Some("A1\na2\nA3\n"));
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app, ":review unstaged<ret>").await?;
    assert_eq!(review_base(&app), Some(ReviewBase::Index));
    assert_eq!(diff_base(&app), "A1\na2\na3\n");
    assert_eq!(selected(&app), "A3\n");

    run_until(&mut app, ":review<ret>", |app| {
        review_base(app) == Some(ReviewBase::Head)
    })
    .await?;
    test_key_sequence(
        &mut app,
        None,
        Some(&|app| {
            assert_eq!(diff_base(app), "a1\na2\na3\n");
            assert_eq!(selected(app), "A1\n");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn staging_takes_a_hunk_out_of_an_unstaged_review() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", "a1\na2\na3\n")],
        &[("a.txt", Some("A1\na2\nA3\n"))],
    );
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app, ":review unstaged<ret>").await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":stage-hunk<ret>"),
                Some(&|app| {
                    assert_eq!(status(app), "Staged 1 change");
                    assert_eq!(index_text(&repo, "a.txt").unwrap(), "A1\na2\na3\n");
                    // The review's base follows the index, so the staged hunk is gone.
                    assert_eq!(diff_base(app), "A1\na2\na3\n");
                }),
            ),
            (
                Some(":stage-hunk<ret>"),
                Some(&|app| {
                    assert_eq!(
                        status(app),
                        "'stage-hunk': There are no unstaged changes under any selection"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn unstaging_puts_back_the_head_text() -> anyhow::Result<()> {
    let repo = repo(
        &[("a.txt", "a1\na2\na3\n")],
        &[("a.txt", Some("A1\na2\nA3\n"))],
    );
    git(repo.path(), &["add", "a.txt"]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app, ":review<ret>").await?;

    test_key_sequence(
        &mut app,
        Some(":unstage-hunk<ret>"),
        Some(&|app| {
            assert_eq!(status(app), "Unstaged 1 change");
            assert_eq!(index_text(&repo, "a.txt").unwrap(), "a1\na2\nA3\n");
            // A HEAD review still shows the unstaged hunk.
            assert_eq!(diff_base(app), "a1\na2\na3\n");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_file_can_be_staged_and_unstaged() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", "a1\n")], &[("c.txt", Some("c1\nc2\n"))]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    start_review(&mut app, ":review unstaged<ret>").await?;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":stage-hunk<ret>"),
                Some(&|app| {
                    assert_eq!(current_file(app), "c.txt");
                    assert_eq!(status(app), "Staged 1 change");
                    assert_eq!(index_text(&repo, "c.txt").unwrap(), "c1\nc2\n");
                }),
            ),
            // Unstaging all of a file HEAD doesn't have removes it from the index.
            (
                Some(":unstage-hunk<ret>"),
                Some(&|app| {
                    assert_eq!(status(app), "Unstaged 1 change");
                    assert_eq!(index_text(&repo, "c.txt"), None);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn staging_needs_a_trusted_workspace() -> anyhow::Result<()> {
    let repo = repo(&[("a.txt", "a1\n")], &[("a.txt", Some("A1\n"))]);
    let mut app = app_on(repo.path().join("a.txt"))?;
    app.editor.workspace_trust = WorkspaceTrust::new(Default::default());

    test_key_sequence(
        &mut app,
        Some(":stage-hunk<ret>"),
        Some(&|app| {
            assert_eq!(
                status(app),
                "'stage-hunk': Staging runs git, which needs a trusted workspace. See :workspace-trust"
            );
            assert_eq!(index_text(&repo, "a.txt").unwrap(), "a1\n");
        }),
        false,
    )
    .await
}
