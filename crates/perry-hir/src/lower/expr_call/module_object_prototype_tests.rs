//! #13963: a builtin module's default object (and `process`) inherits the
//! `Object.prototype` methods, so `buffer.hasOwnProperty(k)` is a call on that
//! object, not on a missing export. These assert which lowering the call got:
//! the unimplemented-API gate compiled it to a deferred throw.
//! Byte-for-byte behaviour against node lives in
//! `tests/parity/node-suite/{buffer,util}/imports/default-object-prototype.ts`
//! and `tests/parity/node-suite/process/object-prototype-methods.ts`.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn hir(src: &str) -> String {
    let src = src.to_string();
    let module: Module = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "module_object_prototype.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "module_object_prototype.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked");
    format!("{module:?}")
}

fn assert_not_gated(src: &str, api: &str) {
    let h = hir(src);
    assert!(
        !h.contains("is not implemented in Perry"),
        "`{src}` lowered `{api}` to the unimplemented-API throw: {h}"
    );
}

#[test]
fn default_import_predicate_calls_are_not_gated() {
    for (src, api) in [
        (
            "import buffer from \"node:buffer\"; console.log(buffer.hasOwnProperty(\"Buffer\"));",
            "buffer.hasOwnProperty",
        ),
        (
            "import util from \"node:util\"; console.log(util.propertyIsEnumerable(\"inspect\"));",
            "util.propertyIsEnumerable",
        ),
        (
            "import fs from \"node:fs\"; console.log(fs.hasOwnProperty(\"readFileSync\"));",
            "fs.hasOwnProperty",
        ),
        (
            "console.log(process.hasOwnProperty(\"env\"));",
            "process.hasOwnProperty",
        ),
    ] {
        assert_not_gated(src, api);
    }
}

/// A value read is a real member read, not the `undefined` an absent export
/// of a core module folds to.
#[test]
fn default_import_inherited_reads_are_member_reads() {
    for (src, prop) in [
        (
            "import util from \"node:util\"; console.log(typeof util.hasOwnProperty);",
            "hasOwnProperty",
        ),
        (
            "import buffer from \"node:buffer\"; const f = buffer.isPrototypeOf; console.log(typeof f);",
            "isPrototypeOf",
        ),
    ] {
        let h = hir(src);
        assert!(
            h.contains(&format!("property: \"{prop}\"")),
            "`{src}` did not lower `{prop}` to a member read: {h}"
        );
    }
}

/// The gate itself stays live: a name the module neither exports nor inherits
/// is still the unimplemented-API throw.
#[test]
fn missing_export_call_is_still_gated() {
    let h = hir("import buffer from \"node:buffer\"; buffer.noSuchExport13963();");
    assert!(
        h.contains("is not implemented in Perry"),
        "a missing export must keep the deferred throw: {h}"
    );
}
