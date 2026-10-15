//! FORK: inline diff.

use std::time::Duration;

use helix_core::{hashmap, visual_offset_from_block};
use helix_term::keymap;
use helix_term::keymap::Keymaps;
use helix_term::{application::Application, config::Config, ui::EditorView};
use helix_view::graphics::Color;
use helix_view::{current_ref, doc, doc_mut, document::Mode, view_mut, Theme};
use tui::buffer::Buffer;

use super::*;

const TEXT: &str = "#[l|]#ine one\nline two\nline three\n";
const BASE: &str = "line one\nold two\nline three\n";

fn app() -> anyhow::Result<Application> {
    app_with(TEXT, |_| {})
}

fn app_with(text: &str, configure: impl FnOnce(&mut Config)) -> anyhow::Result<Application> {
    let mut config = test_config();
    configure(&mut config);
    config.keys = hashmap! {
        Mode::Normal => keymap!({"Normal mode"
            "C-d" => toggle_inline_diff,
        }),
    };
    AppBuilder::new()
        .with_config(config)
        .with_input_text(text)
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

/// Renders the focused view.
fn render(app: &Application) -> Buffer {
    let (view, doc) = current_ref!(app.editor);
    let mut surface = Buffer::empty(view.area);
    EditorView::new(Keymaps::default()).render_view(
        &app.editor,
        doc,
        view,
        view.area,
        &mut surface,
        true,
    );
    surface
}

/// The cells of row `y` of the focused view, gutter or text, as one string.
fn row_cells(app: &Application, surface: &Buffer, y: u16, text: bool) -> String {
    let (view, doc) = current_ref!(app.editor);
    let text_x = view.area.x + view.gutter_offset(doc);
    let (start, end) = match text {
        true => (text_x, view.area.right()),
        false => (view.area.x, text_x),
    };
    (start..end)
        .map(|x| surface[(x, view.area.y + y)].symbol.as_str())
        .collect()
}

fn text_row(app: &Application, surface: &Buffer, y: u16) -> String {
    row_cells(app, surface, y, true)
}

fn gutter_row(app: &Application, surface: &Buffer, y: u16) -> String {
    row_cells(app, surface, y, false)
}

/// The cell at text column `x` of row `y` in the focused view.
fn text_cell<'a>(
    app: &Application,
    surface: &'a Buffer,
    x: usize,
    y: u16,
) -> &'a tui::buffer::Cell {
    let (view, doc) = current_ref!(app.editor);
    let text_x = view.area.x + view.gutter_offset(doc);
    &surface[(text_x + x as u16, view.area.y + y)]
}

fn register(app: &Application, name: char) -> Vec<String> {
    app.editor
        .registers
        .read(name, &app.editor)
        .map(|values| values.map(String::from).collect())
        .unwrap_or_default()
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
                    assert!(current_ref!(app.editor).0.inline_diff.enabled);
                    // "old two" takes the row between line 0 and line 1.
                    assert_eq!(line_row(app, 1), 2);
                    assert_eq!(line_row(app, 2), 3);
                }),
            ),
            (
                Some("<C-d>"),
                Some(&|app| {
                    assert!(!current_ref!(app.editor).0.inline_diff.enabled);
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
                        assert_eq!(view.inline_diff.enabled, focused);
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
            assert!(current_ref!(app.editor).0.inline_diff.enabled);
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

#[tokio::test(flavor = "multi_thread")]
async fn removed_rows_soft_wrap() -> anyhow::Result<()> {
    let mut app = app_with(TEXT, |config| config.editor.soft_wrap.enable = Some(true))?;
    let width = {
        let (view, doc) = current_ref!(app.editor);
        view.inner_width(doc) as usize
    };
    // One long word breaks at the view's edge: a full row, a row after the
    // wrap indicator, and the rest.
    let long = "x".repeat(2 * width + 5);
    set_diff_base(&mut app, &format!("line one\n{long}\nline three\n")).await;

    test_key_sequence(
        &mut app,
        Some("<C-d>"),
        Some(&|app| {
            assert_eq!(line_row(app, 1), 4);
            assert_eq!(line_row(app, 2), 5);

            let surface = render(app);
            let row = |y| text_row(app, &surface, y);
            assert_eq!(row(1).trim_end(), "x".repeat(width));
            assert_eq!(row(2).trim_end(), format!("↪ {}", "x".repeat(width - 2)));
            assert_eq!(row(3).trim_end(), format!("↪ {}", "x".repeat(7)));
            assert_eq!(row(4).trim_end(), "line two");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn diff_yank_base_yanks_the_change_under_the_cursor() -> anyhow::Result<()> {
    let mut app = app()?;
    set_diff_base(&mut app, BASE).await;

    test_key_sequences(
        &mut app,
        vec![
            (
                Some("j:diff-yank-base<ret>"),
                Some(&|app| {
                    assert_status_not_error(&app.editor);
                    assert_eq!(register(app, '"'), ["old two\n"]);
                }),
            ),
            (
                Some(":diff-yank-base a<ret>"),
                Some(&|app| assert_eq!(register(app, 'a'), ["old two\n"])),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn diff_yank_base_needs_a_change_under_a_selection() -> anyhow::Result<()> {
    let mut app = app()?;
    set_diff_base(&mut app, BASE).await;

    test_key_sequence(
        &mut app,
        Some(":diff-yank-base<ret>"),
        Some(&|app| {
            let (status, severity) = app.editor.get_status().expect("a status message");
            assert_eq!(*severity, helix_core::diagnostic::Severity::Error);
            assert!(status.contains("no changes"), "{status}");
            assert!(register(app, '"').is_empty());
        }),
        false,
    )
    .await
}

const RUST_TEXT: &str = "#[f|]#n main() {\n    let width = area.width.saturating_sub(2);\n}\n";
const RUST_BASE: &str = "fn main() {\n    let width = area.width - 2;\n}\n";

const THEME: &str = r##"
"ui.selection" = { bg = "#202020" }
"keyword" = "red"
"diff.minus.view" = { bg = "#300000" }
"diff.delta.view" = { bg = "#000030" }
"diff.delta.view.text" = { bg = "#0000ff" }
"##;

#[tokio::test(flavor = "multi_thread")]
async fn removed_rows_get_syntax_and_word_highlights() -> anyhow::Result<()> {
    let mut app = app_with(RUST_TEXT, |_| {})?;
    app.editor
        .set_theme(Theme::from(toml::from_str::<toml::Value>(THEME)?))?;
    let loader = app.editor.syn_loader.load_full();
    doc_mut!(app.editor).set_language_by_language_id("rust", &loader)?;
    set_diff_base(&mut app, RUST_BASE).await;
    view_mut!(app.editor).inline_diff.enabled = true;

    let removed = Color::Rgb(0x30, 0, 0);
    let modified = Color::Rgb(0, 0, 0x30);
    let changed = Color::Rgb(0, 0, 0xff);

    // The base parses in the background. Rendering starts the parse.
    let mut surface = render(&app);
    for _ in 0..200 {
        if text_cell(&app, &surface, 4, 1).fg == Color::Red {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        surface = render(&app);
    }

    let base_row = text_row(&app, &surface, 1);
    assert_eq!(base_row.trim_end(), "    let width = area.width - 2;");
    let gutter = gutter_row(&app, &surface, 1);
    assert!(gutter.contains('2') && gutter.ends_with('-'), "{gutter:?}");
    let new_row = text_row(&app, &surface, 2);
    assert_eq!(
        new_row.trim_end(),
        "    let width = area.width.saturating_sub(2);"
    );

    let cell = |x, y| text_cell(&app, &surface, x, y);
    // `let` on the removed row: syntax color on the removed background.
    assert_eq!((cell(4, 1).fg, cell(4, 1).bg), (Color::Red, removed));
    // The removed `-` and the added `.saturating_sub(` are changed words.
    let minus = base_row.find('-').unwrap();
    assert_eq!(cell(minus, 1).bg, changed);
    assert_eq!(cell(minus - 1, 1).bg, removed);
    let added = new_row.find(".saturating_sub(").unwrap();
    for x in added..added + ".saturating_sub(".len() {
        assert_eq!(cell(x, 2).bg, changed, "column {x}");
    }
    // The rest of the new line keeps the modified tint and its syntax color.
    assert_eq!((cell(4, 2).fg, cell(4, 2).bg), (Color::Red, modified));

    test_key_sequence(&mut app, None, None, false).await
}
