//! TypeScript parser wrapper using SWC
//!
//! This crate provides a high-level interface to parse TypeScript source code
//! into an AST using the SWC parser, with integrated diagnostic support.

use anyhow::Result;
use perry_diagnostics::{Diagnostic, DiagnosticCode, Diagnostics, FileId, SourceCache, Span};
use swc_common::{input::StringInput, sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::Module;
use swc_ecma_parser::{lexer::Lexer, Parser, Syntax, TsSyntax};

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
    // Add the source to the cache
    let file_id = cache.add_file(filename, source.to_string());

    // Create SWC source map (separate from our cache, used internally by SWC)
    let source_map: Lrc<SourceMap> = Default::default();
    let source_file = source_map.new_source_file(
        Lrc::new(FileName::Custom(filename.to_string())),
        source.to_string(),
    );

    // Enable TSX parsing for .tsx files
    let is_tsx = filename.ends_with(".tsx");

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: is_tsx,
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: false,
        }),
        swc_ecma_ast::EsVersion::Es2022,
        StringInput::from(&*source_file),
        None,
    );

    let mut parser = Parser::new_from(lexer);
    let mut diagnostics = Diagnostics::new();

    let module = parser.parse_module().map_err(|e| {
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
    let source_map: Lrc<SourceMap> = Default::default();
    let source_file = source_map.new_source_file(
        Lrc::new(FileName::Custom(filename.to_string())),
        source.to_string(),
    );

    let is_tsx = filename.ends_with(".tsx");

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: is_tsx,
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: false,
        }),
        swc_ecma_ast::EsVersion::Es2022,
        StringInput::from(&*source_file),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    let module = parser
        .parse_module()
        .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;

    // Check for recoverable errors
    for error in parser.take_errors() {
        eprintln!("Parse warning: {:?}", error);
    }

    Ok(module)
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
}
