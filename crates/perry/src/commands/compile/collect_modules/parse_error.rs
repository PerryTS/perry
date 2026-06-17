//! CJS-wrap parse-error annotation.
//!
//! Extracted from `collect_modules.rs` (file-size split). When a
//! `compilePackages` target fails to parse after Perry's CJS-to-ESM
//! wrap, these helpers append a note explaining that error offsets
//! refer to the post-wrap source and render a small source excerpt
//! around the offending byte offset.

/// Issue #845: when SWC fails to parse a CJS-wrapped source, the byte
/// offset in the error refers to the wrap output, not the on-disk file
/// — so the offset is past EOF of the original. Rewrite the message to
/// say so, and (when we can parse a `(lo..hi, ...)` span out of SWC's
/// Debug-formatted error) include an excerpt of the wrap output around
/// `lo` so the user can see what choked the re-parse. Pass-through for
/// non-wrapped sources.
pub(super) fn annotate_parse_error(
    e: anyhow::Error,
    path: &std::path::Path,
    parsed_source: &str,
    was_cjs_wrapped: bool,
) -> anyhow::Error {
    if !was_cjs_wrapped {
        return e;
    }
    let msg = format!("{}", e);
    let span_re = regex::Regex::new(r"\((\d+)\.\.(\d+),").ok();
    let offset = span_re
        .as_ref()
        .and_then(|re| re.captures(&msg))
        .and_then(|cap| cap.get(1)?.as_str().parse::<usize>().ok());
    let excerpt = offset.and_then(|lo| excerpt_around_offset(parsed_source, lo));

    let mut extra = format!(
        "\nnote: this file is inside a `compilePackages` target and was rewritten by Perry's CJS-to-ESM wrap before parsing. The error offset above refers to the post-wrap source ({} bytes), NOT the {}-byte file on disk. Re-run with `PERRY_DEBUG_CJS_WRAP=1` to see the full wrap output.",
        parsed_source.len(),
        std::fs::metadata(path)
            .map(|m| m.len().to_string())
            .unwrap_or_else(|_| "original".to_string()),
    );
    if let Some(snippet) = excerpt {
        extra.push_str("\nwrap-output excerpt around the error offset:\n");
        extra.push_str(&snippet);
    }
    anyhow::anyhow!("{}{}", msg, extra)
}

/// Render up to 2 lines of context on either side of the byte offset
/// `lo`, with the offending line highlighted by a `>>>` prefix. Returns
/// `None` when `lo` is out of range or the source has no newlines.
fn excerpt_around_offset(source: &str, lo: usize) -> Option<String> {
    let lo = lo.min(source.len().saturating_sub(1));
    let line_start = source[..lo].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[lo..]
        .find('\n')
        .map(|i| lo + i)
        .unwrap_or(source.len());
    let pre_line = (0..2).fold(line_start, |acc, _| {
        source[..acc.saturating_sub(1)]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    });
    let post_line = (0..2).fold(line_end, |acc, _| {
        source
            .get(acc + 1..)
            .and_then(|s| s.find('\n').map(|i| acc + 1 + i))
            .unwrap_or(source.len())
    });
    let line_number_at = |off: usize| source[..off].matches('\n').count() + 1;
    let mut out = String::new();
    let mut cursor = pre_line;
    while cursor < post_line {
        let next = source[cursor..]
            .find('\n')
            .map(|i| cursor + i)
            .unwrap_or(post_line);
        let line = &source[cursor..next];
        let marker = if cursor <= lo && lo <= next {
            ">>>"
        } else {
            "   "
        };
        out.push_str(&format!(
            "{} {:>5} | {}\n",
            marker,
            line_number_at(cursor),
            line
        ));
        if next >= post_line {
            break;
        }
        cursor = next + 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
