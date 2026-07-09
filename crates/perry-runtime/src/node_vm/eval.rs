//! The deterministic mini-interpreter behind `vm.Script` / `runIn*Context` /
//! `compileFunction`: top-level statement splitting, a small expression
//! evaluator (logical/equality/arithmetic operators, `typeof`, literals,
//! property paths), and reference reads/writes against the sandbox target.
//! Perry is V8-free — this models only the local subsets the VM parity
//! fixtures exercise. Extracted from `node_vm.rs` to keep that file under
//! the 2000-line cap; behavior is unchanged.

use super::*;

fn split_top_level(input: &str, delimiter: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quote = None::<char>;
    let mut escape = false;
    for (idx, ch) in input.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                out.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(input[start..].trim());
    out
}

fn strip_wrapping_parens(mut s: &str) -> &str {
    loop {
        let t = s.trim();
        if !(t.starts_with('(') && t.ends_with(')')) {
            return t;
        }
        let mut depth = 0_i32;
        let mut quote = None::<char>;
        let mut escape = false;
        let mut wraps = true;
        for (idx, ch) in t.char_indices() {
            if let Some(q) = quote {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == q {
                    quote = None;
                }
                continue;
            }
            match ch {
                '\'' | '"' | '`' => quote = Some(ch),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != t.len() - 1 {
                        wraps = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !wraps {
            return t;
        }
        s = &t[1..t.len() - 1];
    }
}

fn find_top_level_operator(input: &str, op: &str) -> Option<usize> {
    let mut depth = 0_i32;
    let mut quote = None::<char>;
    let mut escape = false;
    let mut found = None;
    for (idx, ch) in input.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if depth == 0 && input[idx..].starts_with(op) => found = Some(idx),
            _ => {}
        }
    }
    found
}

fn unquote(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0] as char;
    if !matches!(q, '\'' | '"' | '`') || bytes[bytes.len() - 1] as char != q {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    Some(
        inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\\\\", "\\"),
    )
}

fn value_to_number(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else if jv.is_bool() {
        if jv.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jv.is_null() {
        0.0
    } else {
        f64::NAN
    }
}

fn coerce_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    rust_string_from_header(ptr).unwrap_or_default()
}

fn add_values(a: f64, b: f64) -> f64 {
    let aj = JSValue::from_bits(a.to_bits());
    let bj = JSValue::from_bits(b.to_bits());
    if aj.is_any_string() || bj.is_any_string() {
        return string_value(&format!("{}{}", coerce_to_string(a), coerce_to_string(b)));
    }
    number_value(value_to_number(a) + value_to_number(b))
}

fn value_same(a: f64, b: f64) -> bool {
    crate::value::js_jsvalue_equals(a, b) != 0
}

fn get_reference(name: &str, env: &EvalEnv) -> f64 {
    match name {
        "undefined" => undefined_value(),
        "null" => f64::from_bits(JSValue::null().bits()),
        "true" => bool_value(true),
        "false" => bool_value(false),
        "globalThis" | "this" => env.target,
        _ => env
            .params
            .get(name)
            .copied()
            .unwrap_or_else(|| get_object_field(env.target, name)),
    }
}

fn eval_property_path(expr: &str, env: &EvalEnv) -> Option<f64> {
    let expr = expr.trim();
    // Peel a trailing computed accessor (`a.b["k"]`) and read through it after
    // resolving the receiver expression. Recurses so chained accessors work.
    if let Some((object_expr, accessor)) = split_trailing_computed(expr) {
        let object = eval_property_path(object_expr, env)?;
        let key = computed_key_name(accessor, env)?;
        return Some(get_object_field(object, &key));
    }
    let mut parts = expr.split('.');
    let first = parts.next()?.trim();
    if first.is_empty() {
        return None;
    }
    let mut value = get_reference(first, env);
    for part in parts {
        let name = part.trim();
        if name.is_empty() {
            return None;
        }
        value = get_object_field(value, name);
    }
    Some(value)
}

/// Split a trailing computed-member accessor off a member-access expression,
/// e.g. `globalThis.M["/a/b"]` -> (`globalThis.M`, `["/a/b"]`). Returns `None`
/// when the expression does not end in a top-level `[...]` accessor.
fn split_trailing_computed(expr: &str) -> Option<(&str, &str)> {
    let trimmed = expr.trim_end();
    if !trimmed.ends_with(']') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut depth = 0_i32;
    let mut quote = None::<u8>;
    let mut open = None;
    for idx in (0..bytes.len()).rev() {
        let ch = bytes[idx];
        if let Some(q) = quote {
            // Walking backwards through a quoted span: a quote char that is not
            // backslash-escaped closes (opens, in reverse) the span.
            if ch == q && (idx == 0 || bytes[idx - 1] != b'\\') {
                quote = None;
            }
            continue;
        }
        match ch {
            b'\'' | b'"' | b'`' => quote = Some(ch),
            b']' => depth += 1,
            b'[' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let open = open?;
    if open == 0 {
        return None;
    }
    Some((trimmed[..open].trim(), &trimmed[open..]))
}

/// Evaluate the key inside a `[...]` accessor to a property name string.
fn computed_key_name(accessor: &str, env: &EvalEnv) -> Option<String> {
    let inner = accessor.trim();
    let inner = inner.strip_prefix('[')?.strip_suffix(']')?.trim();
    let value = eval_expr(inner, env);
    Some(coerce_to_string(value))
}

fn set_reference(lhs: &str, value: f64, env: &mut EvalEnv) {
    let lhs = lhs.trim();
    if let Some((object_expr, accessor)) = split_trailing_computed(lhs) {
        if let (Some(object), Some(key)) = (
            eval_property_path(object_expr, env),
            computed_key_name(accessor, env),
        ) {
            set_object_field(object, &key, value);
        }
        return;
    }
    if let Some((head, tail)) = lhs.rsplit_once('.') {
        if let Some(object) = eval_property_path(head, env) {
            set_object_field(object, tail.trim(), value);
        }
        return;
    }
    if env.params.contains_key(lhs) {
        env.params.insert(lhs.to_string(), value);
    } else {
        set_object_field(env.target, lhs, value);
    }
}

/// Rewrite a JS object/array literal into strict JSON so the runtime JSON
/// parser can build it. Next.js serializes the RSC manifest payload with
/// `JSON.stringify` (already strict JSON), but the broader contract accepts
/// plain object literals too, so we quote bare identifier keys (`{a:1}` ->
/// `{"a":1}`) and normalize single-quoted strings to double-quoted. Returns
/// `None` if the text is not a well-formed literal we can normalize.
fn normalize_literal_to_json(expr: &str) -> Option<String> {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + 8);
    let mut i = 0;
    // Tracks whether the next bare-identifier run is in a key position (right
    // after `{` or a `,` while inside an object). The brace stack records the
    // kind of each open container: `true` = object, `false` = array.
    let mut object_stack: Vec<bool> = Vec::new();
    let mut expect_key = false;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\n' | b'\r' => {
                out.push(ch as char);
                i += 1;
            }
            b'"' | b'\'' => {
                // Copy a quoted string, re-emitting as a double-quoted JSON
                // string. Track escapes so a quote inside the string doesn't end
                // it early.
                let quote = ch;
                out.push('"');
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'\\' && i + 1 < bytes.len() {
                        // `\'` is valid inside a JS single-quoted string but is
                        // NOT a legal JSON escape, so it would make an otherwise
                        // valid literal like `{name: 'can\'t'}` fail JSON
                        // parsing. Emit a plain apostrophe; pass every
                        // JSON-valid escape (\\, \", \n, …) through unchanged.
                        if bytes[i + 1] == b'\'' {
                            out.push('\'');
                        } else {
                            out.push('\\');
                            out.push(bytes[i + 1] as char);
                        }
                        i += 2;
                        continue;
                    }
                    if c == quote {
                        i += 1;
                        break;
                    }
                    if c == b'"' {
                        out.push('\\');
                    }
                    out.push(c as char);
                    i += 1;
                }
                out.push('"');
                expect_key = false;
            }
            b'{' => {
                out.push('{');
                object_stack.push(true);
                expect_key = true;
                i += 1;
            }
            b'[' => {
                out.push('[');
                object_stack.push(false);
                expect_key = false;
                i += 1;
            }
            b'}' | b']' => {
                out.push(ch as char);
                object_stack.pop();
                expect_key = false;
                i += 1;
            }
            b',' => {
                out.push(',');
                expect_key = object_stack.last().copied().unwrap_or(false);
                i += 1;
            }
            b':' => {
                out.push(':');
                expect_key = false;
                i += 1;
            }
            c if c == b'_' || c == b'$' || c.is_ascii_alphabetic() => {
                // Bare identifier run. In key position, JSON-quote it. As a value
                // it can only be a literal keyword (true/false/null); anything
                // else (a context reference) is outside what we normalize here.
                let start = i;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'_' || c == b'$' || c.is_ascii_alphanumeric() {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let ident = &expr[start..i];
                if expect_key {
                    out.push('"');
                    out.push_str(ident);
                    out.push('"');
                    expect_key = false;
                } else if matches!(ident, "true" | "false" | "null") {
                    out.push_str(ident);
                } else {
                    return None;
                }
            }
            _ => {
                out.push(ch as char);
                i += 1;
            }
        }
    }
    Some(out)
}

/// Parse an object/array literal expression into a value. Tries strict JSON
/// first (the common Next.js manifest case), then a lenient JS-literal->JSON
/// normalization. Returns `None` when the text is not a literal we can build.
fn eval_object_or_array_literal(expr: &str) -> Option<f64> {
    let trimmed = expr.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let attempt = |text: &str| -> Option<f64> {
        let ptr = string_ptr(text);
        match unsafe { crate::json::js_json_parse_result(ptr) } {
            Ok(value) => Some(f64::from_bits(value.bits())),
            Err(_) => None,
        }
    };
    if let Some(value) = attempt(trimmed) {
        return Some(value);
    }
    let normalized = normalize_literal_to_json(trimmed)?;
    attempt(&normalized)
}

fn is_truthy(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}

fn eval_expr(expr: &str, env: &EvalEnv) -> f64 {
    let expr = strip_wrapping_parens(expr);
    if expr.is_empty() {
        return undefined_value();
    }
    // Logical operators bind looser than comparison/arithmetic, so resolve them
    // first. `find_top_level_operator` returns the last top-level occurrence,
    // which yields correct left-associative short-circuit grouping.
    if let Some(idx) = find_top_level_operator(expr, "||") {
        let left = eval_expr(&expr[..idx], env);
        if is_truthy(left) {
            return left;
        }
        return eval_expr(&expr[idx + 2..], env);
    }
    if let Some(idx) = find_top_level_operator(expr, "&&") {
        let left = eval_expr(&expr[..idx], env);
        if !is_truthy(left) {
            return left;
        }
        return eval_expr(&expr[idx + 2..], env);
    }
    if let Some(idx) = find_top_level_operator(expr, "===") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 3..], env);
        return bool_value(value_same(left, right));
    }
    if let Some(idx) = find_top_level_operator(expr, "!==") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 3..], env);
        return bool_value(!value_same(left, right));
    }
    if let Some(idx) = find_top_level_operator(expr, "+") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 1..], env);
        return add_values(left, right);
    }
    if let Some(idx) = find_top_level_operator(expr, "-") {
        if idx > 0 {
            let left = eval_expr(&expr[..idx], env);
            let right = eval_expr(&expr[idx + 1..], env);
            return number_value(value_to_number(left) - value_to_number(right));
        }
    }
    if let Some(rest) = expr.strip_prefix("typeof ") {
        let value = eval_expr(rest, env);
        let ptr = crate::builtins::js_value_typeof(value);
        return f64::from_bits(JSValue::string_ptr(ptr).bits());
    }
    if let Some(s) = unquote(expr) {
        return string_value(&s);
    }
    if let Ok(n) = expr.parse::<f64>() {
        return number_value(n);
    }
    if let Some(value) = eval_object_or_array_literal(expr) {
        return value;
    }
    eval_property_path(expr, env).unwrap_or_else(undefined_value)
}

fn execute_statement(stmt: &str, env: &mut EvalEnv) -> Option<f64> {
    let stmt = stmt.trim();
    if stmt.is_empty() {
        return Some(undefined_value());
    }
    if let Some(rest) = stmt.strip_prefix("return ") {
        return Some(eval_expr(rest, env));
    }
    let decl = ["var ", "let ", "const "]
        .iter()
        .find_map(|prefix| stmt.strip_prefix(prefix));
    if let Some(rest) = decl {
        let mut last = undefined_value();
        for part in split_top_level(rest, ',') {
            let (name, value) = if let Some((name, rhs)) = part.split_once('=') {
                (name.trim(), eval_expr(rhs, env))
            } else {
                (part.trim(), undefined_value())
            };
            if !name.is_empty() {
                set_reference(name, value, env);
                last = value;
            }
        }
        return Some(last);
    }
    for op in ["+=", "-=", "="] {
        if let Some(idx) = find_top_level_operator(stmt, op) {
            let lhs = stmt[..idx].trim();
            let rhs = stmt[idx + op.len()..].trim();
            let right = eval_expr(rhs, env);
            let value = match op {
                "+=" => add_values(eval_expr(lhs, env), right),
                "-=" => number_value(value_to_number(eval_expr(lhs, env)) - value_to_number(right)),
                _ => right,
            };
            set_reference(lhs, value, env);
            return Some(value);
        }
    }
    Some(eval_expr(stmt, env))
}

pub(super) fn run_source(source: &str, target: f64, params: HashMap<String, f64>) -> f64 {
    let mut env = EvalEnv { target, params };
    let mut last = undefined_value();
    for stmt in split_top_level(source, ';') {
        if stmt.trim().starts_with("return ") {
            return eval_expr(stmt.trim().trim_start_matches("return "), &env);
        }
        if let Some(value) = execute_statement(stmt, &mut env) {
            last = value;
        }
    }
    last
}
