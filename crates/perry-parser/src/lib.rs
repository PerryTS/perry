//! TypeScript parser wrapper using SWC
//!
//! This crate provides a high-level interface to parse TypeScript source code
//! into an AST using the SWC parser, with integrated diagnostic support.

use anyhow::Result;
use perry_diagnostics::{Diagnostic, DiagnosticCode, Diagnostics, FileId, SourceCache, Span};
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
    (path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".jsx"))
        && !looks_like_es_module(source)
}

fn looks_like_es_module(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("import ")
            || trimmed.starts_with("import{")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("export{")
            || trimmed.starts_with("export*")
    })
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

    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut state = State::Code;
    while i < bytes.len() {
        match state {
            State::Code => {
                if bytes[i] == b'\'' || bytes[i] == b'"' || bytes[i] == b'`' {
                    state = State::String(bytes[i]);
                    out.push(bytes[i] as char);
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
                } else if let Some((ch, next)) = read_escape(bytes, i) {
                    out.push(ch);
                    i = next;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            State::String(quote) => {
                out.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    if let Some(&next) = bytes.get(i + 1) {
                        out.push(next as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    if bytes[i] == quote {
                        state = State::Code;
                    }
                    i += 1;
                }
            }
            State::LineComment => {
                out.push(bytes[i] as char);
                if bytes[i] == b'\n' {
                    state = State::Code;
                }
                i += 1;
            }
            State::BlockComment => {
                out.push(bytes[i] as char);
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    out.push('/');
                    i += 2;
                    state = State::Code;
                } else {
                    i += 1;
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
}
