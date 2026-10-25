//! Local require must observe deletion and replacement of live cache records.
use super::wrap_commonjs;

fn wrapped(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dep.cjs"), "module.exports = { value: 1 };").unwrap();
    let result = wrap_commonjs(source, &dir.path().join("entry.cjs"));
    perry_parser::parse_typescript(&result, "wrapped.ts").expect("wrapper parses");
    result
}

#[test]
fn deferred_record_memo_is_guarded_by_the_live_cache() {
    let source = wrapped("module.exports = function load() { return require('./dep.cjs'); };");
    assert!(
        source.contains("__rec !== undefined && __perry_cjs_c["),
        "a memo must be invalidated by cache deletion or replacement: {source}"
    );
    assert!(
        source.contains(".loaded === true"),
        "partial modules must not be memoized"
    );
}

#[test]
fn eager_requires_read_the_live_record() {
    let source =
        wrapped("const first = require('./dep.cjs'); module.exports = require('./dep.cjs');");
    assert!(
        source.contains("const __perry_cached = __perry_cjs_c["),
        "eager imports cannot substitute for the live cache: {source}"
    );
    assert!(source.contains("return __perry_cached.exports;"));
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
