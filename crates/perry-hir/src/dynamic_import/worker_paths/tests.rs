use super::*;

fn resolve(source: &str) -> Resolution {
    let ast = perry_parser::parse_typescript(source, "worker-helpers.ts").unwrap();
    let module = crate::lower_module(&ast, "worker-helpers", "worker-helpers.ts").unwrap();
    let consts = collect_module_const_locals(&module);
    let params = collect_dynamic_import_param_literals(&module);
    let locals = collect_dynamic_import_local_candidate_literals(&module, &consts, &params);
    let mut results = Vec::new();
    for_each_worker_new(&module, &mut |expr| {
        if let Expr::WorkerNew { filename, .. } = expr {
            results.push(resolve_worker_path(
                filename, &module, &consts, &params, &locals,
            ));
        }
    });
    assert_eq!(results.len(), 1, "fixture must contain one Worker");
    results.remove(0)
}

fn paths(source: &str, expected: &[&str]) {
    match resolve(source) {
        Resolution::Set(paths) => assert_eq!(paths, expected),
        other => panic!("{other:?}\n{source}"),
    }
}

fn rejected(source: &str, diagnostic: &str) {
    match resolve(source) {
        Resolution::Unresolved(reason) => {
            assert!(reason.contains(diagnostic), "{reason}\n{source}")
        }
        other => panic!("expected {diagnostic}, got {other:?}\n{source}"),
    }
}

#[test]
fn bun_file_url_helper_chain() {
    paths(
        r#"
        const embeddedWorkerUrl = (path) => new URL(`file://${path}`);
        const hooksWorkerUrl = () => embeddedWorkerUrl("/$bunfs/root/worker.js");
        new Worker(hooksWorkerUrl());
    "#,
        &["file:///$bunfs/root/worker.js"],
    );
}

#[test]
fn declarations_aliases_and_nested_argument_calls() {
    paths(
        r#"
        import { Worker } from 'node:worker_threads';
        function identity(path: string) { return path; }
        const same = identity;
        const url = (path) => new URL(path, import.meta.url);
        new Worker(url(same(identity('./worker.js'))));
    "#,
        &["./worker.js"],
    );
    paths(
        r#"
        const identity = (path) => path;
        new Worker(identity(identity(new URL('./worker.js', import.meta.url))));
    "#,
        &["./worker.js"],
    );
}

#[test]
fn return_expressions_reuse_static_path_operations_and_registries() {
    paths(
        r#"
        import path from 'node:path';
        const registry = { worker: './worker.js' };
        const entry = (prefix) => path.join(prefix, registry.worker.replace('.js', '.ts'));
        new Worker(entry('./sub'));
    "#,
        &["sub/worker.ts"],
    );
    paths(
        r#"
        const choose = (key) => ({ a: './worker.js', b: './worker.js' })[key];
        const entry = () => true ? choose('a') : choose('b');
        new Worker(entry());
    "#,
        &["./worker.js"],
    );
}

#[test]
fn unsafe_helpers_stay_unresolved_with_reasons() {
    for body in [
        "console.log('effect'); return './worker.js';",
        "let x = './worker.js'; x = './other.js'; return x;",
        "if (true) return './worker.js'; return './other.js';",
    ] {
        rejected(
            &format!("function entry() {{ {body} }} new Worker(entry());"),
            "single return",
        );
    }
    rejected(
        "const entry = () => process.env.WORKER; new Worker(entry());",
        "unsupported expression",
    );
    rejected(
        "const entry = () => opaque(); new Worker(entry());",
        "opaque call",
    );
    rejected("const entry = () => console.log('x') ? './worker.js' : './worker.js'; new Worker(entry());", "selector");
    rejected(
        "const entry = () => './worker.js'; new Worker(entry(console.log('x')));",
        "exact list",
    );
    rejected(
        "const entry = async () => './worker.js'; new Worker(entry());",
        "async",
    );
    rejected(
        "const entry = (x = './worker.js') => x; new Worker(entry());",
        "simple static",
    );
    rejected(
        "const entry = () => new URL('https://example.com/worker.js'); new Worker(entry());",
        "static file URL",
    );
}

#[test]
fn mutation_and_recursion_are_rejected() {
    rejected(
        "let entry = () => './worker.js'; entry = () => './other.js'; new Worker(entry());",
        "mutable",
    );
    rejected("function entry() { return './worker.js'; } entry = () => './other.js'; new Worker(entry());", "mutable");
    rejected("let path = './worker.js'; path = './other.js'; const entry = () => path; new Worker(entry());", "mutable");
    rejected(
        "function entry() { return entry(); } new Worker(entry());",
        "recursive",
    );
    rejected(
        "const a = () => b(); const b = () => a(); new Worker(a());",
        "recursive",
    );
}

#[test]
fn candidate_depth_and_expansion_limits_are_enforced() {
    let candidates = (0..=DYNAMIC_IMPORT_PATH_CAP)
        .map(|n| format!("true ? './worker{n}.js' : "))
        .collect::<String>();
    // Balanced registry values reach the candidate cap without first hitting
    // the depth cap of a long ternary expression.
    let registry = (0..=DYNAMIC_IMPORT_PATH_CAP)
        .map(|n| format!("p{n}: './worker{n}.js'"))
        .collect::<Vec<_>>()
        .join(",");
    rejected(&format!("const registry = {{{registry}}}; const entry = () => registry.p0; new Worker(entry());"), "candidate count");
    rejected(
        &format!("const entry = () => {candidates}'./last.js'; new Worker(entry());"),
        "depth limit",
    );
    let helpers = (0..80)
        .map(|n| format!("const h{n} = () => h{}();", n + 1))
        .collect::<String>();
    rejected(
        &format!("{helpers} const h80 = () => './worker.js'; new Worker(h0());"),
        "depth limit",
    );
    let nested = (0..20).fold("'./worker.js'".to_string(), |arg, _| {
        format!("double({arg})")
    });
    rejected(
        &format!("const double = (path) => path + path; new Worker({nested});"),
        "string length limit",
    );
}

#[test]
fn dynamic_import_resolution_does_not_follow_helpers() {
    let ast = perry_parser::parse_typescript(
        "const entry = () => './worker.js'; import(entry());",
        "test.ts",
    )
    .unwrap();
    let module = crate::lower_module(&ast, "test", "test.ts").unwrap();
    let consts = collect_module_const_locals(&module);
    let mut count = 0;
    for_each_dynamic_import(&module, &mut |expr| {
        if let Expr::DynamicImport { arg, .. } = expr {
            count += 1;
            assert!(matches!(
                resolve_import_path_with_consts(arg, &consts, &mut HashSet::new()),
                Resolution::Unresolved(_)
            ));
        }
    });
    assert_eq!(count, 1);
}

#[test]
fn urls_remain_carriers_when_substituted_and_are_not_coerced_to_relative_strings() {
    rejected("const stringify = (url) => `${url}`; new Worker(stringify(new URL('./worker.js', import.meta.url)));", "URL string coercion");
    rejected("const prefix = (url) => './prefix' + url; new Worker(prefix(new URL('./worker.js', import.meta.url)));", "URL string coercion");
}

#[test]
fn branching_helper_expansion_has_a_shared_work_budget() {
    let mut source = "const h0 = () => 'x';".to_string();
    for n in 1..=12 {
        source.push_str(&format!("const h{n} = () => h{}() + h{}();", n - 1, n - 1));
    }
    source.push_str("new Worker(h12());");
    rejected(&source, "work limit");
}
