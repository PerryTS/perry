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

/// Issue #4872: extract `__exportStar(require('SPEC'), exports)` re-export
/// calls — the tsc-emitted CJS lowering of `export * from 'SPEC'`. Matches
/// the bare inline-helper form (`__exportStar(require("./x"), exports)`),
/// the tslib member form (`tslib_1.__exportStar(require("./x"), exports)`),
/// and the comma-sequenced form (`(0, tslib_1.__exportStar)(require("./x"),
/// exports)`). The helper *definition* (`var __exportStar = (this && ...)`)
/// never matches because the pattern requires a `require('...')` literal as
/// the first argument. Order preserved, deduped.
pub fn extract_export_star_specs(source: &str) -> Vec<String> {
    let re = regex::Regex::new(
        r#"(?:[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*)?__exportStar\s*\)?\s*\(\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*,\s*exports\s*\)"#,
    )
    .unwrap();
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

/// Issue #5006: does `name` appear as an *assignment target* (reassignment)
/// anywhere in `source`, beyond its own declaration?
///
/// A `require()` alias is normally hoisted into an immutable module-scope ESM
/// import binding (`import s from './m'`) and its `var s = require('./m')`
/// declaration is blanked from the IIFE body (see `wrap.rs` adoption / hoist
/// strip passes). That is correct only when the binding is read-only. A module
/// that *reassigns* the alias (`s = s.filter(...)`, the canonical signal-exit
/// shape) must keep `s` as a real mutable local, so we exclude reassigned
/// aliases from both passes.
///
/// Heuristic, regex-crate-friendly (no lookaround): scan whole-word
/// occurrences of `name`, skip member accesses (`obj.name`) and the
/// `var`/`let`/`const name = ...` declaration itself, and flag the rest when
/// the next non-space token is an assignment operator (`=` that is not `==`,
/// `===`, or `=>`, or any compound `+=`/`&&=`/`>>>=`/… form). False positives
/// only forfeit an optimization (the alias stays a mutable local, which is
/// always correct); they never miscompile.
pub fn identifier_is_reassigned(source: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = source.as_bytes();
    let nlen = name.len();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    let mut from = 0usize;
    while let Some(rel) = source[from..].find(name) {
        let start = from + rel;
        let end = start + nlen;
        from = start + 1;
        // Whole-word boundaries.
        if start > 0 && is_ident(bytes[start - 1]) {
            continue;
        }
        if end < bytes.len() && is_ident(bytes[end]) {
            continue;
        }
        // Preceding non-space char: skip member access (`.name`).
        let mut p = start;
        while p > 0 && (bytes[p - 1] as char).is_whitespace() {
            p -= 1;
        }
        if p > 0 && bytes[p - 1] == b'.' {
            continue;
        }
        // Skip the `var`/`let`/`const name` declaration keyword.
        let mut w = p;
        while w > 0 && is_ident(bytes[w - 1]) {
            w -= 1;
        }
        if matches!(&source[w..p], "var" | "let" | "const") {
            continue;
        }
        // Following non-space char(s) must open an assignment operator.
        let mut q = end;
        while q < bytes.len() && (bytes[q] as char).is_whitespace() {
            q += 1;
        }
        if q >= bytes.len() {
            continue;
        }
        let rest = &source[q..];
        let is_assignment =
            if rest.starts_with("===") || rest.starts_with("==") || rest.starts_with("=>") {
                false
            } else if rest.starts_with('=') {
                true
            } else {
                // Compound assignments: `+=`, `-=`, `*=`, `/=`, `%=`, `**=`,
                // `<<=`, `>>=`, `>>>=`, `&=`, `|=`, `^=`, `&&=`, `||=`, `??=`.
                const COMPOUND: &[&str] = &[
                    ">>>=", "**=", "<<=", ">>=", "&&=", "||=", "??=", "+=", "-=", "*=", "/=", "%=",
                    "&=", "|=", "^=",
                ];
                COMPOUND.iter().any(|op| rest.starts_with(op))
            };
        if is_assignment {
            return true;
        }
    }
    false
}
