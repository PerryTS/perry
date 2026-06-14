//! `module.exports = …` / `exports.X = …` detection and key extraction.

use super::*;

/// Issue #665: detect `module.exports = <BareIdentifier>;` patterns. Returns
/// `Some(name)` when at least one such assignment exists and every
/// `module.exports = ...` assignment in the source targets the same bare
/// identifier. Returns `None` if there are no such assignments, if multiple
/// assignments disagree, or if any assignment is to a non-identifier (object
/// literal, call, member expression, etc.) — those cases need the IIFE's
/// `module.exports` machinery to resolve correctly.
pub fn extract_single_module_exports_assignment(source: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?m)^\s*module\.exports\s*=\s*([^;\n]+?)\s*;?\s*$"#).ok()?;
    let ident_re = regex::Regex::new(r#"^[A-Za-z_$][A-Za-z0-9_$]*$"#).ok()?;
    let mut found: Option<String> = None;
    for cap in re.captures_iter(source) {
        let rhs = cap.get(1)?.as_str().trim();
        if !ident_re.is_match(rhs) {
            return None;
        }
        match &found {
            Some(prev) if prev != rhs => return None,
            Some(_) => {}
            None => found = Some(rhs.to_string()),
        }
    }
    found
}

/// Issue #665 follow-up: detect `(?:module\.)?exports\.NAME = require('SPEC')`
/// patterns and return `(name, spec)` pairs. Order is preserved and duplicates
/// (same NAME) are dropped on the first occurrence. If the same NAME also
/// appears with a non-`require(...)` RHS anywhere else in the source, the
/// pair is dropped — we don't want to forward a name that the file later
/// reassigns to a non-default-import value.
///
/// Matches both `exports.X = require('Y')` and `module.exports.X = require('Y')`.
/// Skips `__esModule` (the Babel/tsc interop marker; never user-meaningful).
pub fn extract_named_exports_from_require(source: &str) -> Vec<(String, String)> {
    let require_re = regex::Regex::new(
        r#"(?m)^\s*(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*;?\s*$"#,
    )
    .unwrap();
    // Any non-require assignment to the same `exports.X` should disqualify
    // the direct-reexport: the file is doing something more interesting and
    // we'd be skipping that runtime value if we routed through the import.
    let other_re = regex::Regex::new(
        r#"(?m)^\s*(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(.+?)\s*;?\s*$"#,
    )
    .unwrap();

    let mut found: Vec<(String, String)> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    for cap in require_re.captures_iter(source) {
        if let (Some(name), Some(spec)) = (cap.get(1), cap.get(2)) {
            let name = name.as_str().to_string();
            if name == "__esModule" {
                continue;
            }
            if seen_names.contains(&name) {
                continue;
            }
            seen_names.push(name.clone());
            found.push((name, spec.as_str().to_string()));
        }
    }
    if found.is_empty() {
        return found;
    }
    // Filter out any name that ALSO appears with a non-require RHS. Walk the
    // looser regex; if a name we matched has an RHS that doesn't start with
    // `require(`, drop the pair.
    let mut disqualified: Vec<String> = Vec::new();
    for cap in other_re.captures_iter(source) {
        if let (Some(name), Some(rhs)) = (cap.get(1), cap.get(2)) {
            let name = name.as_str();
            if seen_names.iter().any(|n| n == name) {
                let rhs = rhs.as_str().trim();
                if !rhs.starts_with("require") {
                    disqualified.push(name.to_string());
                }
            }
        }
    }
    found.retain(|(n, _)| !disqualified.contains(n));
    found
}

/// Issue #665 follow-up (object-literal aggregator): detect the published
/// `rate-limiter-flexible/index.js` shape —
///
/// ```js
/// const RateLimiterMemory = require('./lib/RateLimiterMemory');
/// const RateLimiterRedis  = require('./lib/RateLimiterRedis');
/// module.exports = {
///   RateLimiterMemory,
///   RateLimiterRedis,
///   // ...
/// };
/// ```
///
/// Returns `(exported_name, require_spec)` pairs. Shorthand `{ X }` and longhand
/// `{ X: Y }` are both supported (for longhand, the RHS identifier is what
/// gets looked up against the require-alias table). The consumer's `import
/// { X } from "pkg"` then resolves through the emitted `export { _req_N as X }`
/// directly to the leaf module's default export — which compile.rs's
/// Export::Named arm propagates class identity through, so prototype methods
/// survive the indirection.
///
/// Edge cases skipped (left for the `_cjs.X` fallback):
///   - Computed keys (`[foo]: bar`).
///   - Spreads (`...obj`).
///   - Method definitions (`X() { ... }`).
///   - RHS expressions other than a bare identifier.
///   - Any case where the alias name doesn't match a `const|let|var X = require(...)`
///     binding elsewhere in the file.
///   - Multiple `module.exports = { ... }` assignments — we only inspect the
///     last one, since later assignments overwrite earlier ones at runtime.
pub fn extract_object_literal_exports_from_require(source: &str) -> Vec<(String, String)> {
    // Locate the LAST `module.exports = {` or `exports = {` (case where the file
    // reassigns the whole exports object). Anchored at start-of-line. We use
    // `rfind`-style behavior because later assignments win at runtime.
    let header_re = regex::Regex::new(r#"(?m)^\s*(?:module\.exports|exports)\s*=\s*\{"#).unwrap();
    let last_match = header_re.find_iter(source).last();
    let m = match last_match {
        Some(m) => m,
        None => return Vec::new(),
    };
    let bytes = source.as_bytes();
    // The `{` is the last char of the match.
    let mut p = m.end() - 1;
    if p >= bytes.len() || bytes[p] != b'{' {
        return Vec::new();
    }
    // Brace-balanced scan to find the matching `}`.
    let body_start = p + 1;
    let mut depth: i32 = 1;
    p = body_start;
    while p < bytes.len() && depth > 0 {
        match bytes[p] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'"' | b'\'' => {
                let quote = bytes[p];
                p += 1;
                while p < bytes.len() && bytes[p] != quote {
                    if bytes[p] == b'\\' && p + 1 < bytes.len() {
                        p += 2;
                        continue;
                    }
                    p += 1;
                }
            }
            b'`' => {
                p += 1;
                while p < bytes.len() && bytes[p] != b'`' {
                    if bytes[p] == b'\\' && p + 1 < bytes.len() {
                        p += 2;
                        continue;
                    }
                    p += 1;
                }
            }
            b'/' if p + 1 < bytes.len() && bytes[p + 1] == b'/' => {
                p += 2;
                while p < bytes.len() && bytes[p] != b'\n' {
                    p += 1;
                }
            }
            b'/' if p + 1 < bytes.len() && bytes[p + 1] == b'*' => {
                p += 2;
                while p + 1 < bytes.len() && !(bytes[p] == b'*' && bytes[p + 1] == b'/') {
                    p += 1;
                }
                if p + 1 < bytes.len() {
                    p += 2;
                }
            }
            _ => {}
        }
        if depth == 0 {
            break;
        }
        p += 1;
    }
    if depth != 0 || p <= body_start {
        return Vec::new();
    }
    let body = match std::str::from_utf8(&bytes[body_start..p]) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Build alias -> spec map from `const|let|var X = require('Y')` bindings.
    let alias_re = regex::Regex::new(
        r#"(?m)^\s*(?:var|const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*;?"#,
    )
    .unwrap();
    let mut alias_to_spec: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for cap in alias_re.captures_iter(source) {
        if let (Some(name), Some(spec)) = (cap.get(1), cap.get(2)) {
            // First binding wins (matches JS hoisting / shadowing semantics).
            alias_to_spec
                .entry(name.as_str().to_string())
                .or_insert_with(|| spec.as_str().to_string());
        }
    }
    if alias_to_spec.is_empty() {
        return Vec::new();
    }

    // Split body into top-level entries (comma-separated, brace-balanced).
    let mut entries: Vec<String> = Vec::new();
    let body_bytes = body.as_bytes();
    let mut entry_start = 0usize;
    let mut bdepth: i32 = 0;
    let mut q = 0usize;
    while q < body_bytes.len() {
        match body_bytes[q] {
            b'{' | b'[' | b'(' => bdepth += 1,
            b'}' | b']' | b')' => bdepth -= 1,
            b'"' | b'\'' => {
                let quote = body_bytes[q];
                q += 1;
                while q < body_bytes.len() && body_bytes[q] != quote {
                    if body_bytes[q] == b'\\' && q + 1 < body_bytes.len() {
                        q += 2;
                        continue;
                    }
                    q += 1;
                }
            }
            b'`' => {
                q += 1;
                while q < body_bytes.len() && body_bytes[q] != b'`' {
                    if body_bytes[q] == b'\\' && q + 1 < body_bytes.len() {
                        q += 2;
                        continue;
                    }
                    q += 1;
                }
            }
            b'/' if q + 1 < body_bytes.len() && body_bytes[q + 1] == b'/' => {
                while q < body_bytes.len() && body_bytes[q] != b'\n' {
                    q += 1;
                }
                continue;
            }
            b'/' if q + 1 < body_bytes.len() && body_bytes[q + 1] == b'*' => {
                q += 2;
                while q + 1 < body_bytes.len()
                    && !(body_bytes[q] == b'*' && body_bytes[q + 1] == b'/')
                {
                    q += 1;
                }
                if q + 1 < body_bytes.len() {
                    q += 2;
                }
                continue;
            }
            b',' if bdepth == 0 => {
                let entry = body[entry_start..q].trim().to_string();
                if !entry.is_empty() {
                    entries.push(entry);
                }
                entry_start = q + 1;
            }
            _ => {}
        }
        q += 1;
    }
    let tail = body[entry_start..].trim().to_string();
    if !tail.is_empty() {
        entries.push(tail);
    }

    // Parse each entry as shorthand `X` or longhand `X: Y` (Y must be a bare ident).
    let shorthand_re = regex::Regex::new(r#"^[A-Za-z_$][A-Za-z0-9_$]*$"#).unwrap();
    let longhand_re =
        regex::Regex::new(r#"^([A-Za-z_$][A-Za-z0-9_$]*)\s*:\s*([A-Za-z_$][A-Za-z0-9_$]*)$"#)
            .unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in entries {
        // Strip trailing line/block comments and the trailing comma we might
        // have included.
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if shorthand_re.is_match(entry) {
            if entry == "__esModule" {
                continue;
            }
            if let Some(spec) = alias_to_spec.get(entry) {
                if seen.insert(entry.to_string()) {
                    out.push((entry.to_string(), spec.clone()));
                }
            }
        } else if let Some(cap) = longhand_re.captures(entry) {
            let key = cap.get(1).unwrap().as_str();
            let val = cap.get(2).unwrap().as_str();
            if key == "__esModule" {
                continue;
            }
            if let Some(spec) = alias_to_spec.get(val) {
                if seen.insert(key.to_string()) {
                    out.push((key.to_string(), spec.clone()));
                }
            }
        }
        // Anything else (computed keys, spreads, methods, expressions) is
        // intentionally skipped — those need the `_cjs.X` runtime path.
    }
    out
}

/// Extract named-export patterns from CJS source. Three shapes are matched:
///
///   1. `exports.X = ...` and `module.exports.X = ...` — the canonical CJS
///      named-export form. Skips `__esModule` (the interop marker injected
///      by Babel/TypeScript that consumers use to detect "this is a CJS
///      module pretending to be ESM" — we don't want to re-export a boolean
///      as if it were a named binding).
///   2. `module.exports = { X, Y, fn: someFn }` — object-literal assignment
///      to `module.exports`. Issue #624: the synthetic-package shape that
///      hand-written CJS code typically uses (and that React's transpiled
///      output occasionally falls back to) was unsupported, so the consumer
///      `import { X } from "pkg"` link-failed because no named export was
///      ever extracted.
pub fn extract_exports_from_source(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let push_unique = |names: &mut Vec<String>, name: &str| {
        if name == "__esModule" {
            return;
        }
        // Issue #845: skip JS reserved words. `export const default = _cjs.default;`
        // (and other reserved-word forms) is invalid syntax — the named-export
        // emission emits `export const <NAME> = _cjs.<NAME>;`, which fails to
        // parse if `<NAME>` isn't a valid binding identifier. `default` is the
        // common real-world case (pino: `module.exports.default = pino` —
        // ESM-interop convention). The default export is already covered by
        // the separate `export default _cjs;` statement, so skipping `default`
        // here doesn't lose any export. Reserved words other than `default`
        // are extremely rare as CJS export names but would parse-fail the
        // same way, so filter them all.
        if is_js_reserved_word(name) {
            return;
        }
        let owned = name.to_string();
        if !names.contains(&owned) {
            names.push(owned);
        }
    };

    // Shape 1: `exports.X = ...` / `module.exports.X = ...`
    // The boundary class excludes `.` so `e.exports.X = …` (a property write on
    // some OTHER object — e.g. a webpack/ncc inner module's own exports param,
    // as in next/dist/compiled/p-queue's `e.exports.TimeoutError = TimeoutError`)
    // is NOT mistaken for a named export of the outer bundle. A false positive
    // here makes the wrap emit `export const X = _cjs.X;` at module scope,
    // which shadows the inner binding of the same name during lowering and
    // turns every inner reference to it into `undefined`.
    let dot_re = regex::Regex::new(
        r"(?:^|[^A-Za-z0-9_$.])(?:module\.)?exports\.([A-Za-z_$][A-Za-z0-9_$]*)\s*=",
    )
    .unwrap();
    for cap in dot_re.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            push_unique(&mut names, m.as_str());
        }
    }

    // Shape 2: `module.exports = { ... }` — extract every key from the
    // object literal body. Brace-balanced scan because the body may contain
    // nested braces (`module.exports = { fn: function() {} }`). Two key
    // forms are recognized:
    //   - `name` (shorthand: `{ createContext }` ≡ `{ createContext: createContext }`)
    //   - `name: <expr>` (explicit: `{ createContext: createContext }` or `{ name: function() {} }`)
    // String-keyed entries (`"name": …`) and computed-key entries
    // (`[expr]: …`) are intentionally skipped — those don't surface as ESM
    // named exports anyway.
    let bytes = source.as_bytes();
    let mut search_from = 0usize;
    while let Some(idx) = source[search_from..].find("module.exports") {
        let abs = search_from + idx;
        // Skip a `module.exports = { … }` that is in EXPRESSION position rather
        // than at a statement boundary. The dominant case is the
        // `0 && (module.exports = { X: null, Y: null })` idiom that Babel /
        // TypeScript emit as a DEAD type-only export hint (the values are `null`
        // placeholders; the real exports are installed separately, e.g. via
        // `_export(exports, { X: () => X })` getters). Treating the
        // placeholder's keys as named exports overrode the real getter exports
        // with `undefined` (Next.js `built/pages` → `PagesNormalizers`
        // undefined → "undefined is not a constructor").
        //
        // A real top-level assignment is fine even when it's not at column 0
        // (`exports.a = 1; module.exports = { b, c }`) — accept it at a true
        // statement boundary. The dead `0 && (module.exports = …)` hint (and
        // its newline-split form `0 && (\n  module.exports = …\n)`) sits in
        // EXPRESSION position, so the nearest preceding non-whitespace token —
        // searched ACROSS newlines, not just the current line — is `(` (or an
        // operator), never a statement terminator.
        let prefix = &source[..abs];
        let prev_token = prefix.bytes().rev().find(|b| !b.is_ascii_whitespace());
        // Statement terminator / block brace / start-of-file → real statement.
        let stmt_boundary = matches!(prev_token, None | Some(b';') | Some(b'}') | Some(b'{'));
        // Operator / open-bracket / comma → the assignment continues an
        // expression (`0 && (…`, `x =`, `a,`), so it is NOT a real export.
        let expr_continuation = matches!(
            prev_token,
            Some(b'(' | b'&' | b'|' | b',' | b'=' | b'?' | b':' | b'+' | b'-' | b'*' | b'<' | b'>')
        );
        // A clean column-0 / ASI line start is also acceptable (e.g. after a
        // block comment) — but only when the previous token isn't a dangling
        // expression continuation.
        let line_start_ok = prefix
            .bytes()
            .rev()
            .take_while(|&b| b != b'\n')
            .all(|b| b == b' ' || b == b'\t');
        let accept = stmt_boundary || (line_start_ok && !expr_continuation);
        if !accept {
            search_from = abs + 1;
            continue;
        }
        // Skip past `module.exports`
        let mut p = abs + "module.exports".len();
        // Skip whitespace
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\n') {
            p += 1;
        }
        // Must be `=` (not `.`, `==`, etc.)
        if p >= bytes.len() || bytes[p] != b'=' {
            search_from = abs + 1;
            continue;
        }
        // Reject `==` / `===`
        if p + 1 < bytes.len() && bytes[p + 1] == b'=' {
            search_from = abs + 1;
            continue;
        }
        p += 1;
        // Skip whitespace
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t' || bytes[p] == b'\n') {
            p += 1;
        }
        // Must be `{`
        if p >= bytes.len() || bytes[p] != b'{' {
            search_from = abs + 1;
            continue;
        }
        // Brace-balanced scan to find the matching close.
        let body_start = p + 1;
        let mut depth: i32 = 1;
        let mut q = body_start;
        while q < bytes.len() && depth > 0 {
            match bytes[q] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            q += 1;
        }
        if depth != 0 {
            // Unbalanced — bail out, advance and continue scanning.
            search_from = abs + 1;
            continue;
        }
        let body_end = q - 1; // points at the closing `}`
        let body = &source[body_start..body_end];
        extract_object_literal_keys(body, &mut |name| push_unique(&mut names, name));
        search_from = q;
    }

    names
}

/// Extract top-level keys from an object-literal body (the text between
/// `{` and `}`, exclusive). Skips nested braces / brackets / parens so
/// `fn: function() { return 1; }` doesn't pull `return` as a key. Calls
/// `out` with each shorthand or `name:` key encountered at depth 0.
pub fn extract_object_literal_keys(body: &str, out: &mut dyn FnMut(&str)) {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut at_entry_start = true;
    let mut depth: i32 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' | b'[' | b'(' => {
                depth += 1;
                at_entry_start = false;
                i += 1;
            }
            b'}' | b']' | b')' => {
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => {
                at_entry_start = true;
                i += 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
            }
            _ if depth == 0 && at_entry_start => {
                // Try to read an identifier at the start of an entry.
                if (b as char).is_ascii_alphabetic() || b == b'_' || b == b'$' {
                    let start = i;
                    while i < bytes.len() {
                        let c = bytes[i];
                        if (c as char).is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let name = &body[start..i];
                    // Skip whitespace after the name.
                    let mut j = i;
                    while j < bytes.len()
                        && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n')
                    {
                        j += 1;
                    }
                    // Accept shorthand (`,` / end-of-body) or explicit key (`:`).
                    if j == bytes.len() || bytes[j] == b',' || bytes[j] == b':' {
                        out(name);
                    }
                    at_entry_start = false;
                } else {
                    // Non-identifier at entry start (e.g. `"key":` string,
                    // `[expr]:` computed, `...spread`) — skip; not an ESM
                    // exportable name.
                    at_entry_start = false;
                    i += 1;
                }
            }
            _ => {
                at_entry_start = false;
                i += 1;
            }
        }
    }
}
