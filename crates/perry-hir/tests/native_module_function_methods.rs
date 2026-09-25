//! #11268: `Function.prototype.{bind,call,apply}` on a native-module export
//! (`zlib.inflate.bind(zlib)`) must not lower to a native class-static call
//! `inflate.bind`, which no table entry backs and which evaluated to
//! `undefined`. Genuine native statics of the same names
//! (`AsyncLocalStorage.bind`) must keep their native routing.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lowered_debug(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, "native_module_function_methods.ts", &mut cache)
                    .expect("parse should succeed");
            let module = lower_module(&parsed.module, "test", "native_module_function_methods.ts")
                .expect("lowering should succeed");
            format!("{module:?}")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn function_prototype_methods_on_module_exports_are_not_class_statics() {
    let debug = lowered_debug(
        r#"
        import * as zlib from "zlib";
        import * as path from "path";
        import * as util from "util";
        export const a = zlib.inflate.bind(zlib);
        export const b = path.join.call(path, "x", "y");
        export const c = util.format.apply(util, ["%s", 1]);
        "#,
    );
    for export in ["inflate", "join", "format"] {
        let class_static = format!("class_name: Some(\"{export}\")");
        assert!(
            !debug.contains(&class_static),
            "`<ns>.{export}.<bind|call|apply>` must not lower to a `{export}` class static: {debug}"
        );
    }
}

#[test]
fn registered_native_statics_named_bind_keep_native_routing() {
    let debug = lowered_debug(
        r#"
        import * as async_hooks from "async_hooks";
        export const f = async_hooks.AsyncLocalStorage.bind(() => 1);
        "#,
    );
    assert!(
        debug.contains("class_name: Some(\"AsyncLocalStorage\")") && debug.contains("method: \"bind\""),
        "`AsyncLocalStorage.bind` is a registered native static and must stay a NativeMethodCall: {debug}"
    );
}
