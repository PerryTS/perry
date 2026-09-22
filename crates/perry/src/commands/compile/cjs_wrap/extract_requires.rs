//! `require(...)` specifier extraction and alias detection.

/// Extract `require('X')` / `require("X")` specifiers, preserving order and
/// deduping. Only matches static string literal arguments — dynamic
/// `require(someVar)` is unrepresentable as ESM and the bound `require`
/// inside the IIFE will throw at runtime if hit.
pub fn extract_require_specifiers(source: &str) -> Vec<String> {
    // A trailing comma is valid in calls and is emitted by formatters for
    // multiline require expressions (including OpenCode's watcher selector).
    let re =
        perry_perex::tooling::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*,?\s*\)"#).unwrap();
    let masked = super::detect::strip_comments_and_strings(source);
    let mut specs = Vec::new();
    for cap in re.captures_iter(source) {
        let Some(call) = cap.get(0) else {
            continue;
        };
        // The regexp runs on the original source so it can capture the quoted
        // specifier. Require the `require` token itself to survive the
        // comment/string masking pass, though. Otherwise text such as Next's
        // `"unexpected require(" + request + ") ..."` is mistaken for a call
        // whose specifier is the intervening JavaScript expression.
        if masked.as_bytes()[call.start()..call.start() + b"require".len()] != *b"require" {
            continue;
        }
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
    let re = perry_perex::tooling::Regex::new(
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
    let re = perry_perex::tooling::Regex::new(
        r#"(?m)^\s*(?:var|const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*(?:;|$)"#,
    )
    .unwrap();
    let bytes = source.as_bytes();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for cap in re.captures_iter(source) {
        if let (Some(alias), Some(spec), Some(whole)) = (cap.get(1), cap.get(2), cap.get(0)) {
            // Comma-continued declarator lists (pre-ES6 "comma-first" style).
            //
            // The trailing `(?m)$` lets a match end at end-of-LINE, so
            //
            //     var compileSchema = require('./compile')
            //       , resolve = require('./compile/resolve')
            //       , Cache = require('./cache');
            //
            // matches declarator #0 only. Blanking that range leaves the
            // continuation `, resolve = require('./compile/resolve')` dangling
            // at statement position, which parses as TS1109 ("Expression
            // expected") — the same failure shape as the `.EventEmitter;`
            // case in issue #845, just reached via a comma instead of a member
            // access. Hit in the wild by ajv 6.x (`lib/ajv.js`,
            // `lib/compile/index.js`) via the Vercel CLI corpus.
            //
            // Skip the entire declaration: with no alias entry nothing is
            // blanked, the body keeps the original (valid) multi-declarator
            // statement, and the IIFE-bound `require` resolves each specifier
            // at runtime. The specifiers still become module-scope imports via
            // `extract_require_specifiers`; only the alias-adoption
            // optimization is forfeited. This is the same safe fallback the
            // wrap already takes when it refuses an adoption.
            //
            // The single-line form `var a = require('x'), b = 42;` never
            // matched in the first place (`\s*(?:;|$)` rejects the `,`), so
            // this check only affects the multi-line style.
            let mut p = whole.end();
            while p < bytes.len() && (bytes[p] as char).is_whitespace() {
                p += 1;
            }
            if p < bytes.len() && bytes[p] == b',' {
                continue;
            }
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

/// Deferred-require classification (single forward pass). Returns the set of
/// specifiers whose EVERY `require('<spec>')` call site is NOT guaranteed to
/// run the moment the module loads — a function body (never called, or called
/// later: #Next.js lazy-require), a control-flow block that may not run every
/// time its enclosing scope runs (`if`/`for`/`while`/`switch`/`catch`/`with`/
/// `else`/`try`/`do`/`finally`), or a braceless/operator-guarded equivalent of
/// the same thing (`cond && require(...)`, `cond ? require(...) : x`, `for
/// (...) require(...)` with no block). Node only ever loads such a module when
/// control flow actually reaches the call, so Perry must not eager-init it
/// either (issue #10437: `pg` guards its optional `pg-native` binding exactly
/// this way, behind `if (forceNative) { require('./native') }`).
///
/// An ordinary object literal (`{ key: require(...) }`), a class body, or a
/// bare grouping block do NOT count — their contents run unconditionally
/// whenever the enclosing statement/expression is reached, same as top level,
/// so nesting inside one of those must not flip a spec to lazy (that would be
/// the common `module.exports = { fs: require('fs'), path: require('path') }`
/// barrel-export shape, which really is eager).
///
/// The ternary ALTERNATE arm (`cond ? x : require(...)`) is deliberately NOT
/// matched — a bare `:` immediately before `require(` is indistinguishable
/// from an object-literal property value or a `switch` case label without a
/// real parse, and guessing wrong there risks the same barrel-export
/// misclassification the object-literal exclusion above avoids. That shape
/// keeps the conservative eager default (a known, narrow gap — not in scope
/// for #10437's reproduction).
///
/// A false POSITIVE here (treating a genuinely-unconditional require as
/// conditional) is harmless: the require shim still triggers the target's
/// init at the exact point the call is lexically reached, which for an
/// unconditional call is essentially the same moment eager pre-init would
/// have run it. A false NEGATIVE (missing a genuinely-conditional call) is
/// the actual bug class — the target loads (and can throw) before its
/// guarding condition was ever evaluated.
///
/// Brace/paren scanning runs on a comment/string/regex-masked copy (same
/// length, code structure preserved) so literal braces never corrupt the scope
/// stack. Call-site offsets + specifiers come from the original source.
pub fn function_local_specs(source: &str) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};

    // (offset, spec) for every static `require('<spec>')` call, in source order.
    let re = perry_perex::tooling::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let masked = super::detect::strip_comments_and_strings(source);
    let sbytes = source.as_bytes();
    let mut sites: Vec<(usize, &str)> = Vec::new();
    for cap in re.captures_iter(source) {
        let m0 = cap.get(0).unwrap();
        if masked.as_bytes()[m0.start()..m0.start() + b"require".len()] != *b"require" {
            continue;
        }
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

    // #10437 followup: a spec whose ONLY conditionality is a
    // `process.platform === /!== '<literal>'` if/else guard (either branch —
    // e.g. node-pty's `./windowsTerminal` / `./unixTerminal` split) must NOT
    // be downgraded to lazy by the broader control-flow classification below.
    // The platform is a build TARGET resolved at compile time, not a runtime
    // unknown — `wrap_commonjs_for_target`'s `inactive_platform_guarded_requires`
    // already prunes the dead branch's spec outright for a known target, and
    // the live branch's spec keeps the eager `_req_N` classification it had
    // before this fix. Treating a compile-time-resolved platform check as
    // conditional the way a genuinely runtime-unknown check (env var,
    // arbitrary function result) is would only add needless deferral, not
    // fix a bug — #10437 is about conditions Perry cannot resolve at compile
    // time.
    let platform_guarded_specs = process_platform_guarded_specs(source);

    let mbytes = masked.as_bytes();
    let is_ident = |c: u8| c == b'_' || c == b'$' || c.is_ascii_alphanumeric();
    let control_keywords = ["if", "for", "while", "switch", "catch", "with", "else"];
    // Bare-keyword control blocks with no parens (`try {`, `else {`, `do {`,
    // `} finally {`) — as opposed to an object literal / class body / plain
    // grouping block, whose opening `{` is also not preceded by `)`/`=>` but
    // whose contents are NOT conditional (see doc comment above).
    let bare_control_keywords = ["try", "else", "do", "finally"];

    #[derive(PartialEq)]
    enum Scope {
        /// Function/method/arrow/IIFE body: reachability depends on whether,
        /// and when, the function is ever called.
        Function,
        /// A control-flow block that may not run every time its enclosing
        /// scope runs.
        Block,
        /// Anything else brace-delimited whose contents run unconditionally
        /// when reached (object literal, class body, bare grouping block).
        /// Nesting here does not itself make an enclosed `require()`
        /// conditional.
        Other,
    }
    let mut scopes: Vec<Scope> = Vec::new();
    // spec → (seen any site, all sites so far conditionally-reached).
    let mut state: HashMap<&str, (bool, bool)> = HashMap::new();
    let mut next_site = 0usize;
    let gates_reachability = |scopes: &[Scope]| {
        scopes
            .iter()
            .any(|s| matches!(s, Scope::Function | Scope::Block))
    };

    let mut i = 0usize;
    while i < mbytes.len() {
        // Record any require site at this offset before processing the char.
        while next_site < sites.len() && sites[next_site].0 == i {
            let (_, spec) = sites[next_site];
            let conditional = !platform_guarded_specs.contains(spec)
                && (gates_reachability(&scopes)
                    || site_is_conditionally_guarded(&masked, mbytes, i, &is_ident));
            let e = state.entry(spec).or_insert((false, true));
            e.0 = true;
            e.1 = e.1 && conditional;
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
                    // Not preceded by `)` or `=>`: a bare control keyword
                    // (`try`/`else`/`do`/`finally`) is conditional; an object
                    // literal, class body, or plain grouping block is not.
                    let mut w = p;
                    while w > 0 && is_ident(mbytes[w - 1]) {
                        w -= 1;
                    }
                    if bare_control_keywords.iter().any(|k| *k == &masked[w..p]) {
                        Scope::Block
                    } else {
                        Scope::Other
                    }
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
        let conditional = !platform_guarded_specs.contains(spec)
            && (gates_reachability(&scopes)
                || site_is_conditionally_guarded(&masked, mbytes, mbytes.len(), &is_ident));
        let e = state.entry(spec).or_insert((false, true));
        e.0 = true;
        e.1 = e.1 && conditional;
        next_site += 1;
    }

    state
        .into_iter()
        .filter_map(|(spec, (seen, all_conditional))| {
            if seen && all_conditional {
                Some(spec.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Property reads in a function body cannot be inferred to happen at the
/// preceding `require()` call. Used by the circular-dependency warning scan:
/// warning there would run even when the function is called after the cycle
/// finishes. An immediately invoked function still runs during module init.
pub(super) fn deferred_function_sites(
    masked: &str,
    sites: &[usize],
) -> std::collections::HashSet<usize> {
    if sites.is_empty() {
        return std::collections::HashSet::new();
    }
    let bytes = masked.as_bytes();
    let is_ident = |c: u8| c == b'_' || c == b'$' || c.is_ascii_alphanumeric();
    let mut open: Vec<Option<usize>> = Vec::new();
    let mut functions: Vec<(usize, usize, bool)> = Vec::new();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'{' => {
                let mut p = i;
                while p > 0 && bytes[p - 1].is_ascii_whitespace() {
                    p -= 1;
                }
                let function = if p >= 2 && &bytes[p - 2..p] == b"=>" {
                    true
                } else if p > 0 && bytes[p - 1] == b')' {
                    !matches!(
                        matched_open_head(masked, bytes, p - 1, &is_ident).as_str(),
                        "if" | "for" | "while" | "switch" | "catch" | "with"
                    )
                } else {
                    false
                };
                open.push(function.then_some(i));
            }
            b'}' => {
                if let Some(Some(start)) = open.pop() {
                    let mut after = i + 1;
                    while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                        after += 1;
                    }
                    while after < bytes.len() && bytes[after] == b')' {
                        after += 1;
                        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                            after += 1;
                        }
                    }
                    let tail = &masked[after..];
                    let immediate = tail.starts_with('(')
                        || tail.starts_with(".call(")
                        || tail.starts_with(".apply(");
                    functions.push((start, i, immediate));
                }
            }
            _ => {}
        }
    }
    sites
        .iter()
        .copied()
        .filter(|site| {
            functions
                .iter()
                .any(|(start, end, immediate)| !immediate && start < site && site < end)
        })
        .collect()
}

/// Is the `require(` call whose match starts at masked-source offset
/// `call_start` reached only conditionally by a nearby operator or a
/// braceless control-flow header, even though it has no enclosing `{ }`
/// scope of its own? Brace-scope tracking (above) can't see these shapes:
/// `cond && require(...)` / `cond || require(...)` / `cond ?? require(...)`,
/// the ternary CONSEQUENT arm `cond ? require(...) : x`, a braceless arrow
/// `() => require(...)`, and a braceless control-flow body — `if (...)
/// require(...)`, `for (...) require(...)`, `while (...) require(...)`,
/// `else require(...)`, `do require(...)`.
fn site_is_conditionally_guarded(
    masked: &str,
    mbytes: &[u8],
    call_start: usize,
    is_ident: &impl Fn(u8) -> bool,
) -> bool {
    let mut p = call_start;
    while p > 0 && (mbytes[p - 1] as char).is_whitespace() {
        p -= 1;
    }
    if p == 0 {
        return false;
    }
    if p >= 2 {
        let two = &masked[p - 2..p];
        if two == "&&" || two == "||" || two == "??" || two == "=>" {
            return true;
        }
    }
    // Ternary consequent (`cond ? require(...) : x`) — a lone `?`, not the
    // second char of `??` (already handled above).
    if mbytes[p - 1] == b'?' && !(p >= 2 && mbytes[p - 2] == b'?') {
        return true;
    }
    // Braceless control-flow header: `if (...)`, `for (...)`, `while (...)`
    // immediately followed by the require call (no block).
    if mbytes[p - 1] == b')' {
        let head = matched_open_head(masked, mbytes, p - 1, is_ident);
        return matches!(head.as_str(), "if" | "for" | "while");
    }
    // Bare `else`/`do` immediately before, with no parens and no block.
    let mut w = p;
    while w > 0 && is_ident(mbytes[w - 1]) {
        w -= 1;
    }
    matches!(&masked[w..p], "else" | "do")
}

/// Every `require('<spec>')` specifier textually inside EITHER branch of a
/// `if (process.platform === /!== '<literal>') { … } else { … }` guard.
/// Mirrors the pattern `wrap.rs`'s `inactive_platform_guarded_requires`
/// matches to prune the DEAD branch's spec for a known build target — this
/// helper is target-independent and returns BOTH branches' specs, so the
/// LIVE branch's spec (which `inactive_platform_guarded_requires` keeps) can
/// be exempted from the general conditional-require classification above.
fn process_platform_guarded_specs(source: &str) -> std::collections::HashSet<String> {
    let re = perry_perex::tooling::Regex::new(
        r#"(?s)if\s*\(\s*process\.platform\s*(?:===|!==)\s*['"][^'"]+['"]\s*\)\s*\{(?P<then>.*?)\}\s*else\s*\{(?P<else>.*?)\}"#,
    )
    .unwrap();
    let mut specs = std::collections::HashSet::new();
    for cap in re.captures_iter(source) {
        if let Some(then) = cap.name("then") {
            specs.extend(extract_require_specifiers(then.as_str()));
        }
        if let Some(els) = cap.name("else") {
            specs.extend(extract_require_specifiers(els.as_str()));
        }
    }
    specs
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
