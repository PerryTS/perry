//! Namespace calls must retain the exported value as their receiver.
use crate::Module;
use perry_diagnostics::SourceCache;

fn hir(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = perry_parser::parse_typescript_with_cache(src, "namespace_static.ts", &mut cache)
        .expect("parse");
    let module: Module =
        crate::lower_module(&parsed.module, "test", "namespace_static.ts").expect("lower");
    format!("{module:?}")
}

#[test]
fn node_namespace_statics_call_the_exported_value() {
    for (module, class, method) in [
        ("url", "URL", "canParse"),
        ("buffer", "Buffer", "compare"),
        ("buffer", "Buffer", "isEncoding"),
        ("crypto", "KeyObject", "from"),
        ("worker_threads", "Worker", "getMaxListeners"),
        ("child_process", "ChildProcess", "getMaxListeners"),
        ("http", "Server", "getMaxListeners"),
        ("https", "Server", "getMaxListeners"),
    ] {
        for prefix in ["", "node:"] {
            let src = format!("import * as ns from '{prefix}{module}'; ns.{class}.{method}(1);");
            let h = hir(&src);
            assert!(
                !h.contains(&format!("class_name: Some(\"{class}\")")),
                "receiver-less native class call: {src}: {h}"
            );
            assert!(
                h.contains(&format!("property: \"{class}\""))
                    && h.contains(&format!("property: \"{method}\"")),
                "call lost the actual exported receiver/member: {src}: {h}"
            );
        }
    }
}

#[test]
fn module_wide_native_entries_keep_inherited_static_dispatch() {
    for (module, class, method) in [
        ("events", "EventEmitter", "getMaxListeners"),
        ("events", "EventEmitterAsyncResource", "getMaxListeners"),
        ("cluster", "Worker", "getMaxListeners"),
        ("stream", "Readable", "from"),
        ("stream", "Stream", "getDefaultHighWaterMark"),
        ("module", "Module", "isBuiltin"),
    ] {
        for prefix in ["", "node:"] {
            let src = format!("import * as ns from '{prefix}{module}'; ns.{class}.{method}(1);");
            let h = hir(&src);
            assert!(h.contains("NativeMethodCall"), "{src}: {h}");
            assert!(
                h.contains(&format!("class_name: Some(\"{class}\")")),
                "{src}: {h}"
            );
            assert!(h.contains(&format!("method: \"{method}\"")), "{src}: {h}");
        }
    }
}

#[test]
fn namespace_static_read_getter_spread_and_computed_forms_keep_the_receiver() {
    for src in [
        "import * as ns from 'node:url'; const f = ns.URL.canParse; f('http://x');",
        "import * as ns from 'node:perf_hooks'; ns.PerformanceObserver.supportedEntryTypes.includes('mark');",
        "import * as ns from 'node:url'; ns.URL.canParse(...['http://x']);",
        "import * as ns from 'node:async_hooks'; ns.AsyncLocalStorage.bind(...[() => 1]);",
        "import * as ns from 'node:url'; ns['URL']['canParse']('http://x');",
        "import * as ns from './classes.ts'; ns.Counter.add(2);",
        "import * as ns from './classes.ts'; const f = ns.Counter.add; f(2);",
        "import * as ns from './classes.ts'; ns.Counter.factory(2);",
    ] {
        let h = hir(src);
        assert!(!h.contains("NativeMethodCall"), "{src}: {h}");
        assert!(!h.contains("StaticMethodCall"), "{src}: {h}");
        assert!(h.contains("PropertyGet"), "{src}: {h}");
        assert!(h.contains("Call"), "{src}: {h}");
    }
}

#[test]
fn native_ffi_statics_and_server_call_keep_their_dispatch() {
    let h = hir("import * as eth from 'ethers'; eth.Wallet.createRandom();");
    assert!(h.contains("class_name: Some(\"Wallet\")"), "{h}");
    for module in ["http", "https"] {
        let h = hir(&format!(
            "import * as ns from 'node:{module}'; ns.Server.call({{}}, () => {{}});"
        ));
        assert!(
            h.contains(&format!("js_{module}_server_construct_with_this")),
            "{h}"
        );
    }
}

#[test]
fn unknown_namespace_export_keeps_the_unimplemented_api_gate() {
    let h = hir("import * as ns from 'node:async_hooks'; ns.MissingConstructor.method();");
    assert!(h.contains("not implemented in Perry"), "{h}");
}

#[test]
fn named_constructor_getter_result_is_a_value_receiver() {
    let h = hir("import { PerformanceObserver as P } from 'node:perf_hooks'; P.supportedEntryTypes.includes('mark');");
    assert!(!h.contains("not implemented in Perry"), "{h}");
    assert!(
        !h.contains("class_name: Some(\"supportedEntryTypes\")"),
        "{h}"
    );
    assert!(
        h.contains("supportedEntryTypes") && h.contains("includes"),
        "{h}"
    );
}

#[test]
fn named_native_function_members_are_value_receivers() {
    let h = hir("import { readFileSync as read } from 'node:fs'; read.extension.method();");
    assert!(!h.contains("not implemented in Perry"), "{h}");
    assert!(
        h.contains("PropertyGet") && h.contains("extension") && h.contains("method"),
        "{h}"
    );
}
