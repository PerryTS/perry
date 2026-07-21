//! CommonJS-vs-ESM heuristic detection plus reserved-word filtering.

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
    // An empty (or whitespace-only) file is a valid CJS module exporting
    // `{}` — marker packages like react's `client-only` ship a 0-byte
    // index.js whose default import must resolve to the empty exports
    // object, which only the wrap provides.
    if source.trim().is_empty() {
        return true;
    }
    // ALL token scans run on comment/string-stripped source. Real packages
    // defeat raw-text scans in both directions: Next.js's
    // `setup-node-env.external.js` has the word "import " in a header
    // comment (which flipped the `require(` arm), and `next/dist/build/
    // utils.js` GENERATES an ESM server.js inside a template literal whose
    // column-0 `import path from 'node:path'` line made `has_top_level_esm`
    // classify the (thoroughly CJS) file as ESM — its bare `exports` then
    // threw a ReferenceError at module init.
    let stripped = strip_comments_and_strings(source);
    // ESM-at-the-top wins: a top-level `import`/`export` makes this an
    // ES module regardless of CJS patterns appearing deeper in the file.
    if has_top_level_esm(&stripped) {
        return false;
    }
    if stripped.contains("module.exports")
        || stripped.contains("exports.")
        // Issue #5275: bracket / computed-string-literal CJS export forms —
        // `module['exports'] = …` / `module["exports"] = …` (default) and
        // `exports['name'] = …` / `exports["name"] = …` (named). These are
        // semantically identical to the dot forms (@colors/colors's
        // `lib/custom/trap.js` does `module['exports'] = function runTheTrap`).
        // Without recognizing them the file falls through to the ESM pipeline
        // and the bare `module`/`exports` identifiers throw at module init.
        //
        // NOTE: `strip_comments_and_strings` blanks the `'exports'` STRING
        // CONTENT (and its quotes) to spaces, leaving `module[          ]`, so
        // we can't match the quoted token against `stripped`. Scan for the
        // bracket-export shape on the original source via a regex that allows
        // whitespace where the stripper would have written spaces. A genuinely
        // dynamic `module[k]` (non-string-literal key) does NOT match.
        || has_bracket_cjs_export(source)
        // Issue #4872: tsc-compiled type-only modules (nestjs dist
        // `*.interface.js`) contain ONLY the interop marker
        // `Object.defineProperty(exports, "__esModule", { value: true });`
        // — no `exports.X =`, no `require(`. Without this arm they fall
        // through to the ESM pipeline, where the bare `exports` identifier
        // throws a ReferenceError at module init.
        || stripped.contains("defineProperty(exports,")
    {
        return true;
    }
    stripped.contains("require(") && !stripped.contains("import ")
}

/// Issue #5275: detect a bracket / computed-string-literal CJS export
/// assignment — `module['exports'] = …`, `module["exports"] = …`,
/// `exports['name'] = …`, `exports["name"] = …`, and the
/// `module.exports['name'] = …` variant. Requires the `=` (an assignment) so
/// a bare `module['exports']` read or a comment mention doesn't trip it, and
/// requires a string-literal key so a genuinely dynamic `module[k] = …` is
/// not matched. Matched on the ORIGINAL source because
/// `strip_comments_and_strings` blanks the quoted key.
fn has_bracket_cjs_export(source: &str) -> bool {
    // `module['exports'] = …` / `module["exports"] = …` (default export).
    let module_default = regex::Regex::new(r#"\bmodule\[\s*['"]exports['"]\s*\]\s*="#).unwrap();
    // `exports['name'] = …` / `module.exports['name'] = …` (named export).
    let named =
        regex::Regex::new(r#"\bexports\[\s*['"][A-Za-z_$][A-Za-z0-9_$]*['"]\s*\]\s*="#).unwrap();
    module_default.is_match(source) || named.is_match(source)
}

/// Replace comment bodies and string/template-literal contents with spaces
/// so token scans (`require(`, `import `) only see real code. Same scanner
/// shape as `looks_like_es_module` in perry-parser, including the
/// regex-literal tracking — a regex containing an unescaped quote (e.g.
/// `/['"]/` in vendored minified bundles like comment-json) would otherwise
/// desync the string state and mask the rest of the file, hiding a trailing
/// `module.exports = …`.
pub(crate) fn strip_comments_and_strings(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        Str(u8),
        LineComment,
        BlockComment,
    }

    fn is_ident(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }

    // A `/` starts a regex literal (not division) when the preceding token
    // cannot end an expression. Mirrors perry-parser's heuristic.
    fn regex_can_start_here(bytes: &[u8], slash_at: usize) -> bool {
        let mut i = slash_at;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b'=' | b'(' | b',' | b':' | b'[' | b'!' | b'&' | b'|' | b'?' | b'{' | b'}'
                | b';' | b'+' | b'-' | b'*' | b'%' | b'~' | b'^' | b'<' | b'>' => return true,
                c if is_ident(c) => {
                    let end = i + 1;
                    let mut start = end;
                    while start > 0 && is_ident(bytes[start - 1]) {
                        start -= 1;
                    }
                    return matches!(
                        &bytes[start..end],
                        b"return"
                            | b"typeof"
                            | b"instanceof"
                            | b"in"
                            | b"of"
                            | b"case"
                            | b"do"
                            | b"else"
                            | b"void"
                            | b"delete"
                            | b"throw"
                            | b"new"
                            | b"yield"
                            | b"await"
                    );
                }
                _ => return false,
            }
        }
        true
    }

    // Returns the index just past the closing `/`, or None if no regex
    // terminator is found on this line (then it was division after all).
    fn skip_regex_literal(bytes: &[u8], slash_at: usize) -> Option<usize> {
        let mut i = slash_at + 1;
        let mut in_class = false;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'\n' => return None,
                b'[' => {
                    in_class = true;
                    i += 1;
                }
                b']' => {
                    in_class = false;
                    i += 1;
                }
                b'/' if !in_class => return Some(i + 1),
                _ => i += 1,
            }
        }
        None
    }

    let bytes = source.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut state = State::Code;
    let mut i = 0;
    // Open `${…}` template interpolations: each entry is the `{`-nesting
    // depth inside that interpolation. The interpolation body is real code
    // (left unmasked) and may itself contain nested template literals —
    // next/dist/build/utils.js generates server.js via
    // `` `${moduleType ? `import …` : `const …`}` `` and a non-nesting
    // scanner ends the outer template at the first INNER backtick,
    // unmasking the generated `import` lines.
    let mut template_interp_depth: Vec<u32> = Vec::new();
    while i < bytes.len() {
        match state {
            State::Code => {
                if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
                    state = State::Str(bytes[i]);
                    i += 1;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::LineComment;
                    i += 2;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = State::BlockComment;
                    i += 2;
                } else if bytes[i] == b'/' && regex_can_start_here(bytes, i) {
                    // Regex literal: mask its body (it may contain quotes)
                    // but keep scanning code after it.
                    match skip_regex_literal(bytes, i) {
                        Some(end) => i = end,
                        None => {
                            out[i] = bytes[i];
                            i += 1;
                        }
                    }
                } else if bytes[i] == b'{' {
                    if let Some(depth) = template_interp_depth.last_mut() {
                        *depth += 1;
                    }
                    out[i] = bytes[i];
                    i += 1;
                } else if bytes[i] == b'}' {
                    match template_interp_depth.last_mut() {
                        Some(0) => {
                            // Close of a `${…}` — resume the template literal.
                            template_interp_depth.pop();
                            state = State::Str(b'`');
                            i += 1;
                        }
                        Some(depth) => {
                            *depth -= 1;
                            out[i] = bytes[i];
                            i += 1;
                        }
                        None => {
                            out[i] = bytes[i];
                            i += 1;
                        }
                    }
                } else {
                    out[i] = bytes[i];
                    i += 1;
                }
            }
            State::Str(quote) => {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if quote == b'`' && bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    // `${` — interpolation body is code (and may nest).
                    template_interp_depth.push(0);
                    state = State::Code;
                    i += 2;
                } else {
                    if bytes[i] == quote {
                        state = State::Code;
                    }
                    i += 1;
                }
            }
            State::LineComment => {
                if bytes[i] == b'\n' {
                    state = State::Code;
                    out[i] = b'\n';
                }
                i += 1;
            }
            State::BlockComment => {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
    }
    // SAFETY-free: `out` is pure ASCII spaces plus bytes copied verbatim
    // from `source` at their original positions, so it remains valid UTF-8
    // except where a multi-byte char was partially masked — use lossy
    // conversion to stay safe.
    String::from_utf8_lossy(&out).into_owned()
}

/// Returns true if `source` contains a top-level (bracket-depth-0) `import` /
/// `export` statement — a strong signal that this file is an ES module
/// regardless of any `module.exports`-style content deeper in nested function
/// bodies. Expects `source` to have already been run through
/// `strip_comments_and_strings`, so comment and string-literal contexts are
/// masked to spaces and never trip a false match.
///
/// We track bracket depth (`()[]{}`) and statement starts rather than scanning
/// line-by-line. A line-start scan suffices for pretty-printed bundles but
/// MISSES minified ones: issue #5498's esbuild ESM bundle (the OpenAI Codex
/// CLI) joins every top-level statement onto one giant line, so its real
/// `import{createRequire as NDe}from"module"` lands mid-line as
/// `…})();import{createRequire…}…` — never at a line start. A minifier still
/// separates statements with `;`/`}`, so a statement-aware scan at depth 0
/// finds it while continuing to ignore `import`/`export` tokens nested inside
/// function bodies (an indented dynamic `import('./x')` stays at depth ≥ 1).
///
/// Worst case is a false positive on a pathological file whose only top-level
/// `import`/`export` lives inside a string literal that `strip_comments_and_strings`
/// failed to mask (e.g. a desynced regex); we accept that risk since the
/// alternative is `ImportExportInScript` on real Rollup/esbuild output.
pub fn has_top_level_esm(source: &str) -> bool {
    fn is_ident_start(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphabetic()
    }
    fn is_ident_byte(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }

    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    // True when the next identifier read begins a statement: at start of
    // input, or just after `;`, `{`, `}`, or a newline. Whitespace and
    // comments (already masked to spaces) preserve it.
    let mut stmt_start = true;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            b'\n' | b';' => {
                stmt_start = true;
                i += 1;
            }
            b'{' => {
                depth += 1;
                stmt_start = true;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                stmt_start = true;
                i += 1;
            }
            b'(' | b'[' => {
                depth += 1;
                stmt_start = false;
                i += 1;
            }
            b')' | b']' => {
                depth -= 1;
                stmt_start = false;
                i += 1;
            }
            b if is_ident_start(b) => {
                let start = i;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                if stmt_start && depth == 0 {
                    let word = &source[start..i];
                    if (word == "import" || word == "export")
                        && starts_with_esm_keyword(&source[start..], word)
                    {
                        return true;
                    }
                }
                stmt_start = false;
            }
            _ => {
                stmt_start = false;
                i += 1;
            }
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
