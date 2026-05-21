//! CommonJS-vs-ESM heuristic detection plus reserved-word filtering.

#[allow(unused_imports)]
use super::*;

/// Heuristic CJS detection. Same shape as
/// `perry-jsruntime/src/modules.rs::is_commonjs`. False negatives are
/// acceptable (the file just falls through to the existing ESM-only
/// pipeline); false positives on a real ESM file would be more painful but
/// require a file that uses neither `module.exports` nor `exports.` nor
/// `require(` — i.e., an ESM file that *also* contains those tokens. Real
/// hybrid cases are rare and would need a `"type": "module"` package.json
/// override, which is the next refinement if this trips a real package.
///
/// Issue #851: Rollup-bundled output (the `vitest/dist/chunks/*.js` shape)
/// has top-level ESM `import`/`export` statements AND inlined CJS bodies
/// (`module.exports = factory()`) deep inside nested IIFE helpers. Such
/// files are unambiguously ESM — the inner CJS tokens are just identifiers
/// inside function bodies. If we wrap them as CJS, the wrap moves the
/// top-level `import`/`export` *inside* the IIFE body and SWC errors with
/// `ImportExportInScript`. The guard below short-circuits the wrap when a
/// top-level `import`/`export` statement is detected.
pub(in crate::commands::compile) fn is_commonjs(source: &str) -> bool {
    // ESM-at-the-top wins: a top-level `import`/`export` makes this an
    // ES module regardless of CJS patterns appearing deeper in the file.
    if has_top_level_esm(source) {
        return false;
    }
    source.contains("module.exports")
        || source.contains("exports.")
        || (source.contains("require(") && !source.contains("import "))
}

/// Returns true if `source` contains an unindented `import ` / `import{` /
/// `import"` / `import'` / `export ` / `export{` / `export*` / `export"` /
/// `export'` / `export=` (TS) statement on any line — a strong signal that
/// this file is an ES module regardless of any `module.exports`-style
/// content deeper in nested function bodies. Lines starting with leading
/// whitespace are treated as nested and ignored, because `import` /
/// `export` statements MUST be at module-top-level in ECMAScript. Comment
/// and string-literal contexts are not stripped — a `// import ` line is
/// already excluded by the leading-whitespace filter when indented; an
/// inline `/* import x */` followed by a real statement still triggers a
/// match on the real statement line. Worst case is a false positive on a
/// pathological file where the only top-level `import`/`export` lives
/// inside a multi-line string literal at column 0; we accept that risk
/// since the alternative is `ImportExportInScript` on real Rollup output.
pub fn has_top_level_esm(source: &str) -> bool {
    for raw_line in source.lines() {
        // Skip indented lines — `import`/`export` statements are only
        // valid at module top-level, so any indented occurrence is
        // either inside a function body, a comment, or a string.
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            continue;
        }
        let line = raw_line.trim_start();
        if starts_with_esm_keyword(line, "import") || starts_with_esm_keyword(line, "export") {
            return true;
        }
    }
    false
}

/// Returns true if `line` starts with `keyword` followed by a character
/// that can legally begin an `import`/`export` statement's continuation:
/// space, `{`, `*` (export only), `"`, `'`, or `(` (dynamic import). We
/// reject identifier-continuation characters (a-z, A-Z, 0-9, `_`, `$`) so
/// e.g. `exports.foo = …` does NOT match `export`, and `importMap = …`
/// does NOT match `import`.
pub fn starts_with_esm_keyword(line: &str, keyword: &str) -> bool {
    if let Some(rest) = line.strip_prefix(keyword) {
        match rest.chars().next() {
            None => false,
            Some(c) => {
                // Reject identifier-continuation: this is a different word
                // (`exports`, `importMap`, etc.), not the keyword.
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    return false;
                }
                // Whitespace, `{`, `*`, `"`, `'`, `(` all legally follow
                // `import` or `export` — accept.
                matches!(c, ' ' | '\t' | '{' | '*' | '"' | '\'' | '(')
            }
        }
    } else {
        false
    }
}

/// JS reserved words that cannot be used as binding identifiers (e.g.
/// in `const X = ...`). Used by `extract_exports_from_source` to skip
/// CJS-style `module.exports.X = ...` patterns where `X` is a keyword —
/// emitting `export const <keyword> = _cjs.<keyword>;` would fail to
/// parse. `default` (pino's `module.exports.default = pino` interop
/// pattern) is the common real-world case; the rest are filtered
/// defensively. Contextual keywords (`async`, `arguments`, `eval`, `as`,
/// `from`, `of`) are legal identifiers and not included.
pub fn is_js_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "let"
            | "static"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "await"
    )
}
