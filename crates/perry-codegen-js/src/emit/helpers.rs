/// Check if a string is a valid JavaScript identifier
pub(crate) fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Generate a short variable name from a counter value.
///
/// Produces: a, b, ..., z, A, ..., Z, aa, ab, ..., az, aA, ..., aZ, ba, ...
/// Uses bijective base-52 encoding (a-z, A-Z).
pub(crate) fn gen_short_name(n: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let base = CHARS.len(); // 52
    let mut result = Vec::new();
    let mut val = n;
    loop {
        result.push(CHARS[val % base] as char);
        if val < base {
            break;
        }
        val = val / base - 1;
    }
    result.reverse();
    result.into_iter().collect()
}

/// Check if a string is a JavaScript reserved word.
pub(crate) fn is_js_reserved(s: &str) -> bool {
    matches!(
        s,
        "do" | "if"
            | "in"
            | "for"
            | "let"
            | "new"
            | "try"
            | "var"
            | "case"
            | "else"
            | "enum"
            | "null"
            | "this"
            | "true"
            | "void"
            | "with"
            | "break"
            | "catch"
            | "class"
            | "const"
            | "false"
            | "super"
            | "throw"
            | "while"
            | "yield"
            | "delete"
            | "export"
            | "import"
            | "return"
            | "switch"
            | "typeof"
            | "default"
            | "extends"
            | "finally"
            | "continue"
            | "debugger"
            | "function"
            | "arguments"
            | "instanceof"
            | "of"
    )
}
