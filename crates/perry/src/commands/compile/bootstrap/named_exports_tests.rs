use super::*;
use perry_hir::{Export, Import, ImportSpecifier, Module, ModuleKind};

fn fixture(source: &str, name: &str) -> CompilationContext {
    let mut ctx = CompilationContext::new(PathBuf::from("/repo"));
    let mut importer = Module::new("main");
    importer.imports.push(Import {
        source: source.to_string(),
        specifiers: vec![ImportSpecifier::Named {
            imported: name.into(),
            local: "local_alias".into(),
        }],
        is_native: false,
        module_kind: ModuleKind::NativeCompiled,
        resolved_path: Some("/repo/target.ts".into()),
        type_only: false,
        runtime_erased: false,
        is_dynamic: false,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    });
    ctx.native_modules
        .insert(PathBuf::from("/repo/main.ts"), importer);
    let mut target = Module::new("target");
    target.exports.push(Export::Named {
        local: "existing".into(),
        exported: "existing".into(),
    });
    ctx.native_modules
        .insert(PathBuf::from("/repo/target.ts"), target);
    ctx
}

#[test]
fn missing_named_export_fails_before_codegen() {
    for source in ["./target.ts", "mini"] {
        let mut ctx = fixture(source, "missing");
        let message = enforce_static_import_exports(&mut ctx)
            .expect_err("missing export must fail before creating a linker symbol")
            .to_string();
        for expected in [source, "missing", "local_alias", "main.ts", "target.ts"] {
            assert!(message.contains(expected), "{message}");
        }
    }
}

#[test]
fn known_named_and_namespace_exports_are_accepted() {
    let mut ctx = fixture("./target.ts", "existing");
    enforce_static_import_exports(&mut ctx).unwrap();
    ctx.native_modules
        .get_mut(Path::new("/repo/target.ts"))
        .unwrap()
        .exports = vec![Export::NamespaceReExport {
        source: "node:fs".into(),
        name: "existing".into(),
    }];
    enforce_static_import_exports(&mut ctx).unwrap();
}

#[test]
fn erased_dynamic_and_require_edges_are_not_static_named_imports() {
    for flag in 0..5 {
        let mut ctx = fixture("./target.ts", "missing");
        let import = &mut ctx
            .native_modules
            .get_mut(Path::new("/repo/main.ts"))
            .unwrap()
            .imports[0];
        match flag {
            0 => import.type_only = true,
            1 => import.runtime_erased = true,
            2 => import.is_dynamic = true,
            3 => import.is_native = true,
            _ => import.is_adopted_require = true,
        }
        enforce_static_import_exports(&mut ctx).unwrap();
    }
}

fn add_module(ctx: &mut CompilationContext, name: &str, exports: Vec<Export>) {
    let path = PathBuf::from(format!("/repo/{name}.ts"));
    let mut module = Module::new(name);
    module.exports = exports;
    ctx.native_modules.insert(path.clone(), module);
    ctx.resolve_cache.insert(
        (format!("./{name}.ts"), PathBuf::from("/repo")),
        Some((path, ModuleKind::NativeCompiled)),
    );
}

#[test]
fn renamed_and_star_reexports_resolve_before_validation() {
    let mut ctx = fixture("./target.ts", "alias");
    add_module(
        &mut ctx,
        "target",
        vec![Export::ReExport {
            source: "./barrel.ts".into(),
            imported: "leaf".into(),
            exported: "alias".into(),
        }],
    );
    add_module(
        &mut ctx,
        "barrel",
        vec![Export::ExportAll {
            source: "./leaf.ts".into(),
        }],
    );
    add_module(
        &mut ctx,
        "leaf",
        vec![Export::Named {
            local: "value".into(),
            exported: "leaf".into(),
        }],
    );
    enforce_static_import_exports(&mut ctx).unwrap();
    ctx.native_modules
        .get_mut(Path::new("/repo/leaf.ts"))
        .unwrap()
        .exports
        .clear();
    assert!(enforce_static_import_exports(&mut ctx)
        .unwrap_err()
        .to_string()
        .contains("alias"));
}

#[test]
fn reexport_cycles_terminate_and_still_find_valid_exports() {
    let mut ctx = fixture("./target.ts", "missing");
    add_module(
        &mut ctx,
        "target",
        vec![Export::ExportAll {
            source: "./loop.ts".into(),
        }],
    );
    add_module(
        &mut ctx,
        "loop",
        vec![Export::ExportAll {
            source: "./target.ts".into(),
        }],
    );
    assert!(enforce_static_import_exports(&mut ctx).is_err());
    ctx.native_modules
        .get_mut(Path::new("/repo/loop.ts"))
        .unwrap()
        .exports
        .push(Export::Named {
            local: "value".into(),
            exported: "missing".into(),
        });
    enforce_static_import_exports(&mut ctx).unwrap();
}

#[test]
fn builtin_reexports_are_not_mistaken_for_missing_exports() {
    let mut ctx = fixture("./target.ts", "readFileSync");
    add_module(
        &mut ctx,
        "target",
        vec![Export::ExportAll {
            source: "node:fs".into(),
        }],
    );
    ctx.resolve_cache
        .insert(("node:fs".into(), PathBuf::from("/repo")), None);
    enforce_static_import_exports(&mut ctx).unwrap();
}

#[test]
fn cjs_default_object_does_not_imply_named_exports() {
    let mut ctx = fixture("mini", "get");
    add_module(
        &mut ctx,
        "target",
        vec![Export::Named {
            local: "exports".into(),
            exported: "default".into(),
        }],
    );
    let error = enforce_static_import_exports(&mut ctx)
        .unwrap_err()
        .to_string();
    assert!(error.contains("default import"));
    assert!(error.contains("get"));
}
