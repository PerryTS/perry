//! Local require must observe deletion and replacement of live cache records.
use super::wrap_commonjs;

fn wrapped(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dep.cjs"), "module.exports = { value: 1 };").unwrap();
    let result = wrap_commonjs(source, &dir.path().join("entry.cjs"));
    perry_parser::parse_typescript(&result, "wrapped.ts").expect("wrapper parses");
    result
}

/// The edge function of `require('./dep.cjs')`, split into its arm for a
/// program that is not closed and its arm for a closed one.
fn edge_arms(source: &str) -> (&str, &str) {
    let start = source
        .find("function __perry_require_static_0() {")
        .expect("the edge");
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").expect("its end")];
    let split = body.find("} else {").expect("two arms");
    (&body[..split], &body[split..])
}

#[test]
fn a_lazy_require_is_the_static_edge() {
    let source = wrapped("module.exports = function load() { return require('./dep.cjs'); };");
    assert!(
        source.contains("return __perry_require_static_0();"),
        "{source}"
    );
    assert!(!source.contains("__perry_cjs_require_deferred"), "{source}");
}

#[test]
fn only_a_program_that_is_not_closed_reads_the_live_cache() {
    let source =
        wrapped("const first = require('./dep.cjs'); module.exports = require('./dep.cjs');");
    let (open, closed) = edge_arms(&source);
    // #11249: an edge outlives `require.cache` deletion and replacement only
    // where a module can write the cache: there it reads the live record.
    assert!(
        open.starts_with("function __perry_require_static_0() {\n    if (__perry_cjs_open()) {"),
        "{open}"
    );
    assert!(open.contains("const cached = __perry_cjs_cache["), "{open}");
    assert!(open.contains("return cached.exports;"), "{open}");
    // A fresh record is linked once, by the pair flag
    // (Node pushes a new Module without a scan).
    assert!(open.contains("if (__perry_cjs_lk_0 === false) {"), "{open}");
    assert!(
        open.contains("__perry_cjs_record.children.push(record);"),
        "{open}"
    );
    // Closed: the record binding is the link, one flag per pair, no scan.
    assert!(!closed.contains("__perry_cjs_cache"), "{closed}");
    assert!(
        !closed.contains("indexOf") && !closed.contains("__perry_cjs_link"),
        "{closed}"
    );
    assert!(
        closed.contains("const record = __perry_cjs_init(__perry_cjs_rec_0);"),
        "{closed}"
    );
    assert!(
        closed.contains("if (__perry_cjs_lk_0 === false) {"),
        "{closed}"
    );
}

#[test]
fn computed_requires_refresh_evicted_path_modules() {
    let source = wrapped("module.exports = name => require('./' + name + '.cjs');");
    assert!(
        source.contains("return __perry_cjs_require_computed(specifier, "),
        "computed requests go through the support module: {source}"
    );
    // ... whose path-module arm re-evaluates an evicted module's factory.
    let support = super::super::support::source(&["/x/entry.cjs".to_string()], false);
    assert!(
        support.contains("return refresh(path, pathModule, reloader);"),
        "computed requests need factory re-evaluation too: {support}"
    );
}
