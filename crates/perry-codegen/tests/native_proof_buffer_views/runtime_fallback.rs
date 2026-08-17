//! Runtime-fallback proofs for native-owned typed arrays, split out of
//! `native_proof_buffer_views.rs` to keep that file under the 2000-line cap
//! (#8295 took it to 2036). Same toolkit, same conventions — this module only
//! holds the tests that assert an EXPLICIT runtime fallback is recorded rather
//! than a native proof.

use super::*;

#[test]
fn native_owned_typed_array_fallback_reasons_are_explicit() {
    let disposed = compile_artifact_json(
        "artifact_native_owned_disposed.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(1)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        disposed["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "use_after_dispose"
                    && record["fallback_reason"] == "use_after_dispose"
            }),
        "expected disposed native-owned view fallback reason:\n{disposed:#}"
    );

    let stale_length = compile_artifact_json(
        "artifact_native_owned_stale_length.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            number_let(3, "len", true, int(8)),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                local(3),
            ),
            Stmt::Expr(Expr::LocalSet(3, Box::new(int(4)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        stale_length["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "stale_view_length"
                    && record["fallback_reason"] == "stale_view_length"
            }),
        "expected stale native-owned view length fallback reason:\n{stale_length:#}"
    );

    let mutable_alias = compile_artifact_json(
        "artifact_native_owned_mutable_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Let {
                id: 3,
                name: "alias".to_string(),
                ty: Type::Named("Float64Array".to_string()),
                mutable: false,
                init: Some(local(2)),
            },
            Stmt::Return(Some(index_get(3, int(0)))),
        ],
    );
    assert!(
        mutable_alias["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "mutable_alias"
                    && record["fallback_reason"] == "mutable_alias"
            }),
        "expected native-owned mutable alias fallback reason:\n{mutable_alias:#}"
    );

    let missing_owner = compile_artifact_json(
        "artifact_native_owned_missing_owner_root.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), true),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::NativeArenaAlloc(Box::new(int(64)))),
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        missing_owner["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "missing_owner_root"
                    && record["fallback_reason"] == "missing_owner_root"
            }),
        "expected missing owner-root fallback reason:\n{missing_owner:#}"
    );

    let escaping = compile_artifact_json(
        "artifact_native_owned_escaping_pointer.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(extern_call(
                "escape_native_view",
                vec![local(2)],
                Type::Number,
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        escaping["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "escaping_unowned_pointer"
                    && record["fallback_reason"] == "escaping_unowned_pointer"
            }),
        "expected escaping unowned pointer fallback reason:\n{escaping:#}"
    );
}

#[test]
fn uint8_clamped_typed_array_store_records_runtime_fallback() {
    let body = vec![
        typed_array_let(
            1,
            "clamped",
            "Uint8ClampedArray",
            perry_hir::TYPED_ARRAY_KIND_UINT8_CLAMPED,
            int(8),
        ),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(local(1)),
            index: Box::new(int(0)),
            value: Box::new(number(300.5)),
        }),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_uint8_clamped_store_fallback.ts", body);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArraySet"
                    && record["consumer"] == "TypedArraySet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && !record["fallback_reason"].is_null()
            }),
        "expected Uint8ClampedArray store to stay on runtime fallback:\n{artifact:#}"
    );
}

#[test]
fn typed_array_alias_read_records_runtime_fallback() {
    let body = vec![
        typed_array_let(
            1,
            "array",
            "Uint16Array",
            perry_hir::TYPED_ARRAY_KIND_UINT16,
            int(8),
        ),
        Stmt::Let {
            id: 2,
            name: "alias".to_string(),
            ty: Type::Named("Uint16Array".to_string()),
            mutable: false,
            init: Some(local(1)),
        },
        for_loop(3, int(8), vec![Stmt::Expr(index_get(2, local(3)))]),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_typed_array_alias_fallback.ts", body);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && !record["fallback_reason"].is_null()
            }),
        "expected aliased typed-array read to record runtime fallback:\n{artifact:#}"
    );
}

#[test]
fn reassigned_typed_array_store_records_runtime_fallback() {
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "array".to_string(),
            ty: Type::Named("Int32Array".to_string()),
            mutable: true,
            init: Some(Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(int(8))),
            }),
        },
        Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(int(8))),
            }),
        )),
        array_set(1, int(0), int(42)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let artifact = compile_artifact_json("artifact_typed_array_reassign_fallback.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["consumer"] == "TypedArraySet.slow_path"
                && record["access_mode"] == "dynamic_fallback"
                && !record["fallback_reason"].is_null()
        }),
        "expected reassigned typed-array store to record runtime fallback:\n{artifact:#}"
    );
    // The read must never take an UNCHECKED native path on a reassigned
    // receiver. Two conforming lowerings exist: the runtime-call fallback
    // (`slow_path` / dynamic_fallback) and, since #6883, the inline
    // kind-GUARDED checked read (`checked_f64_param` / checked_native) —
    // whose runtime guard re-validates the receiver on every access, so a
    // reassignment can never serve stale data. What this asserts is the
    // absence of the guard-free proven/unchecked forms.
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && ((record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback")
                    || (record["consumer"] == "TypedArrayGet.checked_f64_param"
                        && record["access_mode"] == "checked_native"))
        }),
        "expected reassigned typed-array read to stay on a runtime-checked path:\n{artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && (record["consumer"] == "TypedArrayGet.proven_view_checked"
                    || record["access_mode"] == "unchecked_native")
        }),
        "reassigned typed-array read must never take a proven/unchecked form:\n{artifact:#}"
    );
}
