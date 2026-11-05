use crate::ir::{Expr, Module, Stmt};

fn lower(source: &str) -> Module {
    let parsed = perry_parser::parse_typescript(source, "worker-import.ts").unwrap();
    crate::lower::lower_module(&parsed, "worker-import", "worker-import.ts").unwrap()
}

fn result(module: &Module) -> &Expr {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { name, init, .. } if name == "result" => init.as_ref(),
            _ => None,
        })
        .expect("result is initialized")
}

fn is_messaging_constructor(expr: &Expr, export: &str) -> bool {
    match expr {
        Expr::NewDynamic { callee, .. } if export == "BroadcastChannel" => {
            matches!(callee.as_ref(), Expr::PropertyGet { object, property, .. }
                if matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "worker_threads")
                    && property == export)
        }
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method,
            ..
        } => module == "worker_threads" && method == export && export == "MessageChannel",
        _ => false,
    }
}

#[test]
fn messaging_imports_select_the_export_not_the_local_constructor_name() {
    for (export, local) in [
        ("MessageChannel", "Channel"),
        ("MessageChannel", "channel"),
        ("MessageChannel", "BroadcastChannel"),
        ("MessageChannel", "Worker"),
        ("BroadcastChannel", "Broadcast"),
        ("BroadcastChannel", "broadcast"),
        ("BroadcastChannel", "MessageChannel"),
    ] {
        let hir = lower(&format!("import {{ {export} as {local} }} from 'node:worker_threads'; const result = new {local}('name');"));
        assert!(
            is_messaging_constructor(result(&hir), export),
            "{export} as {local}: {:?}",
            result(&hir)
        );
    }
}

#[test]
fn renamed_workers_keep_entry_discovery_and_eval_mode() {
    for local in ["Thread", "thread", "MessageChannel"] {
        for is_eval in [false, true] {
            let hir = lower(&format!("import {{ Worker as {local} }} from 'node:worker_threads'; const result = new {local}('entry.cjs', {{ eval: {is_eval} }});"));
            assert!(
                matches!(result(&hir), Expr::WorkerNew { filename, options: Some(_), is_eval: actual, .. }
                if matches!(filename.as_ref(), Expr::String(name) if name == "entry.cjs") && *actual == is_eval),
                "Worker as {local}: {:?}",
                result(&hir)
            );
        }
    }
}

#[test]
fn canonical_and_namespace_constructors_keep_their_construction_paths() {
    for namespace in [false, true] {
        let (imports, prefix) = if namespace {
            ("import * as wt from 'node:worker_threads';", "wt.")
        } else {
            (
                "import { Worker, MessageChannel, BroadcastChannel } from 'node:worker_threads';",
                "",
            )
        };
        for name in ["Worker", "MessageChannel", "BroadcastChannel"] {
            let hir = lower(&format!(
                "{imports} const result = new {prefix}{name}('entry.cjs');"
            ));
            if name == "Worker" {
                assert!(matches!(result(&hir), Expr::WorkerNew { .. }));
            } else {
                assert!(is_messaging_constructor(result(&hir), name));
            }
        }
    }
}

#[test]
fn lexical_and_type_only_bindings_do_not_become_worker_constructors() {
    for source in [
        "import { Worker as Thread } from 'node:worker_threads'; function make(Thread: any) { return new Thread(); }",
        "import { MessageChannel as Channel } from 'node:worker_threads'; function make() { const Channel = class {}; return new Channel(); }",
        "import type { Worker as Thread } from 'node:worker_threads'; const result = new Thread();",
    ] {
        let hir = lower(source);
        let dump = format!("{hir:?}");
        assert!(!dump.contains("WorkerNew"), "{dump}");
        assert!(!dump.contains("method: \"MessageChannel\""), "{dump}");
    }
}

#[test]
fn explicit_globals_ignore_colliding_import_bindings() {
    for global in ["MessageChannel", "BroadcastChannel"] {
        let hir = lower(&format!("import {{ MessageChannel as BroadcastChannel, BroadcastChannel as MessageChannel }} from 'node:worker_threads'; const result = new globalThis.{global}('name');"));
        assert!(
            !matches!(result(&hir), Expr::NativeMethodCall { method, .. } if method != global),
            "explicit global {global}: {:?}",
            result(&hir)
        );
    }
}

#[test]
fn broadcast_construction_preserves_missing_and_explicit_undefined_arguments() {
    for (arguments, expected) in [("", 0), ("undefined", 1), ("undefined, 42", 2)] {
        let hir = lower(&format!("import {{ BroadcastChannel as Channel }} from 'node:worker_threads'; const result = new Channel({arguments});"));
        assert!(matches!(result(&hir), Expr::NewDynamic { args, .. } if args.len() == expected));
    }
}

#[test]
fn imported_broadcast_construction_preserves_spread_positions() {
    let hir = lower("import { BroadcastChannel as Channel } from 'node:worker_threads'; const args = []; const result = new Channel(...args);");
    assert!(matches!(result(&hir), Expr::NewDynamicSpread { args, .. }
        if matches!(args.as_slice(), [crate::ir::CallArg::Spread(_)])));
}
