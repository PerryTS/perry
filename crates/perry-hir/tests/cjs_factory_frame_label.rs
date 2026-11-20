// A CommonJS module's top level runs inside the wrapper's
// `__perry_cjs_factory`. Node renders that frame as
// `Object.<anonymous> (<absolute file path>)`, and stack-parsing libraries
// read the file name out of it (fastify-plugin names an anonymous plugin from
// the path in the caller's frame and throws when there is none). The display
// name registered for the factory is what `Error.stack` prints, so it must
// carry the module's path.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str, path: &str) -> Module {
    let src = src.to_string();
    let path = path.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                parse_typescript_with_cache(&src, &path, &mut cache).expect("parse should succeed");
            lower_module(&parsed.module, "test", &path).expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn the_module_body_frame_is_labelled_with_the_module_path() {
    let module = lower_result(
        r#"
        const _cjs = (function () {
            function __perry_cjs_factory() { return 1; }
            return __perry_cjs_factory();
        })();
        function helper() { return 2; }
        const _h = (function () { function inner() { return 3; } return inner(); })();
        "#,
        "/app/node_modules/plugin/index.js",
    );
    let names: Vec<&String> = module.closure_display_names.values().collect();
    assert!(
        names
            .iter()
            .any(|n| n.as_str() == "Object.<anonymous> (/app/node_modules/plugin/index.js)"),
        "factory frame must carry the module path: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.as_str() == "__perry_cjs_factory"),
        "the synthetic factory name must not reach the frame label: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.as_str() == "inner"),
        "other nested functions keep their own names: {names:?}"
    );
}
