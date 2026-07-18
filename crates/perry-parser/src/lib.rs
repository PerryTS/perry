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
    let mut diagnostics = Diagnostics::new();

    let (module, mut parser) =
        parse_source_file_with_typescript_fallback(&source_file, filename, &parse_source).map_err(
            |e| {
                // Convert SWC error to our diagnostic
                let span = Span::new(file_id, e.span().lo.0, e.span().hi.0);
                let diag =
                    Diagnostic::error(DiagnosticCode::ParseError, format!("{}", e.kind().msg()))
                        .with_span(span)
                        .build();
                diagnostics.push(diag);
                anyhow::anyhow!("Parse error: {}", e.kind().msg())
            },
        )?;

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

    let (module, mut parser) =
        parse_source_file_with_typescript_fallback(&source_file, filename, &source_file.src)
            .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;

    // Check for recoverable errors
    for error in parser.take_errors() {
        eprintln!("Parse warning: {:?}", error);
    }

    Ok(module)
}

fn parser_for_source_file_with_syntax<'a>(
    source_file: &'a swc_common::SourceFile,
    syntax: Syntax,
) -> Parser<Lexer<'a>> {
    let lexer = Lexer::new(
        syntax,
        swc_ecma_ast::EsVersion::Es2022,
        StringInput::from(source_file),
        None,
    );
    Parser::new_from(lexer)
}

fn parse_source_file_with_typescript_fallback<'a>(
    source_file: &'a swc_common::SourceFile,
    filename: &str,
    source: &str,
) -> swc_ecma_parser::PResult<(Module, Parser<Lexer<'a>>)> {
    let syntax = syntax_for_filename(filename);
    let is_typescript = matches!(syntax, Syntax::Typescript(_));
    let mut parser = parser_for_source_file_with_syntax(source_file, syntax);

    match parse_module_or_script(&mut parser, filename, source) {
        Ok(module) => Ok((module, parser)),
        Err(first_error) => {
            if !is_typescript && source_looks_like_typescript(source) {
                let mut retry_parser = parser_for_source_file_with_syntax(
                    source_file,
                    typescript_syntax_for_filename(filename),
                );
                if let Ok(module) = parse_module_or_script(&mut retry_parser, filename, source) {
                    return Ok((module, retry_parser));
                }
            }
            Err(first_error)
        }
    }
}

/// Strip the optional import query/hash suffix (`./mod.ts?inline`,
/// `mod.wasm#section`) from a filename to recover the bare path for extension
/// detection.
///
/// A Windows extended-length ("verbatim") path begins with the literal `\\?\`
/// prefix — exactly what `std::fs::canonicalize` returns on Windows. The `?` in
/// that prefix must not be mistaken for the start of a query string, or the
/// path would be truncated to `\\` and the real `.ts`/`.tsx` extension lost,
/// making every `.ts` file parse as plain JavaScript (issue #5228). Strip the
/// verbatim prefix before splitting so only a genuine trailing query is removed.
fn path_for_extension_check(filename: &str) -> &str {
    let path = filename.strip_prefix(r"\\?\").unwrap_or(filename);
    path.split(['?', '#']).next().unwrap_or(path)
}

fn syntax_for_filename(filename: &str) -> Syntax {
    let path = path_for_extension_check(filename);
    let lower_path = path.to_ascii_lowercase();
    if lower_path.ends_with(".ts")
        || lower_path.ends_with(".tsx")
        || lower_path.ends_with(".mts")
        || lower_path.ends_with(".cts")
    {
        typescript_syntax_for_path(&lower_path)
    } else {
        Syntax::Es(EsSyntax {
            jsx: lower_path.ends_with(".jsx"),
            decorators: true,
            decorators_before_export: true,
            export_default_from: true,
            import_attributes: true,
            ..Default::default()
        })
    }
}

fn typescript_syntax_for_filename(filename: &str) -> Syntax {
    let path = path_for_extension_check(filename);
    typescript_syntax_for_path(&path.to_ascii_lowercase())
}

fn typescript_syntax_for_path(path: &str) -> Syntax {
    Syntax::Typescript(TsSyntax {
        tsx: path.ends_with(".tsx"),
        decorators: true,
        dts: false,
        no_early_errors: false,
        disallow_ambiguous_jsx_like: false,
    })
}

fn strip_comments_and_strings(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        String(u8),
        LineComment,
        BlockComment,
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
                    out.push(' ');
                    i += 1;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::LineComment;
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = State::BlockComment;
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            State::String(quote) => {
                if bytes[i] == b'\\' {
                    out.push(' ');
                    i += 1;
                    if i < bytes.len() {
                        let ch = source[i..].chars().next().unwrap();
                        out.push(if ch == '\n' { '\n' } else { ' ' });
                        i += ch.len_utf8();
                    }
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    if bytes[i] == quote {
                        state = State::Code;
                    }
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                    i += ch.len_utf8();
                }
            }
            State::LineComment => {
                let ch = source[i..].chars().next().unwrap();
                if bytes[i] == b'\n' {
                    state = State::Code;
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += ch.len_utf8();
            }
            State::BlockComment => {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                    state = State::Code;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                    i += ch.len_utf8();
                }
            }
        }
    }
    out
}

fn source_looks_like_typescript(source: &str) -> bool {
    let stripped = strip_comments_and_strings(source);
    stripped.contains("):")
        || stripped.contains(": number")
        || stripped.contains(": string")
        || stripped.contains(": boolean")
        || stripped.contains(": Promise")
        || stripped.contains(": void")
        || stripped.contains(": any")
        || stripped.contains(": unknown")
        || stripped.contains(": bigint")
        || stripped.contains(": symbol")
        || stripped.contains(": object")
        || stripped.contains(" as ")
        || stripped.contains("interface ")
        || stripped.contains("type ")
        || stripped.contains("enum ")
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
    let path = path_for_extension_check(filename);
    if !(path.ends_with(".js") || path.ends_with(".cjs") || path.ends_with(".jsx")) {
        return false;
    }
    if !path.ends_with(".cjs") && file_is_in_esm_package_context(path) {
        return false;
    }
    !looks_like_es_module(source)
}

/// Whether `filename` is an ES module purely by its module FORMAT — i.e.
/// independent of in-file `import`/`export` syntax or a `"use strict"`
/// directive, which the caller weighs separately. An explicit ESM extension
/// (`.mjs`/`.mts`) is always a module; a CommonJS extension (`.cjs`/`.cts`) is
/// never a module by format (Node treats it as CommonJS even under
/// `"type":"module"`); an ambiguous extension (`.js`/`.ts`/`.jsx`/`.tsx`) is a
/// module when it sits in an ESM package context (`"type":"module"` /
/// conditional-export map). Mirrors the format half of Node's CJS-vs-ESM
/// detection and of `should_parse_as_script`'s package-context guard.
///
/// Module code is strict-mode code, so lowering consults this to decide the
/// runtime strictness of a file that carries no in-file module syntax (#6542):
/// a frozen-object write is a silent no-op in a sloppy CommonJS script but a
/// TypeError in an ESM/strict module.
pub fn file_is_es_module_by_format(filename: &str) -> bool {
    let path = path_for_extension_check(filename);
    if path.ends_with(".mjs") || path.ends_with(".mts") {
        return true;
    }
    if path.ends_with(".cjs") || path.ends_with(".cts") {
        return false;
    }
    file_is_in_esm_package_context(path)
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

    // Whether a top-level `import`/`export` keyword found here can begin a
    // module item, given `last_sig` — the last significant *code* byte seen so
    // far (0 = start of input). A module item starts at input start or right
    // after a statement boundary (`;`, `{`, `}`); anything else (an operator, an
    // identifier byte, a string/regex terminator) means the keyword is part of a
    // larger expression and not a real `import`/`export` statement.
    //
    // `last_sig` is tracked during the forward scan rather than recovered by
    // walking the raw bytes backward, because a backward walk cannot tell that
    // the preceding bytes were inside a comment. Bundler chunks almost always
    // open with a banner comment (`// chunk-….js`) immediately followed by a
    // top-level `export`/`import`; a raw backward walk would see the comment's
    // last character (e.g. the `)` of "(cross-chunk re-export)") and wrongly
    // conclude the keyword can't start a module item, so the `.js` chunk parsed
    // as a Script and SWC raised `ImportExportInScript` (issue #5207).
    fn allows_module_item(last_sig: u8) -> bool {
        matches!(last_sig, 0 | b';' | b'{' | b'}')
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

    // A `/` starts a regex literal (not division) when the preceding token
    // cannot end an expression: an operator/punctuator, start of input, or a
    // keyword like `return`. Regex literals may contain unescaped quote chars
    // (e.g. picomatch's `/(^[*!]|[/()[\]{}"])/`), which would desync the
    // string-state scan below if skipped as ordinary code.
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
    let mut i = 0;
    let mut state = State::Code;
    // Last significant code byte seen (0 = start of input). Comments are
    // transparent — they never update this — so a banner comment before a
    // top-level `import`/`export` no longer hides the keyword. Strings and
    // regex literals leave their terminator (`"`/`'`/`` ` ``/`/`) as the last
    // significant byte, matching the old backward walk's behavior.
    let mut last_sig: u8 = 0;
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
                } else if bytes[i] == b'/' && regex_can_start_here(bytes, i) {
                    match skip_regex_literal(bytes, i) {
                        Some(end) => i = end,
                        None => i += 1,
                    }
                    // A regex literal (or a `/` division operator) is an
                    // expression token — a following keyword can't begin a
                    // module item.
                    last_sig = b'/';
                } else {
                    if allows_module_item(last_sig) {
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
                    if !matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
                        last_sig = bytes[i];
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
                        last_sig = quote;
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
    fn test_looks_like_es_module_survives_regex_with_quote() {
        // Regression: picomatch's bundled source contains a regex literal with
        // an unescaped `"` inside a character class. The module-detection scan
        // must not enter string state there, or a trailing `export` (appended
        // by the CJS wrap) is missed and the file parses as a Script.
        let source = "const re = /(^[*!]|[/()[\\]{}\"])/;\nconst x = \"ok\";\nexport default x;\n";
        let module = parse_typescript(source, "vendored.js").unwrap();
        assert_eq!(module.body.len(), 3);
    }

    #[test]
    fn test_banner_comment_before_top_level_export_is_module() {
        // Regression for #5207: a bundler code-split chunk almost always opens
        // with a banner comment immediately followed by a top-level `export`
        // (or `import`). The module-detection scan must look through the comment
        // — its last character (here the `)` of "(cross-chunk re-export)") must
        // not be mistaken for a preceding code token that bars a module item, or
        // the `.js` chunk parses as a Script and SWC raises ImportExportInScript.
        let cases = [
            "// runtime chunk (cross-chunk re-export)\nexport function rt(x) { return x; }\n",
            "// banner foo\nexport const V = 1;\n",
            "/* block banner */\nimport { x } from \"./chunk-shared.js\";\n",
            "// a\n// b\n// c\nexport { y } from \"./other.js\";\n",
        ];
        for src in cases {
            assert!(
                looks_like_es_module(src),
                "expected ESM classification for chunk:\n{src}"
            );
            // And it must actually parse as a module rather than a Script.
            parse_typescript(src, "chunk-abc.js")
                .unwrap_or_else(|e| panic!("chunk failed to parse as a module {src:?}: {e:?}"));
        }
    }

    #[test]
    fn test_comment_does_not_create_false_module_classification() {
        // The transparency fix must not flip a genuinely CommonJS chunk to ESM:
        // a comment ending in `;`/`{`/`}` followed by a non-keyword leaves the
        // file a Script, and `exportFoo`/`importMap`-style identifiers after a
        // comment still don't match the `export`/`import` keywords.
        assert!(!looks_like_es_module(
            "// helper;\nconst exportFoo = 1;\nmodule.exports = exportFoo;\n"
        ));
        assert!(!looks_like_es_module(
            "// note\nconst importMap = {};\nmodule.exports = importMap;\n"
        ));
    }

    #[test]
    fn test_division_not_treated_as_regex() {
        // `a / b` must not be consumed as a regex literal that would swallow
        // the following string quote.
        let source = "const a = 1, b = 2;\nconst c = a / b; const s = \"x\";\nexport default c;\n";
        let module = parse_typescript(source, "math.js").unwrap();
        assert_eq!(module.body.len(), 4);
    }

    #[test]
    fn windows_verbatim_path_parses_as_typescript() {
        // Regression for #5228: on Windows, `std::fs::canonicalize` returns a
        // verbatim path prefixed with `\\?\`. The `?` must not be treated as the
        // start of an import query string, or the extension is lost and the file
        // parses as plain JavaScript — which rejects type annotations whose exact
        // shape the TS-recovery heuristic doesn't catch (`(x: Uint8Array)`).
        let cases = [
            "function f(x: Uint8Array) {}\n",
            "const f = (x: Uint8Array) => 1;\n",
            "function f(x: Uint8Array[]) {}\n",
            "const f = (chunk: Uint8Array | string) => {};\n",
        ];
        for src in cases {
            parse_typescript(src, r"\\?\C:\Users\x\repro.ts")
                .unwrap_or_else(|e| panic!("verbatim .ts path failed to parse {src:?}: {e:?}"));
        }
    }

    #[test]
    fn path_for_extension_check_strips_verbatim_prefix_and_query() {
        assert_eq!(path_for_extension_check(r"\\?\C:\a\mod.ts"), r"C:\a\mod.ts");
        assert_eq!(path_for_extension_check("./mod.ts?inline"), "./mod.ts");
        assert_eq!(path_for_extension_check("mod.wasm#section"), "mod.wasm");
        assert_eq!(path_for_extension_check("/home/u/mod.ts"), "/home/u/mod.ts");
    }

    #[test]
    fn file_is_es_module_by_format_honors_extension() {
        // #6542: an explicit ESM extension is a module by format regardless of
        // any package context, so its code is strict.
        assert!(file_is_es_module_by_format("/no/such/pkg/mod.mjs"));
        assert!(file_is_es_module_by_format("/no/such/pkg/mod.mts"));
        assert!(file_is_es_module_by_format("./mod.mjs?v=2"));
        // A CommonJS extension is never a module by format — Node treats `.cjs`/
        // `.cts` as CommonJS even under `"type":"module"`, so an ancestor
        // package context must not flip these to strict.
        assert!(!file_is_es_module_by_format("/no/such/pkg/mod.cjs"));
        assert!(!file_is_es_module_by_format("/no/such/pkg/mod.cts"));
        // An ambiguous extension with no package.json anywhere up the tree is a
        // CommonJS script (sloppy) — the format signal alone does not make it a
        // module (in-file import/export or a "use strict" directive still can,
        // but those are weighed by the caller, not here).
        assert!(!file_is_es_module_by_format(
            "/nonexistent-perry-6542-root/deep/mod.ts"
        ));
    }

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
    fn test_parse_thread_promise_void_regression_source() {
        let source = r#"
            import { parallelFilter, parallelMap, spawn } from "perry/thread";

            async function spawnNonBlocking(): Promise<void> {
                const bgThread = spawn(() => {
                    let n = 0;
                    for (let i = 0; i < 10; i++) n++;
                    return n;
                });

                const result: number = await bgThread;
                console.log(result.toLocaleString("en-US"));
            }

            await spawnNonBlocking();
        "#;

        let module = parse_typescript(source, "thread.ts").unwrap();
        assert_eq!(module.body.len(), 3);
    }

    #[test]
    fn test_parse_thread_promise_void_windows_uppercase_ts_path() {
        let source = r#"
            async function spawnNonBlocking(): Promise<void> {
                const result: number = await Promise.resolve(1);
                console.log(result);
            }
        "#;

        let module = parse_typescript(source, r"C:\repo\src\THREAD.TS").unwrap();
        assert_eq!(module.body.len(), 1);
    }

    #[test]
    fn test_parse_thread_promise_void_extensionless_fallback() {
        let source = r#"
            async function spawnNonBlocking(): Promise<void> {
                const result: number = await Promise.resolve(1);
                console.log(result);
            }
        "#;

        let module = parse_typescript(source, "thread").unwrap();
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
