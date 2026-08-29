//! Regression coverage for #9034: straight-line builder folding must preserve
//! assignment `[[Set]]` semantics when `Object.prototype` can intercept a key.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, "builder_fold_prototype_descriptor.ts", &mut cache)
            .expect("parse should succeed");
    lower_module(
        &parsed.module,
        "test",
        "builder_fold_prototype_descriptor.ts",
    )
    .expect("lower should succeed")
}

fn binding_init<'a>(module: &'a perry_hir::Module, name: &str) -> &'a Expr {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name: binding,
                init: Some(init),
                ..
            } if binding == name => Some(init),
            _ => None,
        })
        .unwrap_or_else(|| panic!("top-level binding `{name}` not found"))
}

fn has_runtime_set(module: &perry_hir::Module, key: &str) -> bool {
    module.init.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::Expr(Expr::PutValueSet { key: hir_key, .. })
                if matches!(hir_key.as_ref(), Expr::String(actual) if actual == key)
        )
    })
}

#[test]
fn object_prototype_descriptor_mutation_blocks_builder_folding() {
    let module = lower_src(
        r#"
        Object.defineProperty(Object.prototype, "hook", {
            set(v: any) { (this as any).seen = v },
            configurable: true,
        });
        const built: any = {};
        built.hook = 42;
        "#,
    );

    assert!(
        matches!(
            binding_init(&module, "built"),
            Expr::New { class_name, args, .. }
                if class_name.starts_with("__AnonShape_") && args.is_empty()
        ),
        "the empty allocation must not absorb the later assignment: {:?}",
        binding_init(&module, "built")
    );
    assert!(
        has_runtime_set(&module, "hook"),
        "the assignment must survive as runtime [[Set]]: {:?}",
        module.init
    );
}

#[test]
fn ordinary_builder_sequence_still_folds_without_a_prototype_barrier() {
    let module = lower_src(
        r#"
        const built: any = {};
        built.hook = 42;
        "#,
    );

    assert!(
        matches!(
            binding_init(&module, "built"),
            Expr::New { class_name, args, .. }
                if class_name.starts_with("__AnonShape_") && args.len() == 1
        ),
        "the ordinary builder optimization should remain enabled: {:?}",
        binding_init(&module, "built")
    );
    assert!(
        !has_runtime_set(&module, "hook"),
        "the folded assignment should be absorbed into the allocation"
    );
}
