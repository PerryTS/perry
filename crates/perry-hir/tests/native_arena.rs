use perry_diagnostics::SourceCache;
use perry_hir::types::Type;
use perry_hir::{lower_module, Expr, Function, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Result<Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "native_arena_test.ts", &mut cache)
                .map_err(|e| e.to_string())?;
            lower_module(&parsed.module, "test", "native_arena_test.ts").map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn assert_lower_err_eq(src: &str, expected: &str) {
    let err = lower_src(src).expect_err("lowering should fail");
    assert_eq!(err, expected);
}

fn find_let<'m>(module: &'m Module, name: &str) -> &'m Stmt {
    module
        .init
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Let { name: n, .. } if n == name))
        .unwrap_or_else(|| panic!("let `{}` not found in init: {:?}", name, module.init))
}

fn find_function<'m>(module: &'m Module, name: &str) -> &'m Function {
    module
        .functions
        .iter()
        .find(|func| func.name == name)
        .unwrap_or_else(|| panic!("function `{}` not found: {:?}", name, module.functions))
}

fn returned_expr(func: &Function) -> &Expr {
    match func.body.as_slice() {
        [Stmt::Return(Some(expr))] => expr,
        other => panic!("expected single return in `{}`: {:?}", func.name, other),
    }
}

fn expr_any(expr: &Expr, pred: &impl Fn(&Expr) -> bool) -> bool {
    if pred(expr) {
        return true;
    }
    match expr {
        Expr::NativeArenaAlloc(inner) | Expr::NativeArenaDispose(inner) => expr_any(inner, pred),
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => expr_any(owner, pred) || expr_any(byte_offset, pred) || expr_any(length, pred),
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => expr_any(owner, pred) || expr_any(byte_offset, pred) || expr_any(count, pred),
        Expr::NativeMemoryFillU32 { view, value } => expr_any(view, pred) || expr_any(value, pred),
        Expr::NativeMemoryCopy { dst, src } => expr_any(dst, pred) || expr_any(src, pred),
        Expr::Call { callee, args, .. } => {
            expr_any(callee, pred) || args.iter().any(|arg| expr_any(arg, pred))
        }
        Expr::PropertyGet { object, .. } => expr_any(object, pred),
        Expr::Object(fields) => fields.iter().any(|(_, value)| expr_any(value, pred)),
        Expr::Array(items) => items.iter().any(|item| expr_any(item, pred)),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            expr_any(left, pred) || expr_any(right, pred)
        }
        Expr::LocalSet(_, value) => expr_any(value, pred),
        _ => false,
    }
}

fn stmt_any(stmt: &Stmt, pred: &impl Fn(&Expr) -> bool) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(init), ..
        }
        | Stmt::Expr(init) => expr_any(init, pred),
        Stmt::Return(Some(value)) => expr_any(value, pred),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_any(condition, pred)
                || then_branch.iter().any(|s| stmt_any(s, pred))
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(|s| stmt_any(s, pred)))
        }
        _ => false,
    }
}

fn module_any(module: &Module, pred: impl Fn(&Expr) -> bool) -> bool {
    module.init.iter().any(|stmt| stmt_any(stmt, &pred))
        || module
            .functions
            .iter()
            .any(|func| func.body.iter().any(|stmt| stmt_any(stmt, &pred)))
}

#[test]
fn native_arena_public_alloc_view_and_dispose_lower() {
    let module = lower_src(
        r#"
        const arena = NativeArena.alloc(64);
        const view = arena.view(Float64Array, 0, 8);
        const bytes = arena.view("Uint8Array", 0, 8);
        arena.dispose();
        "#,
    )
    .expect("lowering should succeed");

    assert!(matches!(
        find_let(&module, "arena"),
        Stmt::Let {
            init: Some(Expr::NativeArenaAlloc(_)),
            ty: Type::Named(name),
            ..
        } if name == "NativeArena"
    ));
    assert!(matches!(
        find_let(&module, "view"),
        Stmt::Let {
            init: Some(Expr::NativeArenaView { kind, .. }),
            ty: Type::Named(name),
            ..
        } if *kind == perry_hir::TYPED_ARRAY_KIND_FLOAT64 && name == "Float64Array"
    ));
    assert!(matches!(
        find_let(&module, "bytes"),
        Stmt::Let {
            init: Some(Expr::NativeArenaView { kind, .. }),
            ty: Type::Named(name),
            ..
        } if *kind == perry_hir::TYPED_ARRAY_KIND_UINT8 && name == "Uint8Array"
    ));
    assert!(module_any(&module, |expr| matches!(
        expr,
        Expr::NativeArenaDispose(_)
    )));
}

#[test]
fn native_arena_public_pod_view_lowers_with_annotation() {
    let module = lower_src(
        r#"
        type Packet = PerryPod<{ tag: PerryU32; gain: PerryF32; }>;
        const arena = NativeArena.alloc(16);
        const view: PerryPodView<Packet> = arena.podView(0, 1);
        "#,
    )
    .expect("lowering should succeed");

    assert!(matches!(
        find_let(&module, "view"),
        Stmt::Let {
            init: Some(Expr::NativePodView {
                view_type: None,
                ..
            }),
            ty: Type::Generic { base, .. },
            ..
        } if base == "PerryPodView"
    ));
}

#[test]
fn native_arena_public_pod_view_lowers_explicit_type_arg() {
    let module = lower_src(
        r#"
        type Packet = PerryPod<{ tag: PerryU32; gain: PerryF32; }>;
        const arena = NativeArena.alloc(16);
        const view = arena.podView<Packet>(0, 1);
        "#,
    )
    .expect("lowering should succeed");

    match find_let(&module, "view") {
        Stmt::Let {
            init: Some(Expr::NativePodView { view_type, .. }),
            ty,
            ..
        } => {
            assert_eq!(view_type.as_ref(), Some(ty));
            assert!(matches!(
                ty,
                Type::Generic { base, type_args }
                    if base == "PerryPodView"
                        && matches!(
                            type_args.as_slice(),
                            [Type::Generic { base, .. }] if base == "PerryPod"
                        )
            ));
        }
        other => panic!("expected NativePodView let, got {other:?}"),
    }
}

#[test]
fn native_arena_public_pod_view_preserves_generic_type_param() {
    let module = lower_src(
        r#"
        function viewGeneric<T extends PerryPod<any>>(arena: NativeArena) {
            return arena.podView<T>(0, 1);
        }
        "#,
    )
    .expect("lowering should succeed");

    let expected_view_type = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![Type::TypeVar("T".to_string())],
    };
    assert!(matches!(
        returned_expr(find_function(&module, "viewGeneric")),
        Expr::NativePodView { view_type, .. } if view_type.as_ref() == Some(&expected_view_type)
    ));
}

#[test]
fn native_arena_public_api_rejects_spread_arguments() {
    assert_lower_err_eq(
        r#"
        const args: any = [64];
        const arena = NativeArena.alloc(...args);
        "#,
        "NativeArena.alloc(byteLength) does not accept spread arguments",
    );

    assert_lower_err_eq(
        r#"
        const arena = NativeArena.alloc(64);
        const args: any = [Float64Array, 0, 8];
        const view = arena.view(...args);
        "#,
        "NativeArena.view(kind, byteOffset, length) does not accept spread arguments",
    );

    assert_lower_err_eq(
        r#"
        const arena = NativeArena.alloc(16);
        const args: any = [0, 1];
        const view = arena.podView(...args);
        "#,
        "NativeArena.podView(byteOffset, count) does not accept spread arguments",
    );

    assert_lower_err_eq(
        r#"
        const arena = NativeArena.alloc(16);
        const args: any = [];
        arena.dispose(...args);
        "#,
        "NativeArena.dispose() does not accept spread arguments",
    );
}

#[test]
fn pod_layout_constants_lower_to_compile_time_hir_nodes() {
    let module = lower_src(
        r#"
        type Header = PerryPod<{ code: PerryU32; flags: PerryU32; }>;
        type Packet = PerryPod<{ tag: PerryU32; header: Header; total: number; }>;
        const packetSize = sizeof<Packet>();
        const packetAlign = alignof<Packet>();
        const totalOffset = offsetof<Packet>("total");
        const nestedOffset = offsetof<Packet>("header.flags");
        "#,
    )
    .expect("lowering should succeed");

    assert!(matches!(
        find_let(&module, "packetSize"),
        Stmt::Let {
            init: Some(Expr::PodLayoutSizeOf { .. }),
            ..
        }
    ));
    assert!(matches!(
        find_let(&module, "packetAlign"),
        Stmt::Let {
            init: Some(Expr::PodLayoutAlignOf { .. }),
            ..
        }
    ));
    assert!(matches!(
        find_let(&module, "totalOffset"),
        Stmt::Let {
            init: Some(Expr::PodLayoutOffsetOf { field_path, .. }),
            ..
        } if field_path == &vec!["total".to_string()]
    ));
    assert!(matches!(
        find_let(&module, "nestedOffset"),
        Stmt::Let {
            init: Some(Expr::PodLayoutOffsetOf { field_path, .. }),
            ..
        } if field_path == &vec!["header".to_string(), "flags".to_string()]
    ));
}

#[test]
fn native_arena_pod_layout_constants_preserve_generic_pod_type_param() {
    let module = lower_src(
        r#"
        function sizeOfGeneric<T extends PerryPod<any>>() {
            return sizeof<T>();
        }
        function alignOfGeneric<T extends PerryPod<any>>() {
            return alignof<T>();
        }
        function offsetOfGeneric<T extends PerryPod<any>>() {
            return offsetof<T>("field");
        }
        "#,
    )
    .expect("lowering should succeed");

    let type_var_t = Type::TypeVar("T".to_string());
    assert!(matches!(
        returned_expr(find_function(&module, "sizeOfGeneric")),
        Expr::PodLayoutSizeOf { ty } if ty == &type_var_t
    ));
    assert!(matches!(
        returned_expr(find_function(&module, "alignOfGeneric")),
        Expr::PodLayoutAlignOf { ty } if ty == &type_var_t
    ));
    assert!(matches!(
        returned_expr(find_function(&module, "offsetOfGeneric")),
        Expr::PodLayoutOffsetOf { ty, field_path }
            if ty == &type_var_t && field_path == &vec!["field".to_string()]
    ));
}

#[test]
fn pod_layout_constants_respect_shadowing() {
    let module = lower_src(
        r#"
        type Packet = PerryPod<{ tag: PerryU32; }>;
        const sizeof = <T>() => 7;
        const value = sizeof<Packet>();
        "#,
    )
    .expect("lowering should succeed");

    assert!(matches!(
        find_let(&module, "value"),
        Stmt::Let {
            init: Some(Expr::Call { .. }),
            ..
        }
    ));
    assert!(!module_any(&module, |expr| matches!(
        expr,
        Expr::PodLayoutSizeOf { .. }
            | Expr::PodLayoutAlignOf { .. }
            | Expr::PodLayoutOffsetOf { .. }
    )));
}

#[test]
fn pod_layout_constants_reject_dynamic_offset_path() {
    let err = lower_src(
        r#"
        type Packet = PerryPod<{ tag: PerryU32; }>;
        const field = "tag";
        const value = offsetof<Packet>(field);
        "#,
    )
    .expect_err("dynamic offsetof selector should fail lowering");

    assert!(
        err.contains("offsetof<T>(field) requires a compile-time string-literal field path"),
        "unexpected error: {err}"
    );
}

#[test]
fn pod_layout_constants_require_explicit_type_arg() {
    let err = lower_src(
        r#"
        const value = sizeof();
        "#,
    )
    .expect_err("missing type arg should fail lowering");

    assert!(
        err.contains("sizeof<T>() requires exactly one explicit PerryPod type argument"),
        "unexpected error: {err}"
    );
}

#[test]
fn native_arena_public_api_respects_shadowing() {
    let module = lower_src(
        r#"
        const NativeArena: any = { alloc: (byteLength: number) => byteLength };
        const value = NativeArena.alloc(64);
        "#,
    )
    .expect("lowering should succeed");

    assert!(!module_any(&module, |expr| matches!(
        expr,
        Expr::NativeArenaAlloc(_)
            | Expr::NativeArenaView { .. }
            | Expr::NativePodView { .. }
            | Expr::NativeArenaDispose(_)
    )));

    let spread_module = lower_src(
        r#"
        const NativeArena: any = { alloc: 1 };
        const args: any = [64];
        const value = NativeArena.alloc(...args);
        "#,
    )
    .expect("shadowed spread call should use generic call lowering");

    assert!(!module_any(&spread_module, |expr| matches!(
        expr,
        Expr::NativeArenaAlloc(_)
            | Expr::NativeArenaView { .. }
            | Expr::NativePodView { .. }
            | Expr::NativeArenaDispose(_)
    )));
}

#[test]
fn native_memory_public_api_lowers_direct_global_calls() {
    let module = lower_src(
        r#"
        const arena = NativeArena.alloc(64);
        const words = arena.view(Uint32Array, 0, 16);
        const dst = new Uint32Array(16);
        NativeMemory.fillU32(words, 0);
        NativeMemory.copy(dst, words);
        "#,
    )
    .expect("lowering should succeed");

    assert!(module_any(&module, |expr| matches!(
        expr,
        Expr::NativeMemoryFillU32 { .. }
    )));
    assert!(module_any(&module, |expr| matches!(
        expr,
        Expr::NativeMemoryCopy { .. }
    )));
}

#[test]
fn native_memory_public_api_respects_shadowing() {
    let module = lower_src(
        r#"
        const NativeMemory: any = { fillU32: () => 1, copy: () => 2 };
        const words = new Uint32Array(4);
        NativeMemory.fillU32(words, 0);
        NativeMemory.copy(words, words);
        "#,
    )
    .expect("lowering should succeed");

    assert!(!module_any(&module, |expr| matches!(
        expr,
        Expr::NativeMemoryFillU32 { .. } | Expr::NativeMemoryCopy { .. }
    )));
}

#[test]
fn native_memory_public_api_rejects_spread_and_wrong_arity() {
    let spread_err = lower_src(
        r#"
        const words = new Uint32Array(4);
        const args: any = [words, 0];
        NativeMemory.fillU32(...args);
        "#,
    )
    .expect_err("spread should fail lowering");
    assert!(
        spread_err.contains("NativeMemory.fillU32(view, value) does not accept spread arguments"),
        "unexpected error: {spread_err}"
    );

    let arity_err = lower_src(
        r#"
        const words = new Uint32Array(4);
        NativeMemory.copy(words);
        "#,
    )
    .expect_err("wrong arity should fail lowering");
    assert!(
        arity_err.contains("NativeMemory.copy(dst, src) expects exactly two arguments"),
        "unexpected error: {arity_err}"
    );
}

#[test]
fn native_arena_public_view_rejects_dynamic_kind() {
    let err = lower_src(
        r#"
        const arena = NativeArena.alloc(64);
        const Kind = Float64Array;
        const view = arena.view(Kind, 0, 8);
        "#,
    )
    .expect_err("dynamic kind should fail lowering");

    assert!(
        err.contains("NativeArena.view kind must be a typed-array constructor or string literal"),
        "unexpected error: {err}"
    );
}
