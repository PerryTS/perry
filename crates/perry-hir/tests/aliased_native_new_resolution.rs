//! Regression test for #8730: an ALIASED ESM named import of a Node built-in
//! class (`import { BlockList as Wj4 } from "net"; new Wj4()`) must not lower
//! `new <alias>()` to the nameless `js_throw_reference_error_unresolved_get`
//! throw.
//!
//! Root cause: the alias-rewrite block in `lower_new` replaces the callee's
//! `class_name` with the native class's EXPORT name (`Wj4` -> `BlockList`) so
//! the construction path matches the un-aliased form, but the freshly-added
//! (#8688) unresolved-`new` guard then consulted `lookup_native_module` under
//! that rewritten export name — which is not in the registry (it is keyed on
//! the LOCAL import name). None of these classes are reified global builtins,
//! so the guard fired and every command threw `ReferenceError: identifier is
//! not defined` at module init.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

const THROW_HELPER: &str = "js_throw_reference_error_unresolved_get";

fn lower_debug(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "aliased_native_new_resolution.ts", &mut cache)
                    .expect("parse should succeed");
            let module = lower_module(&parsed.module, "test", "aliased_native_new_resolution.ts")
                .expect("lowering should succeed");
            format!("{module:#?}")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn aliased_native_class_import_does_not_lower_to_nameless_throw() {
    // Each mirrors a real cli.js 2.1.112 shape from #8730 (BlockList/Wj4 built
    // and `.addSubnet`-ed at module init; AsyncLocalStorage/J_z; PassThrough).
    let cases = [
        (
            "BlockList",
            r#"import { BlockList as Wj4 } from "net";
               const b = new Wj4();
               b.addSubnet("10.0.0.0", 8);
               console.log(b.check("10.1.2.3"));"#,
        ),
        (
            "AsyncLocalStorage",
            r#"import { AsyncLocalStorage as J_z } from "async_hooks";
               const s = new J_z();
               console.log(typeof s.run);"#,
        ),
        (
            "PassThrough",
            r#"import { PassThrough as Lrz } from "stream";
               const p = new Lrz();
               console.log(typeof p.pipe);"#,
        ),
    ];

    for (label, src) in cases {
        let debug = lower_debug(src);
        assert!(
            !debug.contains(THROW_HELPER),
            "aliased native import `{label}` must construct, not throw the nameless \
             ReferenceError at module init:\n{debug}"
        );
    }
}

#[test]
fn unaliased_native_class_import_still_constructs() {
    // Control: the un-aliased form was never broken; keep it green so the fix
    // is symmetric across aliased/un-aliased native imports.
    let debug = lower_debug(
        r#"import { BlockList } from "net";
           const b = new BlockList();
           b.addSubnet("10.0.0.0", 8);
           console.log(b.check("10.1.2.3"));"#,
    );
    assert!(
        !debug.contains(THROW_HELPER),
        "un-aliased native import must construct, not throw:\n{debug}"
    );
}

#[test]
fn genuinely_unresolved_new_still_throws() {
    // Positive control: the guard must still fire for a `new` on an identifier
    // that resolves to no binding at all — the fix must not blanket-suppress it.
    let debug = lower_debug(r#"const x = new Totally_Undefined_Constructor_Xyz();"#);
    assert!(
        debug.contains(THROW_HELPER),
        "a genuinely unresolved `new` must still lower to the nameless \
         ReferenceError throw:\n{debug}"
    );
}
