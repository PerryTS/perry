//! `require(...)` specifier extraction and alias detection.

#[allow(unused_imports)]
use super::*;

/// Extract `require('X')` / `require("X")` specifiers, preserving order and
/// deduping. Only matches static string literal arguments — dynamic
/// `require(someVar)` is unrepresentable as ESM and the bound `require`
/// inside the IIFE will throw at runtime if hit.
pub fn extract_require_specifiers(source: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let mut specs = Vec::new();
    for cap in re.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str().to_string();
            if !specs.contains(&s) {
                specs.push(s);
            }
        }
    }
    specs
}

/// Refs #488 drizzle-sqlite: extract `var <alias> = require("<spec>");`
/// declarations from the source as `(alias_name, spec, (start_byte,
/// end_byte))`. The byte range covers the whole matched statement so
/// `wrap_commonjs` can blank it from the IIFE body — leaving the binding
/// only at module scope where the wrap emits `const <alias> = _req_N;`,
/// so hoisted class declarations' `extends <alias>.Y` resolve correctly
/// without the inner `var` re-binding shadowing the outer alias when the
/// IIFE evaluates.
///
/// Matches `var` / `const` / `let`. Order is preserved and duplicates
/// are dropped on the alias name (the first binding wins — matches JS
/// hoisting semantics for the original source).
///
/// Issue #845: the trailing `\s*(?:;|$)` (require a semicolon or
/// end-of-line in multiline mode) is intentional. Without it,
/// `const EventEmitter = require('events').EventEmitter;` matches as
/// `const EventEmitter = require('events')` and the blanking pass at
/// line 336 above leaves `.EventEmitter;` dangling at column 0 of the
/// wrapped output, producing a TS1109 ("Expression expected") parse
/// failure 1000+ bytes past EOF. Only whole-statement aliases (those
/// whose require call is followed by `;` or end-of-line) are safe to
/// blank — anything with `.X` trailing member access binds to the
/// property, not the module object, so the alias-rename pass would
/// be wrong anyway. Same-line follow-on statements like
/// `var dep = require('./dep'); module.exports = dep.value;` still
/// match because the `;` form ends the alias matched region before
/// the follow-on.
pub fn extract_require_aliases_with_ranges(source: &str) -> Vec<(String, String, (usize, usize))> {
    let re = regex::Regex::new(
        r#"(?m)^\s*(?:var|const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*(?:;|$)"#,
    )
    .unwrap();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for cap in re.captures_iter(source) {
        if let (Some(alias), Some(spec), Some(whole)) = (cap.get(1), cap.get(2), cap.get(0)) {
            let alias = alias.as_str().to_string();
            if seen.contains(&alias) {
                continue;
            }
            seen.push(alias.clone());
            out.push((
                alias,
                spec.as_str().to_string(),
                (whole.start(), whole.end()),
            ));
        }
    }
    out
}
