use super::*;

fn lower(path: &str, source: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(source, path).unwrap();
    perry_hir::lower_module(&ast, path, path).unwrap()
}

fn worker_graph() -> super::super::CompilationContext {
    let mut ctx = super::super::CompilationContext::new(PathBuf::from("/fixture"));
    let entry = "/fixture/main.ts";
    let mut module = lower(
        entry,
        r#"
        import { Worker } from "node:worker_threads";
        import "./file.js";
        import "./eval.js";
        import "./dead.js";
        new Worker("./file.js");
        new Worker("require('node:worker_threads');", { eval: true });
        function neverCalled() { return import("./dead.js"); }
    "#,
    );
    perry_hir::for_each_worker_new_mut(&mut module, &mut |expr| {
        if let perry_hir::Expr::WorkerNew { paths, is_eval, .. } = expr {
            *paths = vec![if *is_eval { "./eval.js" } else { "./file.js" }.into()];
        }
    });
    for import in &mut module.imports {
        if import.source.starts_with("./") {
            import.resolved_path = Some(format!("/fixture/{}", &import.source[2..]));
            import.is_dynamic = true;
        }
    }
    ctx.native_modules.insert(PathBuf::from(entry), module);
    for path in ["file.js", "eval.js", "dead.js", "helper.js"] {
        let full = format!("/fixture/{path}");
        let source = if path == "file.js" {
            r#"import "./helper.js"; import("./helper.js");"#
        } else {
            "export const value = 1;"
        };
        let mut module = lower(&full, source);
        for import in &mut module.imports {
            import.resolved_path = Some("/fixture/helper.js".into());
            import.is_dynamic = true;
        }
        perry_hir::for_each_dynamic_import_mut(&mut module, &mut |expr| {
            if let perry_hir::Expr::DynamicImport { paths, .. } = expr {
                *paths = vec!["./helper.js".into()];
            }
        });
        ctx.native_modules.insert(PathBuf::from(full), module);
    }
    ctx
}

#[test]
fn worker_roots_include_file_eval_and_deduplicated_url_spellings() {
    let mut ctx = worker_graph();
    ctx.worker_url_entries = vec![
        ("/fixture/file.js".into(), "/fixture/file.js".into()),
        ("/fixture/url.js".into(), "/fixture/canonical-url.js".into()),
    ];
    assert_eq!(
        module_roots(&ctx),
        vec![
            "/fixture/canonical-url.js",
            "/fixture/eval.js",
            "/fixture/file.js",
            "/fixture/url.js",
        ]
    );
}

#[test]
fn worker_activation_retains_owned_import_but_prunes_unrelated_dead_import() {
    let mut ctx = worker_graph();
    let mut roots = vec![PathBuf::from("/fixture/main.ts")];
    roots.extend(module_roots(&ctx).into_iter().map(PathBuf::from));
    super::super::dynamic_reach::prune(&mut ctx, &roots).unwrap();
    for path in ["main.ts", "file.js", "eval.js", "helper.js"] {
        assert!(ctx
            .native_modules
            .contains_key(&PathBuf::from(format!("/fixture/{path}"))));
    }
    assert!(!ctx
        .native_modules
        .contains_key(&PathBuf::from("/fixture/dead.js")));
}
