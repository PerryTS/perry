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
            matches!(result(&hir), Expr::NativeMethodCall { module, class_name: None, object: None, method, .. }
            if module == "worker_threads" && method == export),
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
fn canonical_and_namespace_constructors_keep_specialised_hir() {
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
                assert!(
                    matches!(result(&hir), Expr::NativeMethodCall { module, method, .. } if module == "worker_threads" && method == name)
                );
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
