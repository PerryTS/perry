//! `require(...)` specifier extraction and alias detection.

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

/// Does the CJS source declare a binding named `name` via `var`/`let`/`const`/
/// `function`/`class` anywhere in the body? Used by the named-export emission to
/// decide whether `name` is a real module binding (so `export const name =
/// _cjs.name;` is safe) or merely an object-literal export KEY whose value is a
/// global/expression — in which case emitting a module-scope `const name` would
/// SHADOW a global builtin that the body references freely.
///
/// Concretely: bluebird's `errors.js` ends with `module.exports = { Error:
/// Error, TypeError: _TypeError, ... }`. The keys `Error`/`TypeError`/
/// `RangeError` are JS global builtins; the body has no `function Error` /
/// `var Error` etc. Emitting `export const Error = _cjs.Error;` introduced a
/// module-scope `Error` that shadowed the global for the body's
/// `inherits(SubError, Error)` (an IIFE-local free reference) — `Error` read
/// `undefined` and `Parent.prototype` threw `Cannot read properties of
/// undefined (reading 'prototype')`. This helper lets the caller skip the
/// shadowing `const` for such names.
///
/// Heuristic, regex-crate-friendly (no lookaround): whole-word scan for `name`
/// preceded (modulo whitespace) by a `var`/`let`/`const`/`function`/`class`
/// keyword. Member-access positions (`x.name`) and `name`-as-a-substring are
/// excluded. A false positive (treating a non-declared name as declared) only
/// reverts to the prior `export const` behavior; a false negative (treating a
/// declared name as undeclared) only forfeits a named export's module-scope
/// binding while still surfacing the value via `_cjs.name`.
pub fn identifier_is_declared_binding(source: &str, name: &str) -> bool {
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
        // Preceding identifier token must be a binding keyword.
        let mut w = p;
        while w > 0 && is_ident(bytes[w - 1]) {
            w -= 1;
        }
        if matches!(
            &source[w..p],
            "var" | "let" | "const" | "function" | "class"
        ) {
            return true;
        }
    }
    false
}

/// Next.js lazy-require classification (single forward pass). Returns the set
/// of specifiers whose EVERY `require('<spec>')` call site is lexically inside
/// a FUNCTION body — never at module top level, and never inside a top-level
/// control-flow block that runs at module load. Node loads such a module
/// lazily (only when the enclosing function runs), so Perry must not eager-init
/// it.
///
/// Conservative by construction: a spec with any top-level call site (including
/// top-level `if`/`for`/`try` blocks, which execute during module evaluation)
/// is excluded and keeps the default eager behavior. A misclassification is
/// self-correcting at runtime — the require shim triggers the target's init
/// when `require()` is actually called — so this only governs eager-init-loop
/// membership.
///
/// Brace/paren scanning runs on a comment/string/regex-masked copy (same
/// length, code structure preserved) so literal braces never corrupt the scope
/// stack. Call-site offsets + specifiers come from the original source.
pub fn function_local_specs(source: &str) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};

    // (offset, spec) for every static `require('<spec>')` call, in source order.
    let re = regex::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let sbytes = source.as_bytes();
    let mut sites: Vec<(usize, &str)> = Vec::new();
    for cap in re.captures_iter(source) {
        let m0 = cap.get(0).unwrap();
        // Skip member-access matches (`foo.require('x')`).
        let mut p = m0.start();
        while p > 0 && (sbytes[p - 1] as char).is_whitespace() {
            p -= 1;
        }
        if p > 0 && sbytes[p - 1] == b'.' {
            continue;
        }
        sites.push((m0.start(), cap.get(1).unwrap().as_str()));
    }
    if sites.is_empty() {
        return HashSet::new();
    }

    let masked = super::detect::strip_comments_and_strings(source);
    let mbytes = masked.as_bytes();
    let is_ident = |c: u8| c == b'_' || c == b'$' || c.is_ascii_alphanumeric();
    let control_keywords = ["if", "for", "while", "switch", "catch", "with", "else"];

    #[derive(PartialEq)]
    enum Scope {
        Function,
        Block,
    }
    let mut scopes: Vec<Scope> = Vec::new();
    // spec → (seen any site, all sites so far in-function).
    let mut state: HashMap<&str, (bool, bool)> = HashMap::new();
    let mut next_site = 0usize;
    let in_function = |scopes: &[Scope]| scopes.contains(&Scope::Function);

    let mut i = 0usize;
    while i < mbytes.len() {
        // Record any require site at this offset before processing the char.
        while next_site < sites.len() && sites[next_site].0 == i {
            let (_, spec) = sites[next_site];
            let here = in_function(&scopes);
            let e = state.entry(spec).or_insert((false, true));
            e.0 = true;
            e.1 = e.1 && here;
            next_site += 1;
        }
        match mbytes[i] {
            b'{' => {
                let mut p = i;
                while p > 0 && (mbytes[p - 1] as char).is_whitespace() {
                    p -= 1;
                }
                let kind = if p >= 2 && &masked[p - 2..p] == "=>" {
                    Scope::Function
                } else if p > 0 && mbytes[p - 1] == b')' {
                    let head = matched_open_head(&masked, mbytes, p - 1, &is_ident);
                    if control_keywords.iter().any(|k| *k == head) {
                        Scope::Block
                    } else {
                        // `function f(...) {`, method `m(...) {`, arrow
                        // `(...) => {` (caught above), IIFE `(...)(...) {`…
                        Scope::Function
                    }
                } else {
                    Scope::Block
                };
                scopes.push(kind);
            }
            b'}' => {
                scopes.pop();
            }
            _ => {}
        }
        i += 1;
    }
    // Any sites at EOF offset (defensive).
    while next_site < sites.len() {
        let (_, spec) = sites[next_site];
        let e = state.entry(spec).or_insert((false, true));
        e.0 = true;
        e.1 = e.1 && in_function(&scopes);
        next_site += 1;
    }

    state
        .into_iter()
        .filter_map(|(spec, (seen, all_in_fn))| {
            if seen && all_in_fn {
                Some(spec.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Given the index of a `)` in the masked source, walk back to its matching
/// `(` and return the identifier/keyword immediately before that `(`.
fn matched_open_head(
    masked: &str,
    mbytes: &[u8],
    close_paren: usize,
    is_ident: &impl Fn(u8) -> bool,
) -> String {
    let mut depth = 0i32;
    let mut i = close_paren;
    loop {
        match mbytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    let mut p = i;
                    while p > 0 && (mbytes[p - 1] as char).is_whitespace() {
                        p -= 1;
                    }
                    let end = p;
                    while p > 0 && is_ident(mbytes[p - 1]) {
                        p -= 1;
                    }
                    return masked[p..end].to_string();
                }
            }
            _ => {}
        }
        if i == 0 {
            return String::new();
        }
        i -= 1;
    }
}
