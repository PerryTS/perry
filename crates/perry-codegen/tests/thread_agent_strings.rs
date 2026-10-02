use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

const LITERAL: &str =
    "direct-module-thread-literal-longer-than-sixty-four-bytes-must-belong-to-this-agent";

fn literal_ir(name: &str, method: Option<&str>, opts: CompileOptions, deferred: bool) -> String {
    let mut module = Module::new(name);
    if deferred {
        module.init_kind = ModuleInitKind::Deferred;
    }
    module.init.push(Stmt::Expr(Expr::String(LITERAL.into())));
    if let Some(method) = method {
        let closure = Expr::Closure {
            func_id: 17,
            params: Vec::new(),
            return_type: Type::String,
            body: vec![Stmt::Return(Some(Expr::String(LITERAL.into())))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        };
        let args = if method == "spawn" {
            vec![closure]
        } else {
            vec![
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
                closure,
            ]
        };
        module.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/thread".into(),
            class_name: None,
            object: None,
            method: method.into(),
            args,
        }));
    }
    String::from_utf8(compile_module(&module, opts).expect("thread fixture compiles")).unwrap()
}

fn options(graph: &[&str], is_entry: bool) -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        is_entry_module: is_entry,
        thread_literal_module_prefixes: graph.iter().map(|prefix| (*prefix).into()).collect(),
        ..Default::default()
    }
}

fn callback<'a>(ir: &'a str, prefix: &str) -> &'a str {
    let anchor = format!("define void @__perry_prepare_thread_strings_{prefix}(");
    ir.split(anchor.as_str())
        .nth(1)
        .expect("owner callback must be defined")
        .split("\n}")
        .next()
        .unwrap()
}

fn assert_tls_pool(ir: &str, prefix: &str) {
    assert!(ir
        .lines()
        .any(|line| line.contains(".handle = internal thread_local global double")));
    assert!(ir.contains(&format!(
        "@__perry_agent_strings_ready_{prefix} = internal thread_local global i8 0"
    )));
}

fn assert_string_only_callback(ir: &str, prefix: &str, graph: &[&str]) {
    let cb = callback(ir, prefix);
    let calls: Vec<_> = cb
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("call void @"))
        .collect();
    let expected: Vec<_> = graph
        .iter()
        .map(|prefix| format!("call void @__perry_prepare_agent_strings_{prefix}()"))
        .collect();
    assert_eq!(calls, expected, "one pure-string call per graph module");
    for forbidden in [
        "__init",
        "__perry_prepare_literals_",
        "js_class",
        "js_shape",
        "js_gc",
    ] {
        assert!(
            !cb.contains(forbidden),
            "graph callback contains {forbidden}"
        );
    }
}

#[test]
fn standalone_launches_prepare_local_tls_literals_without_cli_flags() {
    for is_entry in [false, true] {
        for (method, runtime) in [
            ("spawn", "js_thread_spawn_with_literals"),
            ("parallelMap", "js_thread_parallel_map_with_literals"),
            ("parallelFilter", "js_thread_parallel_filter_with_literals"),
        ] {
            let ir = literal_ir(
                "thread_direct.ts",
                Some(method),
                options(&[], is_entry),
                false,
            );
            assert!(ir.contains(&format!("call double @{runtime}(")));
            assert!(ir.contains(
                "ptrtoint (ptr @__perry_prepare_thread_strings_thread_direct_ts to i64)"
            ));
            assert_tls_pool(&ir, "thread_direct_ts");
            assert_eq!(
                ir.matches("define void @__perry_prepare_thread_strings_")
                    .count(),
                1
            );
            assert_string_only_callback(&ir, "thread_direct_ts", &["thread_direct_ts"]);
        }
    }
}

#[test]
fn graph_emits_one_callback_in_actual_entry_and_all_launches_reference_it() {
    // The actual entry sorts last and contains no launch. Deferred/eager helper
    // pools must still be TLS and prepared without executing their bodies.
    let graph = [
        "z_entry_ts",
        "a_launcher_ts",
        "b_mapper_ts",
        "c_filter_ts",
        "deferred_ts",
    ];
    let modules = [
        ("z_entry.ts", None, true, false),
        ("a_launcher.ts", Some("spawn"), false, false),
        ("b_mapper.ts", Some("parallelMap"), false, false),
        ("c_filter.ts", Some("parallelFilter"), false, false),
        ("deferred.ts", None, false, true),
    ];
    let mut definitions = 0;
    for ((name, method, entry, deferred), prefix) in modules.into_iter().zip(graph) {
        let ir = literal_ir(name, method, options(&graph, entry), deferred);
        assert_tls_pool(&ir, prefix);
        definitions += ir
            .matches("define void @__perry_prepare_thread_strings_")
            .count();
        if entry {
            assert_string_only_callback(&ir, "z_entry_ts", &graph);
        } else {
            assert!(!ir.contains("define void @__perry_prepare_thread_strings_"));
            for foreign in graph.iter().filter(|other| **other != prefix) {
                assert!(!ir.contains(&format!(
                    "declare void @__perry_prepare_agent_strings_{foreign}("
                )));
            }
        }
        if method.is_some() {
            assert!(ir.contains("ptrtoint (ptr @__perry_prepare_thread_strings_z_entry_ts to i64)"));
            assert!(!ir.contains(&format!("@__perry_prepare_thread_strings_{prefix}(")));
        }
    }
    assert_eq!(
        definitions, 1,
        "one callback definition across the compiled graph"
    );
}

#[test]
fn explicit_first_prefix_owns_callback_and_owner_normalizes_duplicates() {
    let graph = ["z_owner_ts", "helper_ts", "z_owner_ts", "helper_ts"];
    // Direct embedders may choose any linked module; the first graph prefix,
    // rather than the entry boolean or lexicographic minimum, selects it.
    let owner = literal_ir("z_owner.ts", None, options(&graph, false), false);
    assert_string_only_callback(&owner, "z_owner_ts", &["z_owner_ts", "helper_ts"]);
    let helper = literal_ir("helper.ts", Some("spawn"), options(&graph, true), false);
    assert_tls_pool(&helper, "helper_ts");
    assert!(!helper.contains("define void @__perry_prepare_thread_strings_"));
    assert!(helper.contains("ptrtoint (ptr @__perry_prepare_thread_strings_z_owner_ts to i64)"));
}

#[test]
fn thread_free_direct_module_keeps_process_wide_string_handles() {
    let ir = literal_ir("thread_direct.ts", None, options(&[], false), false);
    assert!(ir
        .lines()
        .any(|line| line.contains(".handle = internal global double")));
    assert!(!ir.contains("define void @__perry_prepare_thread_strings_"));
}
