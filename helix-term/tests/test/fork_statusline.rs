//! FORK: align-message. Where the status message is drawn.

use helix_term::application::Application;

use super::*;

fn message_row(app: &Application) -> String {
    let buffer = app.terminal_buffer();
    let y = buffer.area.bottom() - 1;
    (0..8)
        .map(|x| buffer.get(x, y).unwrap().symbol.to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn status_message_starts_under_the_statusline_text() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":echo hello<ret>"),
        Some(&|app| assert_eq!(" hello  ", message_row(app))),
        false,
    )
    .await
}
