//! The identity audit (DESIGN-p4l4 §2.2, VR-0): a realm-declared body whose
//! source compares values by identity must say so (`identity(slot)`).
//!
//! A **precision test**, not the soundness argument. It reads the runtime's
//! Rust source by name: a declared body and the functions it calls, up to
//! [`DEPTH`] hops, matched by function name (no type resolution, no trait or
//! pointer calls). A comparison that is further away, or behind an indirect
//! call, is not seen. Soundness rests on default-deny: a slot no declaration
//! names is never a value-record boundary, and a native with no declaration
//! is unknown code.
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The runtime's identity comparisons: SameValueZero, IsStrictlyEqual,
/// IsLooselyEqual (`value/equality.rs`) and the Map/Set key lookup.
pub(crate) const IDENTITY_FNS: &[&str] = &[
    "js_jsvalue_same_value_zero",
    "js_jsvalue_equals",
    "js_jsvalue_loose_equals",
    "find_key_index",
    "js_map_find_key_index",
];

/// Hops from a declared body through the functions it names.
pub(crate) const DEPTH: usize = 3;

/// Every `fn name` of the sources: name -> bodies (one per definition).
pub(crate) fn fn_bodies(files: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for text in files {
        let b = text.as_bytes();
        let mut at = 0;
        while let Some(i) = text[at..].find("fn ") {
            let start = at + i;
            at = start + 3;
            if start > 0 && (b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_') {
                continue;
            }
            let name: String = text[at..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            // The body: the first `{` after the signature (before any `;`).
            let Some(open) = text[at..].find(['{', ';']).map(|k| at + k) else {
                break;
            };
            if b[open] == b';' {
                continue;
            }
            let mut depth = 0usize;
            let mut end = open;
            for (k, &c) in b[open..].iter().enumerate() {
                match c {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end > open {
                out.entry(name)
                    .or_default()
                    .push(text[open..=end].to_string());
            }
        }
    }
    // A thunk a macro defines (`thunk_macro!(name, crate::array::engine)`):
    // its body calls what the invocation names after the thunk's name.
    for text in files {
        let mut at = 0;
        while let Some(i) = text[at..].find("!(") {
            let open = at + i + 1;
            at = open + 1;
            let Some(close) = text[open..].find(')').map(|k| open + k) else {
                break;
            };
            let inner = &text[open + 1..close];
            let mut words = inner
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty());
            let Some(name) = words.next() else { continue };
            if !name.contains('_') || inner.contains('"') {
                continue;
            }
            let body: String = words.map(|w| format!("{w}() ")).collect();
            out.entry(name.to_string()).or_default().push(body);
        }
    }
    out
}

/// The identifiers a body calls (`name(` or `name::<..>(`).
fn callees(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if b.get(i) == Some(&b'(') {
                out.insert(body[s..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The identity comparison `body` reaches within `DEPTH` hops, by name.
pub(crate) fn reaches_identity(
    fns: &BTreeMap<String, Vec<String>>,
    body: &str,
) -> Option<Vec<String>> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut frontier: Vec<(String, Vec<String>)> = vec![(body.to_string(), Vec::new())];
    if !seen.insert(body.to_string()) {
        return None;
    }
    for _ in 0..=DEPTH {
        let mut next = Vec::new();
        for (name, path) in frontier {
            let Some(defs) = fns.get(&name) else { continue };
            for def in defs {
                for c in callees(def) {
                    let mut p = path.clone();
                    p.push(c.clone());
                    if IDENTITY_FNS.contains(&c.as_str()) {
                        return Some(p);
                    }
                    if seen.insert(c.clone()) {
                        next.push((c, p));
                    }
                }
            }
        }
        frontier = next;
    }
    None
}

/// Rust source files under `root`.
pub(crate) fn read_sources(root: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            read_sources(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            if let Ok(t) = std::fs::read_to_string(&p) {
                out.push(t);
            }
        }
    }
}

/// The last path segment of a declaration's body (`body` or, for an
/// effects-only block, `effects_body`).
pub(crate) fn body_name(d: &Value) -> Option<String> {
    let body = d
        .get("effects_body")
        .or_else(|| d.get("body"))
        .and_then(Value::string)?;
    let body = body.strip_prefix("source:").unwrap_or(body);
    Some(body.rsplit("::").next().unwrap_or(body).to_string())
}

pub(crate) fn declares_identity(d: &Value) -> bool {
    d.get("effects")
        .and_then(Value::array)
        .is_some_and(|es| es.iter().any(|e| e.get("identity").is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> (Vec<Value>, BTreeMap<String, Vec<String>>) {
        let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../perry-runtime/src"));
        let manifest = crate::realm_source::read(src).unwrap();
        let ds = manifest
            .get("declarations")
            .and_then(Value::array)
            .unwrap()
            .to_vec();
        let mut files = Vec::new();
        read_sources(src, &mut files);
        read_sources(&src.join("../../perry-stdlib/src"), &mut files);
        (ds, fn_bodies(&files))
    }

    /// Every declared body that compares by identity within `DEPTH` hops
    /// declares `identity(slot)`; every declared body is found (the audit
    /// is not vacuous).
    #[test]
    fn declared_bodies_that_compare_by_identity_declare_it() {
        let (ds, fns) = runtime();
        let mut missing = Vec::new();
        let mut found = Vec::new();
        let mut unresolved = Vec::new();
        let declared: Vec<&Value> = ds.iter().filter(|d| d.get("effects").is_some()).collect();
        assert!(declared.len() > 100, "the audit sees the declarations");
        for d in declared {
            let name = d.get("name").and_then(Value::string).unwrap_or("?");
            let body = body_name(d).unwrap_or_default();
            if !fns.contains_key(&body) {
                unresolved.push(format!("{name} ({body})"));
                continue;
            }
            if let Some(path) = reaches_identity(&fns, &body) {
                found.push(name.to_string());
                if !declares_identity(d) {
                    missing.push(format!("{name}: {body} -> {}", path.join(" -> ")));
                }
            }
        }
        assert!(unresolved.is_empty(), "bodies not found: {unresolved:?}");
        assert!(
            missing.is_empty(),
            "declare identity(slot) on the compared slots: {missing:?}"
        );
        // Positive control: the two declared searches are seen comparing.
        for n in ["Array.prototype.includes", "Array.prototype.indexOf"] {
            assert!(
                found.iter().any(|f| f == n),
                "{n} reaches an identity comparison"
            );
        }
    }

    /// The scan follows calls by name and macro-made thunks, and finds no
    /// comparison where none is (negative control).
    #[test]
    fn the_scan_follows_named_calls_and_macro_thunks() {
        let files = vec!["fn engine(a: f64, b: f64) -> bool { helper(a, b) }\n\
             fn helper(a: f64, b: f64) -> bool { js_jsvalue_equals(a, b) != 0 }\n\
             fn reader(a: f64) -> f64 { other(a) }\n\
             fn other(a: f64) -> f64 { a }\n\
             search_thunk!(the_search_thunk, crate::array::engine);\n\
             search_thunk!(the_read_thunk, crate::array::reader);\n"
            .to_string()];
        let fns = fn_bodies(&files);
        assert_eq!(
            reaches_identity(&fns, "the_search_thunk"),
            Some(vec![
                "engine".to_string(),
                "helper".to_string(),
                "js_jsvalue_equals".to_string()
            ])
        );
        assert_eq!(reaches_identity(&fns, "the_read_thunk"), None);
        assert_eq!(reaches_identity(&fns, "reader"), None);
    }
}
