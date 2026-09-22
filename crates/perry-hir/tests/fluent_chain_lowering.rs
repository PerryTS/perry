use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<perry_hir::Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "fluent_chain_lowering.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "fluent_chain_lowering.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn chain_source(routes: &[(&str, &str)]) -> String {
    let chain = routes
        .iter()
        .map(|(method, name)| format!("  .{method}(\"{name}\", 0)"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"
        declare const handlers: any;

        export const out = handlers
        {chain}
        "#
    )
}

#[test]
fn opencode_session_route_chain_lowers_without_exponential_receiver_relowering() {
    let routes = [
        ("handle", "list"),
        ("handle", "status"),
        ("handle", "get"),
        ("handle", "children"),
        ("handle", "todo"),
        ("handle", "diff"),
        ("handle", "messages"),
        ("handle", "message"),
        ("handleRaw", "create"),
        ("handle", "remove"),
        ("handle", "update"),
        ("handleRaw", "fork"),
        ("handle", "abort"),
        ("handle", "init"),
        ("handle", "share"),
        ("handle", "unshare"),
        ("handle", "summarize"),
        ("handle", "prompt"),
        ("handle", "promptAsync"),
        ("handle", "command"),
        ("handle", "shell"),
        ("handle", "revert"),
        ("handle", "unrevert"),
        ("handle", "permissionRespond"),
        ("handle", "deleteMessage"),
        ("handle", "deletePart"),
        ("handle", "updatePart"),
    ];

    let module = lower_result(&chain_source(&routes)).expect("fluent route chain should lower");
    let debug = format!("{module:#?}");
    assert!(
        debug.contains("property: \"handle\"") && debug.contains("property: \"handleRaw\""),
        "route builder calls should remain generic property calls: {debug}"
    );
    assert!(
        !debug.contains("NativeMethodCall"),
        "generic route builder calls must not be classified as native methods: {debug}"
    );
}

#[test]
fn uppercase_imported_builder_chain_stays_generic() {
    let chain = (0..80)
        .map(|idx| format!("  .add(\"route-{idx}\", 0)"))
        .collect::<Vec<_>>()
        .join("\n");

    let src = format!(
        r#"
        import {{ HttpApiGroup }} from "effect/unstable/httpapi";

        export const out = HttpApiGroup.make("session")
        {chain}
        "#
    );

    let module = lower_result(&src).expect("uppercase imported builder chain should lower");
    let debug = format!("{module:#?}");
    assert!(
        debug.contains("property: \"add\""),
        "builder calls should remain generic property calls: {debug}"
    );
    assert!(
        !debug.contains("NativeMethodCall"),
        "uppercase imported builders must not be misclassified as native instance chains: {debug}"
    );
}

// `native_fluent_chain_still_dispatches_through_native_methods` removed here
// (was `new Decimal(1).plus(2).times(3).toString()`, no import).
//
// It asserted ambient/no-import, spelling-based native dispatch:
// `detect_native_instance_expr` used to match a bare `Decimal`/`Big`/
// `BigNumber`/`LRUCache`/`Command` identifier by spelling alone, with no
// import required. This commit tightens that (the #10439 fix this PR makes)
// to require `ctx.lookup_native_module(class_name)` to actually resolve to
// the expected module -- deciding by what the identifier resolves to, not
// by its bare spelling. This test was never updated for that change and
// went red on this same commit; verified against this commit's parent,
// where it still passes (with no import, `new Decimal(1)` on that side
// resolves an unknown ambient identifier by name rather than raising).
//
// With no import, `new Decimal(1)` (or `Command`/`LRUCache`/...) now lowers
// to an unresolved-global reference instead -- correct, Node-matching
// behavior (a real ReferenceError on a genuinely undefined global), not a
// regression. Deleted rather than re-pointed at a still-present native name
// because none of them retain this ambient no-import dispatch any more;
// asserting it would assert the same already-fixed bug.
