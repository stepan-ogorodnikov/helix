use helix_core::command_line::Args;
use helix_core::Rope;

use crate::compositor;
use crate::ui::PromptEvent;

/// Format every diagnostic as one compiler-style report.
///
/// A record looks like `path:line:col: severity[code] (source): message`.
/// Line and column are 1-based. `[code]` and `(source)` are omitted when the
/// server did not send them. Newlines inside the message are kept, and a
/// blank line separates records, so a paste stays readable when a message
/// itself contains newlines.
fn format_diagnostics_report(
    path: &str,
    text: &Rope,
    diagnostics: &[helix_core::diagnostic::Diagnostic],
) -> String {
    let mut out = String::new();
    for (i, diagnostic) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_diagnostic_record(path, text, diagnostic));
        out.push('\n');
    }
    out
}

fn format_diagnostic_record(
    path: &str,
    text: &Rope,
    diagnostic: &helix_core::diagnostic::Diagnostic,
) -> String {
    use helix_core::diagnostic::NumberOrString;

    let pos = diagnostic.range.start.min(text.len_chars());
    let line = text.char_to_line(pos);
    let column = pos - text.line_to_char(line);

    let mut record = format!(
        "{path}:{}:{}: {}",
        line + 1,
        column + 1,
        severity_name(diagnostic.severity())
    );
    if let Some(code) = &diagnostic.code {
        record.push('[');
        match code {
            NumberOrString::Number(number) => record.push_str(&number.to_string()),
            NumberOrString::String(code) => record.push_str(code),
        }
        record.push(']');
    }
    if let Some(source) = diagnostic.source.as_deref() {
        record.push_str(" (");
        record.push_str(source);
        record.push(')');
    }

    let message = diagnostic.message.trim_end_matches(['\r', '\n']);
    if !message.is_empty() {
        record.push_str(": ");
        record.push_str(message);
    }
    record
}

fn severity_name(severity: helix_core::diagnostic::Severity) -> &'static str {
    use helix_core::diagnostic::Severity;

    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

pub(super) fn yank_diagnostics(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }

    let reg = super::yank_register(&args)?;
    let doc = doc!(cx.editor);
    let diagnostics = doc.diagnostics();
    let n = diagnostics.len();
    if n == 0 {
        anyhow::bail!("No diagnostics in the current buffer");
    }

    let path = doc.display_name();
    let report = format_diagnostics_report(&path, doc.text(), diagnostics);
    cx.editor.registers.write(reg, vec![report])?;
    cx.editor.set_status(format!(
        "Yanked {n} diagnostic{} to register {reg}",
        if n == 1 { "" } else { "s" }
    ));
    Ok(())
}

#[cfg(test)]
mod test {
    use helix_core::diagnostic::{
        Diagnostic, DiagnosticProvider, LanguageServerId, NumberOrString, Severity,
    };
    use helix_core::Rope;
    use slotmap::Key;

    use super::format_diagnostics_report;

    fn diagnostic(
        start: usize,
        end: usize,
        severity: impl Into<Option<Severity>>,
        code: Option<NumberOrString>,
        source: Option<&str>,
        message: &str,
    ) -> Diagnostic {
        Diagnostic {
            range: helix_core::diagnostic::Range { start, end },
            ends_at_word: false,
            starts_at_word: false,
            zero_width: start == end,
            line: 0,
            message: message.into(),
            severity: severity.into(),
            code,
            provider: DiagnosticProvider::Lsp {
                server_id: LanguageServerId::null(),
                identifier: None,
            },
            tags: Vec::new(),
            source: source.map(ToOwned::to_owned),
            data: None,
        }
    }

    #[test]
    fn formats_diagnostics_as_a_compiler_report() {
        // "café\n    let x = 1;\n"
        // é is char 3 (line 1, col 4). x is char 13 (line 2, col 9).
        let text = Rope::from("café\n    let x = 1;\n");
        let report = format_diagnostics_report(
            "src/main.rs",
            &text,
            &[
                diagnostic(
                    0,
                    1,
                    Severity::Info,
                    None,
                    Some("eslint"),
                    "missing semicolon",
                ),
                diagnostic(
                    3,
                    4,
                    Severity::Error,
                    Some(NumberOrString::String("E0308".into())),
                    Some("rustc"),
                    "mismatched types\nexpected `i32`, found `&str`\n",
                ),
                diagnostic(
                    13,
                    14,
                    Severity::Warning,
                    Some(NumberOrString::String("unused_variable".into())),
                    Some("clippy"),
                    "unused variable `x`",
                ),
            ],
        );

        assert_eq!(
            report,
            "\
src/main.rs:1:1: info (eslint): missing semicolon

src/main.rs:1:4: error[E0308] (rustc): mismatched types
expected `i32`, found `&str`

src/main.rs:2:9: warning[unused_variable] (clippy): unused variable `x`
"
        );
    }

    #[test]
    fn formats_missing_severity_code_and_clamps_the_position() {
        let text = Rope::from("x");
        let report = format_diagnostics_report(
            "[scratch]",
            &text,
            &[
                diagnostic(0, 1, None, Some(NumberOrString::Number(38)), None, ""),
                diagnostic(100, 100, Severity::Hint, None, None, "beyond the buffer"),
            ],
        );

        assert_eq!(
            report,
            "\
[scratch]:1:1: warning[38]

[scratch]:1:2: hint: beyond the buffer
"
        );
    }
}
