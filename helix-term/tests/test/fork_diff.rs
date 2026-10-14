//! FORK: inline diff.

use std::time::Duration;

use helix_core::{hashmap, visual_offset_from_block};
use helix_term::{application::Application, keymap};
use helix_view::{current_ref, doc, doc_mut, document::Mode};

use super::*;

const TEXT: &str = "#[l|]#ine one\nline two\nline three\n";
const BASE: &str = "line one\nold two\nline three\n";

fn app() -> anyhow::Result<Application> {
    let mut config = test_config();
    config.keys = hashmap! {
        Mode::Normal => keymap!({"Normal mode"
            "C-d" => toggle_inline_diff,
        }),
    };
    AppBuilder::new()
        .with_config(config)
        .with_input_text(TEXT)
        .build()
}

async fn set_diff_base(app: &mut Application, base: &str) {
    doc_mut!(app.editor).set_diff_base(base.as_bytes().to_vec());
    // The diff worker runs in the background.
    for _ in 0..200 {
        let diff = doc!(app.editor).diff_handle().map(|diff| diff.load().len());
        if diff.is_some_and(|hunks| hunks > 0) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the diff never finished");
}

/// The visual row of `line` in the focused view, as scrolling and motions see it.
fn line_row(app: &Application, line: usize) -> usize {
    let (view, doc) = current_ref!(app.editor);
    let text = doc.text().slice(..);
    let annotations = view.text_annotations(doc, None);
    let text_fmt = doc.text_format(view.inner_width(doc), None);
    let pos = text.line_to_char(line);
    visual_offset_from_block(text, 0, pos, &text_fmt, &annotations)
        .0
        .row
}

#[tokio::test(flavor = "multi_thread")]
async fn toggle_reserves_rows_above_changed_lines() -> anyhow::Result<()> {
    let mut app = app()?;
    set_diff_base(&mut app, BASE).await;
    assert_eq!(line_row(&app, 2), 2);

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-d>"),
                Some(&|app| {
                    assert!(current_ref!(app.editor).0.inline_diff);
                    // "old two" takes the row between line 0 and line 1.
                    assert_eq!(line_row(app, 1), 2);
                    assert_eq!(line_row(app, 2), 3);
                }),
            ),
            (
                Some("<C-d>"),
                Some(&|app| {
                    assert!(!current_ref!(app.editor).0.inline_diff);
                    assert_eq!(line_row(app, 2), 2);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn toggle_is_per_view() -> anyhow::Result<()> {
    let mut app = app()?;
    set_diff_base(&mut app, BASE).await;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("<C-w>v<C-d>"),
                Some(&|app| {
                    let views: Vec<_> = app.editor.tree.views().collect();
                    assert_eq!(views.len(), 2);
                    for (view, focused) in views {
                        assert_eq!(view.inline_diff, focused);
                    }
                }),
            ),
            // Both views have to close for the app to exit.
            (Some(":qa!<ret>"), None),
        ],
        true,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn editing_with_the_diff_on() -> anyhow::Result<()> {
    let mut app = app()?;
    set_diff_base(&mut app, BASE).await;

    // Deleting everything leaves the diff a frame behind the buffer while it renders.
    test_key_sequence(
        &mut app,
        Some("<C-d>%dihello<esc>jOnew<esc>"),
        Some(&|app| {
            assert_eq!(doc!(app.editor).text().to_string(), "new\nhello");
            assert!(current_ref!(app.editor).0.inline_diff);
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn toggle_without_diff_base_says_so() -> anyhow::Result<()> {
    let mut app = app()?;

    test_key_sequence(
        &mut app,
        Some("<C-d>"),
        Some(&|app| {
            let (status, _) = app.editor.get_status().expect("a status message");
            assert!(status.contains("no diff base"), "{status}");
        }),
        false,
    )
    .await
}
