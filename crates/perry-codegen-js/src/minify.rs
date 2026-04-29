//! JavaScript source minifier
//!
//! Strips comments, collapses whitespace, and produces compact output.
//! Correctly handles string literals, template literals, and regex literals.

/// Minify JavaScript source code by removing comments and collapsing whitespace.
///
/// Preserves string literals, template literals, and regex literals exactly.
/// Works at the byte level for efficiency (JS syntax is ASCII outside literals).
pub fn minify_js(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = Vec::with_capacity(len / 2);
    let mut i = 0;

    while i < len {
        match bytes[i] {
            // String literals — copy through unchanged
            b'"' | b'\'' => {
                let quote = bytes[i];
                out.push(quote);
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        out.push(bytes[i]);
                        out.push(bytes[i + 1]);
                        i += 2;
                    } else if bytes[i] == quote {
                        out.push(quote);
                        i += 1;
                        break;
                    } else {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            // Template literals — copy through, handling ${} nesting
            b'`' => {
                out.push(b'`');
                i += 1;
                copy_template_literal(bytes, &mut i, &mut out);
            }
            // Single-line comment
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                } // skip newline
                  // Emit space if needed to prevent token merge
                emit_separator_if_needed(
                    &out,
                    if i < len { Some(bytes[i]) } else { None },
                    &mut out.clone(),
                );
                if i < len && !out.is_empty() && needs_space(*out.last().unwrap(), bytes[i]) {
                    out.push(b' ');
                }
            }
            // Multi-line comment
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < len {
                    i += 2;
                } // skip */
                  // Emit space if needed to prevent token merge
                if i < len && !out.is_empty() && needs_space(*out.last().unwrap(), bytes[i]) {
                    out.push(b' ');
                }
            }
            // Regex literal (when context indicates regex, not division)
            b'/' if is_regex_start(&out) => {
                out.push(b'/');
                i += 1;
                while i < len && bytes[i] != b'/' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        out.push(bytes[i]);
                        out.push(bytes[i + 1]);
                        i += 2;
                    } else if bytes[i] == b'[' {
                        // Character class — / doesn't end regex inside []
                        out.push(bytes[i]);
                        i += 1;
                        while i < len && bytes[i] != b']' {
                            if bytes[i] == b'\\' && i + 1 < len {
                                out.push(bytes[i]);
                                out.push(bytes[i + 1]);
                                i += 2;
                            } else {
                                out.push(bytes[i]);
                                i += 1;
                            }
                        }
                        if i < len {
                            out.push(bytes[i]);
                            i += 1;
                        }
                    } else {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
                if i < len {
                    out.push(bytes[i]);
                    i += 1;
                } // closing /
                  // Regex flags
                while i < len && bytes[i].is_ascii_alphabetic() {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            // Regular / (division operator)
            b'/' => {
                out.push(b'/');
                i += 1;
            }
            // Whitespace — collapse and emit only when needed
            b if b.is_ascii_whitespace() => {
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && !out.is_empty() && needs_space(*out.last().unwrap(), bytes[i]) {
                    out.push(b' ');
                }
            }
            // Everything else — pass through
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    // The output is valid UTF-8 because we only remove ASCII whitespace/comments
    // and copy everything else byte-for-byte from valid UTF-8 input
    unsafe { String::from_utf8_unchecked(out) }
}

/// Copy a template literal body (after the opening backtick) to output.
/// Handles `${}` expression interpolation with proper brace nesting,
/// including nested template literals inside expressions.
fn copy_template_literal(bytes: &[u8], i: &mut usize, out: &mut Vec<u8>) {
    let len = bytes.len();
    while *i < len {
        if bytes[*i] == b'\\' && *i + 1 < len {
            out.push(bytes[*i]);
            out.push(bytes[*i + 1]);
            *i += 2;
        } else if bytes[*i] == b'`' {
            out.push(b'`');
            *i += 1;
            return;
        } else if bytes[*i] == b'$' && *i + 1 < len && bytes[*i + 1] == b'{' {
            out.push(b'$');
            out.push(b'{');
            *i += 2;
            // Inside ${}, parse as JS until matching }
            let mut brace_depth: usize = 1;
            while *i < len && brace_depth > 0 {
                match bytes[*i] {
                    b'{' => {
                        brace_depth += 1;
                        out.push(b'{');
                        *i += 1;
                    }
                    b'}' => {
                        brace_depth -= 1;
                        out.push(b'}');
                        *i += 1;
                    }
                    b'\'' | b'"' => {
                        let q = bytes[*i];
                        out.push(q);
                        *i += 1;
                        while *i < len {
                            if bytes[*i] == b'\\' && *i + 1 < len {
                                out.push(bytes[*i]);
                                out.push(bytes[*i + 1]);
                                *i += 2;
                            } else if bytes[*i] == q {
                                out.push(q);
                                *i += 1;
                                break;
                            } else {
                                out.push(bytes[*i]);
                                *i += 1;
                            }
                        }
                    }
                    b'`' => {
                        out.push(b'`');
                        *i += 1;
                        copy_template_literal(bytes, i, out);
                    }
                    _ => {
                        out.push(bytes[*i]);
                        *i += 1;
                    }
                }
            }
        } else {
            out.push(bytes[*i]);
            *i += 1;
        }
    }
}

/// Determine if two adjacent bytes need a space between them to prevent token merging.
fn needs_space(last: u8, next: u8) -> bool {
    let is_id = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    // Two identifier characters must not merge
    if is_id(last) && is_id(next) {
        return true;
    }
    // Prevent + + from becoming ++ and - - from becoming --
    if last == b'+' && next == b'+' {
        return true;
    }
    if last == b'-' && next == b'-' {
        return true;
    }
    false
}

/// Check if a `/` at the current position is the start of a regex literal
/// (as opposed to a division operator).
fn is_regex_start(out: &[u8]) -> bool {
    // Find last non-space byte in output so far
    let last = out.iter().rev().find(|b| !b.is_ascii_whitespace());
    match last {
        None => true, // Start of input — must be regex
        Some(b) => match b {
            // After these characters, / is division
            b')' | b']' | b'_' | b'$' => false,
            b if b.is_ascii_alphanumeric() => false,
            // After operators, punctuation — regex
            _ => true,
        },
    }
}

/// Emit a space separator if the last output byte and next input byte
/// would merge into a different token.
fn emit_separator_if_needed(_out: &[u8], _next: Option<u8>, _target: &mut Vec<u8>) {
    // Handled inline in the main loop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strips_single_line_comments() {
        let input = "var x = 1; // comment\nvar y = 2;";
        let result = minify_js(input);
        assert!(result.contains("var x=1;"));
        assert!(!result.contains("comment"));
        assert!(result.contains("var y=2;"));
    }

    #[test]
    fn test_strips_multi_line_comments() {
        let input = "var x = 1; /* multi\nline\ncomment */ var y = 2;";
        let result = minify_js(input);
        assert!(!result.contains("multi"));
        assert!(result.contains("var x=1;"));
        assert!(result.contains("var y=2;"));
    }

    #[test]
    fn test_preserves_strings() {
        let input = r#"var x = "hello world"; var y = 'foo bar';"#;
        let result = minify_js(input);
        assert!(result.contains("\"hello world\""));
        assert!(result.contains("'foo bar'"));
    }

    #[test]
    fn test_preserves_template_literals() {
        let input = "var x = `hello ${name} world`;";
        let result = minify_js(input);
        assert!(result.contains("`hello ${name} world`"));
    }

    #[test]
    fn test_collapses_whitespace() {
        let input = "var   x   =   1 ;";
        let result = minify_js(input);
        assert_eq!(result, "var x=1;");
    }

    #[test]
    fn test_preserves_regex() {
        let input = "var re = /pattern/gi;";
        let result = minify_js(input);
        assert!(result.contains("/pattern/gi"));
    }

    #[test]
    fn test_identifier_separation() {
        let input = "return value;";
        let result = minify_js(input);
        assert_eq!(result, "return value;");
    }

    #[test]
    fn test_increment_separation() {
        let input = "x + + y";
        let result = minify_js(input);
        assert_eq!(result, "x+ +y");
    }

    #[test]
    fn test_function_minification() {
        let input = "function foo(a, b) {\n  return a + b;\n}";
        let result = minify_js(input);
        assert_eq!(result, "function foo(a,b){return a+b;}");
    }

    #[test]
    fn test_preserves_string_with_slashes() {
        let input = r#"var x = "http://example.com";"#;
        let result = minify_js(input);
        assert!(result.contains("\"http://example.com\""));
    }

    #[test]
    fn test_nested_template_literal() {
        let input = "var x = `a ${`b ${c}`} d`;";
        let result = minify_js(input);
        assert!(result.contains("`a ${`b ${c}`} d`"));
    }
}
