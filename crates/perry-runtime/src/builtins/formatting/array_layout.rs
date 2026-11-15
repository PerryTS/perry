//! Node's `util.inspect` layout of an array body and of a string value:
//! `groupArrayElements`, the array arm of `reduceToSingleString` with
//! `isBelowBreakLength`, and the long-string split of `formatPrimitive`
//! (node `lib/internal/util/inspect.js`, default options: `breakLength` 80,
//! `compact` 3).
//!
//! Perry's formatters build a nested value's text with its continuation
//! lines indented relative to the value itself and let each enclosing
//! container add two spaces per level. Node's text carries the absolute
//! indentation. Where node measures a length, the relative text is measured
//! with the indentation it will be printed with added back (`node_len`), so
//! the decisions are node's.

/// node `inspectDefaultOptions.breakLength`.
pub(crate) const BREAK_LENGTH: usize = 80;
/// node `inspectDefaultOptions.compact`.
const COMPACT: usize = 3;
/// node `kMinLineLength`: shorter strings are never split.
const MIN_LINE_LENGTH: usize = 16;
/// One space and one comma between two entries.
const SEPARATOR_SPACE: usize = 2;

/// node `ctx.indentationLvl` of a value at nesting `depth`: every array
/// element and object property adds two.
pub(crate) fn indentation_for_depth(depth: usize) -> usize {
    depth * 2
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// node `getStringWidth(str)` for uncoloured text: C0 controls take no
/// column, every other character one. (Node's ICU east-asian width is not
/// modelled: a full-width character counts as one column here.)
fn string_width(s: &str) -> usize {
    s.chars().filter(|c| (*c as u32) >= 32).count()
}

/// The `.length` node sees for an entry whose continuation lines will be
/// printed `child_indent` columns further right than they are in `text`.
fn node_len(text: &str, child_indent: usize) -> usize {
    utf16_len(text) + text.matches('\n').count() * child_indent
}

fn node_width(text: &str, child_indent: usize) -> usize {
    string_width(text) + text.matches('\n').count() * child_indent
}

/// node `isBelowBreakLength` with `base === ''`.
fn is_below_break_length(output: &[String], start: usize, child_indent: usize) -> bool {
    let mut total = output.len() + start;
    if total + output.len() > BREAK_LENGTH {
        return false;
    }
    for text in output {
        total += node_len(text, child_indent);
        if total > BREAK_LENGTH {
            return false;
        }
    }
    true
}

/// node `groupArrayElements`: lay more than six short entries out in
/// columns. `pad_start` is node's `order`: right-align when every value is a
/// number or bigint, left-align otherwise. `None` when node keeps the
/// entries as they are.
fn group_array_elements(
    output: &[String],
    indentation: usize,
    pad_start: bool,
) -> Option<Vec<String>> {
    let child_indent = indentation + 2;
    let data_len: Vec<usize> = output.iter().map(|t| node_width(t, child_indent)).collect();
    let total_length: usize = data_len.iter().map(|len| len + SEPARATOR_SPACE).sum();
    let max_length = data_len.iter().copied().max().unwrap_or(0);
    let actual_max = max_length + SEPARATOR_SPACE;
    // At least three entries fit next to each other, and no entry is longer
    // than a fifth of all of them together.
    if !(actual_max * 3 + indentation < BREAK_LENGTH
        && (total_length as f64 / actual_max as f64 > 5.0 || max_length <= 6))
    {
        return None;
    }
    let approx_char_heights = 2.5_f64;
    let average_bias = (actual_max as f64 - total_length as f64 / output.len() as f64).sqrt();
    let biased_max = (actual_max as f64 - 3.0 - average_bias).max(1.0);
    let columns = [
        ((approx_char_heights * biased_max * output.len() as f64).sqrt() / biased_max).round()
            as usize,
        (BREAK_LENGTH - indentation) / actual_max,
        COMPACT * 4,
        15,
    ]
    .into_iter()
    .min()
    .unwrap_or(1);
    if columns <= 1 {
        return None;
    }
    let max_line_length: Vec<usize> = (0..columns)
        .map(|column| {
            data_len
                .iter()
                .skip(column)
                .step_by(columns)
                .copied()
                .max()
                .unwrap_or(0)
                + SEPARATOR_SPACE
        })
        .collect();
    let pad = |text: &str, target: usize| -> String {
        let fill = " ".repeat(target.saturating_sub(utf16_len(text)));
        if pad_start {
            format!("{fill}{text}")
        } else {
            format!("{text}{fill}")
        }
    };
    let mut rows = Vec::with_capacity(output.len().div_ceil(columns));
    for row_start in (0..output.len()).step_by(columns) {
        let row_end = (row_start + columns).min(output.len());
        let mut row = String::new();
        for j in row_start..row_end - 1 {
            // node pads to a `.length`; the width/length difference of
            // non-ASCII text is added back the way node adds colour codes.
            let target = max_line_length[j - row_start] + utf16_len(&output[j]) - data_len[j];
            row.push_str(&pad(&format!("{}, ", output[j]), target));
        }
        let last = row_end - 1;
        if pad_start {
            let target = max_line_length[last - row_start] + utf16_len(&output[last])
                - data_len[last]
                - SEPARATOR_SPACE;
            row.push_str(&pad(&output[last], target));
        } else {
            row.push_str(&output[last]);
        }
        rows.push(row);
    }
    Some(rows)
}

/// Each entry on its own line; continuation lines move two columns right
/// with the entry.
fn one_entry_per_line(output: &[String]) -> String {
    let body = output
        .iter()
        .map(|text| format!("  {}", text.replace('\n', "\n  ")))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("[\n{body}\n]")
}

/// node `reduceToSingleString` for an array (braces `[`/`]`, no base) at
/// nesting `depth`. `pad_start`: every element is a number or bigint.
/// `compact` false is node's `compact: false`: one entry per line.
///
/// node also keeps a body off one line when the value nests `compact` (3)
/// or more objects below it (`ctx.currentDepth - recurseTimes`); with the
/// default depth of 2 that cannot happen, and Perry's formatters do not track
/// it.
pub(crate) fn reduce_array_entries(
    entries: Vec<String>,
    depth: usize,
    pad_start: bool,
    compact: bool,
) -> String {
    if entries.is_empty() {
        return "[]".to_string();
    }
    if !compact {
        return one_entry_per_line(&entries);
    }
    let indentation = indentation_for_depth(depth);
    let entry_count = entries.len();
    let mut output = entries;
    if entry_count > 6 {
        if let Some(grouped) = group_array_elements(&output, indentation, pad_start) {
            output = grouped;
        }
    }
    if output.len() == entry_count {
        // `[`.length + 10, node's allowance for what else shares the line.
        let start = output.len() + indentation + 1 + 10;
        if is_below_break_length(&output, start, indentation + 2) {
            let joined = output.join(", ");
            if !joined.contains('\n') {
                return format!("[ {joined} ]");
            }
        }
    }
    one_entry_per_line(&output)
}

/// node `strEscape` + `addQuotes`: single quotes unless the text holds one;
/// then double quotes, or backticks when it also holds a double quote.
pub(crate) fn quote_inspect_string(s: &str) -> String {
    let mut quote = '\'';
    if s.contains('\'') {
        if !s.contains('"') {
            quote = '"';
        } else if !s.contains('`') && !s.contains("${") {
            quote = '`';
        }
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        let code = c as u32;
        match c {
            '\'' if quote == '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            _ if code < 0x20 || (0x7f..0xa0).contains(&code) => {
                out.push_str(&format!("\\x{code:02X}"));
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A string as an inspected value at nesting `depth` (node `formatPrimitive`):
/// a string longer than 16 that does not fit the line is printed as one
/// quoted piece per source line, joined with ` +`.
pub(crate) fn inspect_string_value(s: &str, depth: usize) -> String {
    let len = utf16_len(s);
    let indentation = indentation_for_depth(depth);
    if len > MIN_LINE_LENGTH && len + indentation + 4 > BREAK_LENGTH {
        return s
            .split_inclusive('\n')
            .map(quote_inspect_string)
            .collect::<Vec<_>>()
            .join(" +\n  ");
    }
    quote_inspect_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn short_numbers_group_into_right_aligned_columns() {
        let numbers: Vec<String> = (0..30).map(|i| (i * 1000).to_string()).collect();
        let body = reduce_array_entries(numbers, 0, true, true);
        assert!(
            body.starts_with("[\n      0,  1000,  2000,  3000,  4000,\n   5000,"),
            "{body}"
        );
        assert!(
            body.ends_with("\n  25000, 26000, 27000, 28000, 29000\n]"),
            "{body}"
        );
    }

    #[test]
    fn long_strings_print_one_per_line() {
        let entries = texts(&[
            "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
            "'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'",
            "'c'",
            "'d'",
            "'e'",
            "'f'",
            "'g'",
        ]);
        let body = reduce_array_entries(entries, 0, false, true);
        assert_eq!(body.lines().count(), 9, "{body}");
    }

    #[test]
    fn six_entries_never_group_and_fit_on_one_line() {
        let entries = texts(&["1", "2", "3", "4", "5", "6"]);
        assert_eq!(
            reduce_array_entries(entries, 0, true, true),
            "[ 1, 2, 3, 4, 5, 6 ]"
        );
        assert_eq!(reduce_array_entries(Vec::new(), 0, true, true), "[]");
    }

    #[test]
    fn mixed_short_entries_group_left_aligned() {
        let entries = texts(&["1", "'a'", "true", "null", "2", "'bb'", "3", "'c'"]);
        let body = reduce_array_entries(entries, 0, false, true);
        assert_eq!(
            body,
            "[\n  1,    'a', true,\n  null, 2,   'bb',\n  3,    'c'\n]"
        );
    }

    #[test]
    fn compact_false_is_one_entry_per_line() {
        let entries = texts(&["1", "2"]);
        assert_eq!(
            reduce_array_entries(entries, 0, true, false),
            "[\n  1,\n  2\n]"
        );
    }

    #[test]
    fn quotes_follow_node() {
        assert_eq!(quote_inspect_string("a"), "'a'");
        assert_eq!(quote_inspect_string("it's"), "\"it's\"");
        assert_eq!(quote_inspect_string("it's \"x\""), "`it's \"x\"`");
        assert_eq!(quote_inspect_string("a\nb\u{1}"), "'a\\nb\\x01'");
    }

    #[test]
    fn long_multi_line_strings_split_after_each_newline() {
        let s = format!("{}\n{}", "x".repeat(50), "y".repeat(40));
        assert_eq!(
            inspect_string_value(&s, 1),
            format!("'{}\\n' +\n  '{}'", "x".repeat(50), "y".repeat(40))
        );
        assert_eq!(inspect_string_value("short\nline", 1), "'short\\nline'");
    }
}
