//! TypeScript parser wrapper using SWC
//!
//! This crate provides a high-level interface to parse TypeScript source code
//! into an AST using the SWC parser, with integrated diagnostic support.

use anyhow::Result;
use perry_diagnostics::{Diagnostic, DiagnosticCode, Diagnostics, FileId, SourceCache, Span};
use std::path::Path;
use swc_common::{input::StringInput, sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::{Module, ModuleItem, Script};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, Syntax, TsSyntax};

// Re-export AST types for consumers that need to inspect the AST
pub use swc_ecma_ast;

// Re-export Spanned trait for getting spans from AST nodes
pub use swc_common::Spanned;

/// Result of parsing a TypeScript file.
#[derive(Debug)]
pub struct ParseResult {
    /// The parsed AST module
    pub module: Module,
    /// The file ID in the source cache
    pub file_id: FileId,
    /// Any diagnostics (parse warnings, etc.)
    pub diagnostics: Diagnostics,
}

/// Parse TypeScript source code into an AST Module with diagnostic support.
///
/// This function parses TypeScript source code, adds it to the source cache,
/// and returns both the AST and any diagnostics encountered during parsing.
///
/// # Arguments
///
/// * `source` - The TypeScript source code to parse
/// * `filename` - The filename for error reporting
/// * `cache` - The source cache to add the file to
///
/// # Returns
///
/// A `ParseResult` containing the AST, file ID, and any diagnostics.
pub fn parse_typescript_with_cache(
    source: &str,
    filename: &str,
    cache: &mut SourceCache,
) -> Result<ParseResult> {
    let parse_source = normalize_unicode_identifier_escapes(source);
    // Add the source to the cache
    let file_id = cache.add_file(filename, source.to_string());

    // Create SWC source map (separate from our cache, used internally by SWC)
    let source_map: Lrc<SourceMap> = Default::default();
    let source_file = source_map.new_source_file(
        Lrc::new(FileName::Custom(filename.to_string())),
        parse_source.clone(),
    );

    let mut parser = parser_for_source_file(&source_file, filename);
    let mut diagnostics = Diagnostics::new();

    let module = parse_module_or_script(&mut parser, filename, &parse_source).map_err(|e| {
        // Convert SWC error to our diagnostic
        let span = Span::new(file_id, e.span().lo.0, e.span().hi.0);
        let diag = Diagnostic::error(DiagnosticCode::ParseError, format!("{}", e.kind().msg()))
            .with_span(span)
            .build();
        diagnostics.push(diag);
        anyhow::anyhow!("Parse error: {}", e.kind().msg())
    })?;

    // Collect recoverable errors as warnings
    for error in parser.take_errors() {
        let span = Span::new(file_id, error.span().lo.0, error.span().hi.0);
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::ParseError,
                format!("{}", error.kind().msg()),
            )
            .with_span(span)
            .build(),
        );
    }

    Ok(ParseResult {
        module,
        file_id,
        diagnostics,
    })
}

/// Parse TypeScript source code into an AST Module (legacy API).
///
/// This is the original parsing function for backward compatibility.
/// For new code, prefer `parse_typescript_with_cache` for better diagnostics.
pub fn parse_typescript(source: &str, filename: &str) -> Result<Module> {
    let parse_source = normalize_unicode_identifier_escapes(source);
    let source_map: Lrc<SourceMap> = Default::default();
    let source_file = source_map.new_source_file(
        Lrc::new(FileName::Custom(filename.to_string())),
        parse_source,
    );

    let mut parser = parser_for_source_file(&source_file, filename);

    let module = parse_module_or_script(&mut parser, filename, &source_file.src)
        .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;

    // Check for recoverable errors
    for error in parser.take_errors() {
        eprintln!("Parse warning: {:?}", error);
    }

    Ok(module)
}

fn parser_for_source_file<'a>(
    source_file: &'a swc_common::SourceFile,
    filename: &str,
) -> Parser<Lexer<'a>> {
    let lexer = Lexer::new(
        syntax_for_filename(filename),
        swc_ecma_ast::EsVersion::Es2022,
        StringInput::from(source_file),
        None,
    );
    Parser::new_from(lexer)
}

fn syntax_for_filename(filename: &str) -> Syntax {
    let path = filename.split(['?', '#']).next().unwrap_or(filename);
    if path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
    {
        Syntax::Typescript(TsSyntax {
            tsx: path.ends_with(".tsx"),
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: false,
        })
    } else {
        Syntax::Es(EsSyntax {
            jsx: path.ends_with(".jsx"),
            decorators: true,
            decorators_before_export: true,
            export_default_from: true,
            import_attributes: true,
            ..Default::default()
        })
    }
}

fn parse_module_or_script(
    parser: &mut Parser<Lexer<'_>>,
    filename: &str,
    source: &str,
) -> swc_ecma_parser::PResult<Module> {
    if should_parse_as_script(filename, source) {
        parser.parse_script().map(script_to_module)
    } else {
        parser.parse_module()
    }
}

fn should_parse_as_script(filename: &str, source: &str) -> bool {
    let path = filename.split(['?', '#']).next().unwrap_or(filename);
    if !(path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".jsx")) {
        return false;
    }
    if !path.ends_with(".cjs") && file_is_in_esm_package_context(path) {
        return false;
    }
    !looks_like_es_module(source)
}

fn looks_like_es_module(source: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        String(u8),
        LineComment,
        BlockComment,
    }

    fn is_ident(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }

    fn prev_allows_module_item(bytes: &[u8], mut i: usize) -> bool {
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b';' | b'{' | b'}' => return true,
                _ => return false,
            }
        }
        true
    }

    fn next_after_keyword(bytes: &[u8], i: usize, keyword: &[u8]) -> Option<usize> {
        let end = i.checked_add(keyword.len())?;
        if bytes.get(i..end)? != keyword {
            return None;
        }
        if i > 0 && is_ident(bytes[i - 1]) {
            return None;
        }
        if bytes.get(end).is_some_and(|b| is_ident(*b)) {
            return None;
        }
        Some(end)
    }

    let bytes = source.as_bytes();
    let mut i = 0;
    let mut state = State::Code;
    while i < bytes.len() {
        match state {
            State::Code => {
                if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
                    state = State::String(bytes[i]);
                    i += 1;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::LineComment;
                    i += 2;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = State::BlockComment;
                    i += 2;
                } else {
                    if prev_allows_module_item(bytes, i) {
                        if let Some(end) = next_after_keyword(bytes, i, b"export") {
                            if matches!(
                                bytes.get(end),
                                Some(b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*')
                            ) {
                                return true;
                            }
                        }
                        if let Some(end) = next_after_keyword(bytes, i, b"import") {
                            if matches!(
                                bytes.get(end),
                                Some(b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*' | b'"' | b'\'')
                            ) || bytes.get(end) == Some(&b'.')
                            {
                                return true;
                            }
                        }
                    }
                    i += 1;
                }
            }
            State::String(quote) => {
                if bytes[i] == b'\\' {
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
                }
                i += 1;
            }
            State::BlockComment => {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    i += 2;
                    state = State::Code;
                } else {
                    i += 1;
                }
            }
        }
    }
    false
}

fn file_is_in_esm_package_context(filename: &str) -> bool {
    let path = Path::new(filename);
    let mut current = Path::new(filename).parent();
    while let Some(dir) = current {
        let package_json = dir.join("package.json");
        if package_json.exists() {
            if let Ok(content) = std::fs::read_to_string(&package_json) {
                if package_json_declares_esm_context(&content, dir, path) {
                    return true;
                }
            }
        }
        current = dir.parent();
    }
    false
}

fn package_json_declares_esm_context(content: &str, package_dir: &Path, file_path: &Path) -> bool {
    let compact: String = content.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.contains(r#""type":"module""#) {
        return true;
    }

    let relative = match file_path.strip_prefix(package_dir) {
        Ok(path) => path.to_string_lossy().replace('\\', "/"),
        Err(_) => return false,
    };
    let relative_dot = format!("./{relative}");
    let quoted_relative = format!(r#""{relative}""#);
    let quoted_relative_dot = format!(r#""{relative_dot}""#);
    let metadata_mentions_file =
        compact.contains(&quoted_relative) || compact.contains(&quoted_relative_dot);

    metadata_mentions_file && (compact.contains(r#""module":"#) || compact.contains(r#""import":"#))
}

fn script_to_module(script: Script) -> Module {
    Module {
        span: script.span,
        body: script.body.into_iter().map(ModuleItem::Stmt).collect(),
        shebang: script.shebang,
    }
}

fn normalize_unicode_identifier_escapes(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        String(u8),
        Regex { in_class: bool },
        LineComment,
        BlockComment,
    }

    fn hex_value(b: u8) -> Option<u32> {
        match b {
            b'0'..=b'9' => Some((b - b'0') as u32),
            b'a'..=b'f' => Some((b - b'a' + 10) as u32),
            b'A'..=b'F' => Some((b - b'A' + 10) as u32),
            _ => None,
        }
    }

    fn read_escape(bytes: &[u8], i: usize) -> Option<(char, usize)> {
        if bytes.get(i) != Some(&b'\\') || bytes.get(i + 1) != Some(&b'u') {
            return None;
        }
        if bytes.get(i + 2) == Some(&b'{') {
            let mut j = i + 3;
            let mut value = 0u32;
            let mut saw_digit = false;
            while let Some(&b) = bytes.get(j) {
                if b == b'}' {
                    if saw_digit {
                        return char::from_u32(value).map(|ch| (ch, j + 1));
                    }
                    return None;
                }
                value = value.checked_mul(16)?.checked_add(hex_value(b)?)?;
                saw_digit = true;
                j += 1;
            }
            return None;
        }
        let mut value = 0u32;
        for off in 2..6 {
            value = value
                .checked_mul(16)?
                .checked_add(hex_value(*bytes.get(i + off)?)?)?;
        }
        char::from_u32(value).map(|ch| (ch, i + 6))
    }

    #[derive(Clone, Copy)]
    enum LastSig {
        None,
        Char(u8),
        Ident { start: usize, end: usize },
    }

    fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
    }

    fn regex_allowed_after_keyword(word: &str) -> bool {
        matches!(
            word,
            "return"
                | "throw"
                | "case"
                | "delete"
                | "void"
                | "typeof"
                | "yield"
                | "await"
                | "else"
                | "do"
                | "in"
                | "of"
        )
    }

    fn last_sig_allows_regex(last: LastSig, source: &str) -> bool {
        match last {
            LastSig::None => true,
            LastSig::Char(b) => matches!(
                b,
                b'(' | b'{'
                    | b'['
                    | b'='
                    | b':'
                    | b','
                    | b';'
                    | b'!'
                    | b'?'
                    | b'+'
                    | b'-'
                    | b'*'
                    | b'%'
                    | b'&'
                    | b'|'
                    | b'^'
                    | b'~'
                    | b'<'
                    | b'>'
            ),
            LastSig::Ident { start, end } => regex_allowed_after_keyword(&source[start..end]),
        }
    }

    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut state = State::Code;
    let mut last_sig = LastSig::None;
    while i < bytes.len() {
        match state {
            State::Code => {
                if bytes[i].is_ascii_whitespace() {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                } else if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
                    state = State::String(bytes[i]);
                    out.push(bytes[i] as char);
                    last_sig = LastSig::Char(bytes[i]);
                    i += 1;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::LineComment;
                    out.push('/');
                    out.push('/');
                    i += 2;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = State::BlockComment;
                    out.push('/');
                    out.push('*');
                    i += 2;
                } else if bytes[i] == b'/' && last_sig_allows_regex(last_sig, source) {
                    state = State::Regex { in_class: false };
                    out.push('/');
                    last_sig = LastSig::Char(b'/');
                    i += 1;
                } else if let Some((ch, next)) = read_escape(bytes, i) {
                    out.push(ch);
                    if ch == '_' || ch == '$' || ch.is_alphanumeric() {
                        last_sig = LastSig::Ident {
                            start: i,
                            end: next,
                        };
                    } else {
                        last_sig = LastSig::Char(b'\\');
                    }
                    i = next;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    if bytes[i].is_ascii() && is_ident_byte(bytes[i]) {
                        let start = i;
                        i += 1;
                        while bytes.get(i).is_some_and(|b| is_ident_byte(*b)) {
                            out.push(bytes[i] as char);
                            i += 1;
                        }
                        last_sig = LastSig::Ident { start, end: i };
                    } else {
                        last_sig = LastSig::Char(bytes[i]);
                        i += ch.len_utf8();
                    }
                }
            }
            State::String(quote) => {
                if bytes[i] == b'\\' {
                    out.push('\\');
                    i += 1;
                    if i < bytes.len() {
                        let ch = source[i..].chars().next().unwrap();
                        out.push(ch);
                        i += ch.len_utf8();
                    }
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    if bytes[i] == quote {
                        state = State::Code;
                    }
                    i += ch.len_utf8();
                }
            }
            State::Regex { in_class } => {
                out.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    if let Some(&next) = bytes.get(i + 1) {
                        out.push(next as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if bytes[i] == b'[' {
                    state = State::Regex { in_class: true };
                    i += 1;
                } else if bytes[i] == b']' {
                    state = State::Regex { in_class: false };
                    i += 1;
                } else if bytes[i] == b'/' && !in_class {
                    state = State::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            State::LineComment => {
                let ch = source[i..].chars().next().unwrap();
                out.push(ch);
                if bytes[i] == b'\n' {
                    state = State::Code;
                }
                i += ch.len_utf8();
            }
            State::BlockComment => {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    out.push('*');
                    out.push('/');
                    i += 2;
                    state = State::Code;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
    }
    out
}

/// Utility to convert SWC span to our span type.
///
/// This is useful when processing SWC AST nodes and need to create
/// diagnostics with proper span information.
pub fn swc_span_to_span(swc_span: swc_common::Span, file_id: FileId) -> Span {
    Span::new(file_id, swc_span.lo.0, swc_span.hi.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let source = r#"
            function factorial(n: number): number {
                if (n <= 1) return 1;
                return n * factorial(n - 1);
            }
        "#;

        let module = parse_typescript(source, "test.ts").unwrap();
        assert_eq!(module.body.len(), 1);
    }

    #[test]
    fn test_parse_class() {
        let source = r#"
            class Trade {
                public id: number;
                public amount: bigint;

                constructor(id: number) {
                    this.id = id;
                    this.amount = 0n;
                }
            }
        "#;

        let module = parse_typescript(source, "test.ts").unwrap();
        assert_eq!(module.body.len(), 1);
    }

    #[test]
    fn test_parse_with_cache() {
        let source = "let x: number = 42;";
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.ts", &mut cache).unwrap();

        assert_eq!(result.module.body.len(), 1);
        assert!(!result.file_id.0 == FileId::DUMMY.0);
        assert!(result.diagnostics.is_empty());

        // Verify the file is in the cache
        assert!(cache.get_file(result.file_id).is_some());
    }

    #[test]
    fn test_parse_error_with_cache() {
        let source = "let x: number = ;"; // Invalid syntax
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.ts", &mut cache);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_js_sloppy_with_without_ts_warning() {
        let source = r#"
            function foo() {
                var a = { a: 10 };
                with (a) {
                    return () => a;
                }
            }
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_sloppy_yield_arrow_parameter() {
        let source = r#"
            var yield = 23;
            var f = (x = yield) => x;
            var g = yield => yield;
            var h = (yield) => yield;
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_sloppy_future_reserved_words() {
        let source = r#"
            var implements = "implements";
            var interface = "interface";
            var package = "package";
            var private = "private";
            var protected = "protected";
            var public = "public";
            var static = "static";
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_lookalike_directives_keep_function_body_sloppy() {
        let source = r#"
            function doubledSpace() {
                "use  strict";
                var public = 1;
                return public;
            }
            function escapedSpace() {
                "use\x20strict";
                var yield = 2;
                return yield;
            }
            function interrupted() {
                var interface = 3;
                "use strict";
                return interface;
            }
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_sloppy_escaped_contextual_identifiers() {
        let source = r#"
            var imp\u006Cements = 1;
            var yie\u006Cd = 2;
            var awa\u0069t = 3;
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_keyword_property_accessors() {
        let source = r#"
            var obj = { await: 0, yield: 1, static: 2, implements: 3 };
            obj.await = "await";
            obj.yield = "yield";
            obj.static = "static";
            obj.implements = "implements";
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_ts_still_rejects_ts_syntax_errors() {
        let source = "let x: number = ;";
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.ts", &mut cache);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_js_module_syntax_still_uses_module_parser() {
        let source = r#"
            export const value = 1;
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_regex_preserves_control_unicode_escapes() {
        let source = r#"
            const ASCII_WHITESPACE_REPLACE_REGEX = /[\u0009\u000A\u000C\u000D\u0020]/g;
            export default ASCII_WHITESPACE_REPLACE_REGEX;
        "#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "undici-cjs-wrap.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_minified_js_module_syntax_uses_module_parser() {
        let source = r#"const value=1;export{value};"#;
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(source, "test.js", &mut cache).unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_parse_js_script_regex_preserves_control_unicode_escapes() {
        let source = r#"
'use strict'

const ASCII_WHITESPACE_REPLACE_REGEX = /[\u0009\u000A\u000C\u000D\u0020]/g // eslint-disable-line no-control-regex

if (!ASCII_WHITESPACE_REPLACE_REGEX.test(' ')) {
  throw new Error('unexpected regex result')
}
"#;
        let mut cache = SourceCache::new();

        let result =
            parse_typescript_with_cache(source, "undici-cjs-wrap-control-regex.js", &mut cache)
                .unwrap();

        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn normalize_preserves_non_ascii_in_strings_and_comments() {
        let source = "const s = \"café\"; // déjà\nconst t = `naïve`; /* año */";
        assert_eq!(normalize_unicode_identifier_escapes(source), source);
    }

    #[test]
    fn normalize_keeps_string_unicode_escapes_literal() {
        let source = r#"let \u0061 = "\u0062";"#;
        assert_eq!(
            normalize_unicode_identifier_escapes(source),
            r#"let a = "\u0062";"#
        );
    }

    #[test]
    fn test_parse_js_inside_type_module_package_uses_module_parser() {
        let dir =
            std::env::temp_dir().join(format!("perry_parser_type_module_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), r#"{ "type": "module" }"#).unwrap();
        let source_path = dir.join("index.js");
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(
            "const value = 1; export { value };",
            source_path.to_str().unwrap(),
            &mut cache,
        )
        .unwrap();

        assert!(result.diagnostics.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_js_referenced_by_import_export_metadata_uses_module_parser() {
        let dir = std::env::temp_dir().join(format!(
            "perry_parser_exports_module_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let esm_dir = dir.join("dist/esm");
        std::fs::create_dir_all(&esm_dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{
  "name": "pkg",
  "exports": {
    ".": {
      "import": {
        "default": "./dist/esm/index.js"
      }
    }
  },
  "module": "./dist/esm/index.js"
}"#,
        )
        .unwrap();
        let source_path = esm_dir.join("index.js");
        let mut cache = SourceCache::new();

        let result = parse_typescript_with_cache(
            "await Promise.resolve(1);",
            source_path.to_str().unwrap(),
            &mut cache,
        )
        .unwrap();

        assert!(result.diagnostics.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
