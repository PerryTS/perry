//! Shared rendering for HIR lowering errors (#5249).
//!
//! `perry check` and `perry compile` both surface lowering failures, but
//! historically only `check` resolved the captured span into a
//! `file:line:column` diagnostic — `compile` printed the bare anyhow message
//! with no location, which made any lowering wall on large/minified/generated
//! input effectively undiagnosable.
//!
//! A [`perry_hir::error::LowerError`] carries its span as a file-relative
//! SWC span with no file identity, so the span can only be resolved where the
//! offending module's source text is in scope. This module hosts the single
//! downcast-and-build helper both front-ends share, plus a `compile`-side
//! renderer that emits the same `error[CODE] --> file:line:col` + snippet
//! layout `check` produces.

use perry_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEmitter, FileId, SourceCache, Span, TerminalEmitter,
};
use std::io::IsTerminal;

/// Build an error `Diagnostic` from a lowering failure.
///
/// When the error is a [`perry_hir::error::LowerError`] carrying a span, the
/// span is resolved against `file_id` so the emitter can print
/// `file:line:column` and the offending snippet. A span-less `LowerError` (or
/// any other error) falls back to a location-less message, prefixed with
/// `filename` so the user at least knows which file produced it.
pub fn lower_error_to_diagnostic(
    err: &anyhow::Error,
    file_id: FileId,
    filename: &str,
) -> Diagnostic {
    let (message, span) =
        if let Some(lower_err) = err.downcast_ref::<perry_hir::error::LowerError>() {
            let span = match lower_err.span {
                Some(swc_span) => Span::new(file_id, swc_span.lo.0, swc_span.hi.0),
                None => Span::DUMMY,
            };
            (lower_err.message.clone(), span)
        } else {
            // No span info — prefix the filename so the user at least knows
            // which file produced the error.
            (format!("{}: {}", filename, err), Span::DUMMY)
        };

    let mut builder = Diagnostic::error(DiagnosticCode::UnsupportedFeature, message);
    if !span.is_dummy() {
        builder = builder.with_span(span);
    }
    builder.build()
}

/// Render a lowering error as a full terminal diagnostic string for the
/// `compile` path (#5249).
///
/// `compile` lowers each module where only that module's `source` text is in
/// scope, and the `LowerError` span is file-relative but carries no file
/// identity — so rendering must happen at the lowering site rather than at
/// the top-level error sink. Builds a one-file [`SourceCache`], resolves the
/// span, and emits the same `error[CODE] --> file:line:col` + snippet layout
/// as `perry check`.
///
/// Returns `None` when the error carries no span — letting the caller keep
/// the bare-message behaviour for failures that gain nothing from the snippet
/// machinery.
///
/// `source` must be the exact text the module's AST was parsed from (post any
/// CJS-wrap / require-rewrite transforms), so the span's byte offsets line up.
pub fn render_compile_lower_error(
    err: &anyhow::Error,
    filename: &str,
    source: &str,
) -> Option<String> {
    let lower_err = err.downcast_ref::<perry_hir::error::LowerError>()?;
    // Only take over rendering when there's a span to point at.
    lower_err.span?;

    let mut cache = SourceCache::new();
    let file_id = cache.add_file(filename, source.to_string());
    let diag = lower_error_to_diagnostic(err, file_id, filename);

    // The rendered string is returned as the error message, which the top-
    // level `main` Termination prints to stderr — colorize to match `check`
    // when stderr is a terminal and `NO_COLOR` isn't set.
    let use_color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut buf: Vec<u8> = Vec::new();
    let mut emitter = TerminalEmitter::new(&mut buf, use_color);
    emitter.emit(&diag, &cache).ok()?;
    Some(String::from_utf8_lossy(&buf).trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::error::LowerError;
    use swc_common::{BytePos, Span as SwcSpan};

    fn span(lo: u32, hi: u32) -> SwcSpan {
        SwcSpan::new(BytePos(lo), BytePos(hi))
    }

    #[test]
    fn render_compile_lower_error_emits_location_and_snippet() {
        // Mirror the #5249 repro: a span-tagged lowering failure should
        // resolve to `error[U006] --> file:line:col` plus the offending line.
        let source = "for (var { a, b } = { a: 1, b: 2 }, i = 0; i < 1; i++) {}\n";
        // SWC BytePos is 1-based for a fresh single-file source map, so the
        // `{` of `{ a, b }` (byte 9) lands at BytePos 10.
        let err = anyhow::Error::new(LowerError::new("Unsupported binding pattern", span(10, 18)));

        let rendered =
            render_compile_lower_error(&err, "t.ts", source).expect("span-tagged error renders");

        assert!(
            rendered.contains("error[U006]: Unsupported binding pattern"),
            "missing header: {rendered}"
        );
        assert!(rendered.contains("t.ts:1:11"), "missing location: {rendered}");
        assert!(rendered.contains("for (var { a, b }"), "missing snippet: {rendered}");
        assert!(rendered.contains('^'), "missing underline: {rendered}");
    }

    #[test]
    fn render_compile_lower_error_skips_spanless_errors() {
        // A span-less LowerError gains nothing from the snippet machinery —
        // the caller should fall back to the bare message.
        let err = anyhow::Error::new(LowerError::without_span("no location here"));
        assert!(render_compile_lower_error(&err, "t.ts", "x;\n").is_none());
    }

    #[test]
    fn render_compile_lower_error_skips_non_lower_errors() {
        let err = anyhow::anyhow!("some other failure");
        assert!(render_compile_lower_error(&err, "t.ts", "x;\n").is_none());
    }

    #[test]
    fn lower_error_to_diagnostic_falls_back_to_filename_prefixed_message() {
        // Non-LowerError → location-less, filename-prefixed message, no span.
        let err = anyhow::anyhow!("boom");
        let diag = lower_error_to_diagnostic(&err, FileId(0), "mod.ts");
        assert_eq!(diag.message, "mod.ts: boom");
        assert!(diag.span.is_dummy());
    }
}
