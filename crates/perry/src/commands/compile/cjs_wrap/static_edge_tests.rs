//! A literal `require` of a compiled CommonJS module is a static edge
//! (design CJS_LINK 2026-10-26 §2.2, §7).
use super::wrap::wrap_commonjs;

fn wrapped(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.cjs"), "module.exports = { value: 1 };").unwrap();
    std::fs::write(dir.path().join("esm.mjs"), "export const value = 1;").unwrap();
    let result = wrap_commonjs(source, &dir.path().join("entry.cjs"));
    perry_parser::parse_typescript(&result, "wrapped.ts").expect("wrapper parses");
    result
}

#[test]
fn a_literal_require_in_the_body_is_the_edge_not_a_call_of_require() {
    let source = wrapped("const a = 1;\nconsole.log(require('./x.cjs').value);\n");
    assert!(
        source.contains("console.log(__perry_require_static_0().value);"),
        "{source}"
    );
    assert!(
        source.contains("import { __perry_cjs_record as __perry_cjs_rec_0 } from"),
        "{source}"
    );
    assert!(
        source.contains("export var __perry_cjs_record;"),
        "{source}"
    );
    // The `require` function's arm is the same edge, for `require` as a value.
    assert!(
        source.contains("if (specifier === \"./x.cjs\") return __perry_require_static_0();"),
        "{source}"
    );
}

#[test]
fn a_shadowed_require_is_left_alone() {
    let source =
        wrapped("function f(require) { return require('./x.cjs'); }\nmodule.exports = f;\n");
    assert!(
        source.contains("function f(require) { return require('./x.cjs'); }"),
        "{source}"
    );
    let source = wrapped(
        "function g() { const require = h; return require('./x.cjs'); }\nrequire('./x.cjs');\n",
    );
    assert!(source.contains("return require('./x.cjs'); }"), "{source}");
    assert!(source.contains("\n__perry_require_static_0();"), "{source}");
    // Bound at module scope: no call is the wrapper's.
    let source = wrapped("var require = h;\nrequire('./x.cjs');\n");
    assert!(source.contains("\nrequire('./x.cjs');"), "{source}");
}

#[test]
fn a_computed_specifier_keeps_the_require_function() {
    let source = wrapped("const name = './x.cjs';\nmodule.exports = require(name);\n");
    assert!(
        source.contains("module.exports = require(name);"),
        "{source}"
    );
    assert!(
        source.contains("return __perry_cjs_require_computed(specifier, "),
        "{source}"
    );
}

#[test]
fn an_es_module_target_is_not_an_edge() {
    let source = wrapped("module.exports = require('./esm.mjs');\n");
    assert!(!source.contains("__perry_require_static_"), "{source}");
    assert!(
        source.contains("module.exports = require('./esm.mjs');"),
        "{source}"
    );
}

#[test]
fn an_adopted_alias_declaration_becomes_its_edge_statement() {
    // The alias binding is the hoisted import; the target still evaluates
    // where the declaration ran, after the statement before it.
    let source =
        wrapped("console.log('before');\nconst x = require('./x.cjs');\nconsole.log(x.value);\n");
    let before = source.find("console.log('before');").unwrap();
    let edge = source
        .find("\n__perry_require_static_0();")
        .expect("edge statement");
    assert!(before < edge, "{source}");
    assert!(
        !source.contains("const x = require('./x.cjs');"),
        "{source}"
    );
}

#[test]
fn registry_publication_is_only_for_a_program_that_is_not_closed() {
    let source = wrapped("module.exports = 1;\n");
    for call in [
        "__perry_register_path_module_partial(",
        "__perry_link_path_module_parent(",
        "__perry_register_path_module(",
    ] {
        let at = source
            .find(call)
            .unwrap_or_else(|| panic!("{call}: {source}"));
        let open = source[..at]
            .rfind("if (__perry_cjs_open()) {")
            .expect("open arm");
        assert!(
            !source[open..at].contains('}'),
            "{call} outside the open arm:\n{source}"
        );
    }
}

#[test]
fn an_open_edge_links_each_record_once() {
    // CJL2: in a program that is not closed the link flag says the edge
    // linked the record it found, one byte test per call after the first.
    // A module that cannot require by a computed path pushes without a scan;
    // its reload is its own loader (`createRequire(filename)`) with the
    // literal path (the new record's parent, the facts' path node), and it
    // links the new record through the scan.
    let source = wrapped("const x = require('./x.cjs');\nmodule.exports = x.value;\n");
    assert!(
        source.contains("if (__perry_cjs_lk_0 === false) { __perry_cjs_lk_0 = true; if (cached.parent === undefined) cached.parent = __perry_cjs_record; __perry_cjs_record.children.push(cached); }"),
        "{source}"
    );
    assert!(
        source.contains("if (fresh !== undefined) { __perry_cjs_lk_0 = true; __perry_cjs_link(__perry_cjs_record, fresh); }"),
        "{source}"
    );
    assert!(
        !source.contains("__perry_cjs_link(__perry_cjs_record, cached)"),
        "{source}"
    );
    assert!(
        source.contains("const required = __perry_cjs_reload("),
        "{source}"
    );
    // A module that can require by a computed path scans on its first link
    // of a cached record: the runtime loader may have linked it already.
    let source = wrapped(
        "const x = require('./x.cjs');\nconst r = require;\nmodule.exports = [x.value, r];\n",
    );
    assert!(
        source.contains("if (__perry_cjs_lk_0 === false) { __perry_cjs_lk_0 = true; __perry_cjs_link(__perry_cjs_record, cached); }"),
        "{source}"
    );
}

#[test]
fn two_specifiers_of_one_target_are_one_edge() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("y.js"), "module.exports = { value: 1 };").unwrap();
    let source = wrap_commonjs(
        "const a = require('./y');\nconst b = require('./y.js');\nmodule.exports = a === b;\n",
        &dir.path().join("entry.cjs"),
    );
    assert!(
        source.contains("function __perry_require_static_0()"),
        "{source}"
    );
    assert!(
        !source.contains("function __perry_require_static_1()"),
        "{source}"
    );
    assert!(source.contains("let __perry_cjs_lk_0 = false;"), "{source}");
    assert!(!source.contains("__perry_cjs_lk_1"), "{source}");
    assert!(
        source.contains("if (specifier === \"./y.js\") return __perry_require_static_0();"),
        "{source}"
    );
}

#[test]
fn a_literal_builtin_require_in_the_body_reads_its_binding() {
    // CJL2: the `require` function's builtin arm hands the namespace out as
    // a value, which links the builtin's whole surface (1.06 MB for `fs`); a
    // closed program never calls that function, so the body reads the
    // binding and the arm exists only where the program is not closed.
    let source = wrapped(
        "const fs = require('fs');\nfunction read(p) { return require('fs').readFileSync(p); }\nmodule.exports = read;\n",
    );
    assert!(source.contains("return fs.readFileSync(p); }"), "{source}");
    assert!(
        source.contains("if (__perry_cjs_open()) { if (specifier === 'fs') { return fs; } }"),
        "{source}"
    );
    // A call inside a scope that binds the local again keeps the function
    // and its unconditional arm.
    let source = wrapped(
        "const fs = require('fs');\nfunction g() { const fs = require('fs'); return fs; }\nmodule.exports = [fs, g];\n",
    );
    assert!(
        source.contains("{ const fs = require('fs'); return fs; }"),
        "{source}"
    );
    assert!(
        source.contains("        if (specifier === 'fs') { return fs; }\n"),
        "{source}"
    );
    // A destructuring pattern keeps the call and the arm: the native value
    // path it would lower through is not relinkable as a class parent.
    let source = wrapped(
        "const { EventEmitter } = require('node:events');\nclass Bus extends EventEmitter {}\nmodule.exports = Bus;\n",
    );
    assert!(
        source.contains("const { EventEmitter } = require('node:events');"),
        "{source}"
    );
    assert!(
        source.contains("        if (specifier === 'node:events') { return _req_0; }\n"),
        "{source}"
    );
}
