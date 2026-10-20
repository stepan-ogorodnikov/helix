//! FORK: review-stream. Completion in the review against rust-analyzer, which
//! has to be on the `PATH`. The fixture is the working directory, as the
//! server's root, so the test is ignored and runs alone, at the review's
//! manual check:
//!
//! `cargo integration-test -- --ignored rust_analyzer --test-threads=1`

use std::sync::Arc;
use std::time::Duration;

use helix_core::completion::CompletionProvider;
use helix_view::document::SavePoint;
use tokio::time::{timeout_at, Instant};

use super::*;

const SHAPES: &str = "pub struct Square {
    pub side: f64,
}

impl Square {
    pub fn area(&self) -> f64 {
        self.side * self.side
    }

    pub fn grow(&mut self, by: f64) {
        self.side += by;
    }
}
";

const LIB: &str = "pub mod shapes;

pub fn one() -> u32 {
    1
}

pub fn two() -> u32 {
    2
}

pub fn three() -> u32 {
    3
}

pub fn area() -> f64 {
    let mut s = shapes::Square { side: 2.0 };
    s.side
}
";

/// Sends `keys`, then runs the loop until `done`, for up to `secs`. The
/// server answers on its own time, past the loop's idle.
async fn wait(
    app: &mut Application,
    keys: &str,
    secs: u64,
    done: impl Fn(&Application) -> bool,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut rx = UnboundedReceiverStream::new(rx);
    for key in parse_macro(keys)? {
        tx.send(Ok(Event::Key(KeyEvent::from(key))))?;
    }
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        app.editor.reset_idle_timer();
        // WHY: an editor that never goes idle, as one asking a server over and
        // over, would hang the test.
        let idle = timeout_at(deadline, app.event_loop_until_idle(&mut rx)).await;
        if idle.is_ok() && done(app) {
            app.editor.reset_idle_timer();
            return Ok(());
        }
        if idle.is_err() || Instant::now() > deadline {
            bail!(
                "`{keys}` never finished. The status says: {:?}",
                status(app)
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The menu's context from rust-analyzer, once its items are in.
fn from_server(app: &Application) -> Option<(bool, Arc<SavePoint>)> {
    app.editor
        .handlers
        .completions
        .active_completions
        .iter()
        .find(|(provider, _)| matches!(provider, CompletionProvider::Lsp(_)))
        .map(|(_, context)| (context.is_incomplete, context.savepoint.clone()))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs rust-analyzer, and changes the working directory"]
async fn rust_analyzer_completes_in_the_review() -> anyhow::Result<()> {
    let changed = LIB.replace(
        "    let mut s = shapes::Square { side: 2.0 };\n",
        "    let mut s = shapes::Square { side: 2.0 };\n    let t = 1.0;\n",
    );
    let repo = repo(
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (".gitignore", "target\n"),
        ],
        &[],
    );
    std::fs::create_dir(repo.path().join("src"))?;
    std::fs::write(repo.path().join("src/shapes.rs"), SHAPES)?;
    std::fs::write(repo.path().join("src/lib.rs"), LIB)?;
    git(repo.path(), &["add", "--all"]);
    git(repo.path(), &["commit", "--quiet", "--message", "src"]);
    std::fs::write(repo.path().join("src/lib.rs"), &changed)?;
    std::env::set_current_dir(repo.path())?;
    let lib = repo.path().join("src/lib.rs").canonicalize()?;

    let mut config = test_config();
    config.editor.lsp.enable = true;
    let mut app = AppBuilder::new()
        .with_config(config)
        .with_file(lib.clone(), None)
        .build()?;
    wait(&mut app, "", 90, |app| {
        app.editor
            .document_by_path(&lib)
            .is_some_and(|doc| doc.language_servers().next().is_some())
    })
    .await?;
    start_review(&mut app).await?;
    let text = |app: &Application| buffer_text(app, &lib).unwrap_or_default();

    // WHY: rust-analyzer answers with no items until it has indexed.
    let mut tries = 0;
    while wait(&mut app, "/let t<ret>os.gr", 20, |app| {
        from_server(app).is_some()
    })
    .await
    .is_err()
    {
        tries += 1;
        assert!(tries < 6, "rust-analyzer never offered items");
        wait(&mut app, "<esc>u", 5, |_| true).await?;
    }
    wait(&mut app, "<C-n><ret><esc>", 10, |app| {
        text(app).contains("s.grow(")
    })
    .await?;
    assert_eq!(text(&app).matches("s.grow(").count(), 1, "{}", text(&app));
    assert!(stream_text(&app).contains("s.grow("));

    wait(&mut app, "/let t<ret>oSquar", 20, |app| {
        from_server(app).is_some()
    })
    .await?;
    wait(&mut app, "<C-n><ret><esc>", 10, |app| {
        text(app).contains("use crate::shapes::Square;")
    })
    .await?;
    assert!(
        text(&app).contains("    let t = 1.0;\n    Square\n"),
        "the item, at the cursor's place in the file: {}",
        text(&app)
    );
    assert!(stream_text(&app).contains("use crate::shapes::Square;"));
    wait(&mut app, "u", 10, |app| !text(app).contains("Square\n")).await?;
    assert!(
        !text(&app).contains("use crate::shapes::Square;"),
        "`u` takes out the item and its import: {}",
        text(&app)
    );

    wait(&mut app, "/let t<ret>os.", 20, |app| {
        from_server(app).is_some()
    })
    .await?;
    let (incomplete, asked) = from_server(&app).expect("waited for it");
    assert!(
        incomplete,
        "rust-analyzer's list is complete, so nothing asks again"
    );
    wait(&mut app, "ar", 10, |app| {
        from_server(app).is_some_and(|(_, savepoint)| !Arc::ptr_eq(&savepoint, &asked))
    })
    .await?;
    wait(&mut app, "<C-n><ret><esc>", 10, |app| {
        text(app).contains("s.area()")
    })
    .await?;
    assert_eq!(text(&app).matches("s.area()").count(), 1, "{}", text(&app));
    Ok(())
}
