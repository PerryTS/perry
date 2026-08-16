use super::*;

#[test]
fn backing_buffer_exposure_records_pointer_invalidation_before_fallback() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(1),
        ),
        array_set(1, int(0), int(7)),
        typed_array_let(
            2,
            "bytes",
            "Uint8Array",
            perry_hir::TYPED_ARRAY_KIND_UINT8,
            Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "buffer".to_string(),
            },
        ),
        array_set(1, int(0), int(0x01020304)),
        Stmt::Return(Some(index_get(2, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["local_id"] == 1
                && record["access_mode"] == "unchecked_native"
                && record["buffer_view_pointer_state"]["state"] == "stable"
        }),
        "expected the pre-exposure access to carry stable-pointer evidence:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["local_id"] == 1
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "mutable_alias"
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
                && record["buffer_view_pointer_state"]["reason"] == "mutable_alias"
        }),
        "expected `.buffer` exposure to invalidate the cached pointer before fallback:\n{artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["local_id"] == 1
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
                && matches!(
                    record["access_mode"].as_str(),
                    Some("unchecked_native" | "checked_native")
                )
        }),
        "the invalidated words pointer must not reach a native access:\n{artifact:#}"
    );
}

#[test]
fn computed_backing_buffer_exposure_invalidates_cached_pointer() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(1),
        ),
        Stmt::Expr(index_get(1, Expr::String("buffer".to_string()))),
        array_set(1, int(0), int(0x01020304)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("computed_buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["local_id"] == 1
                && record["access_mode"] == "dynamic_fallback"
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
        }),
        "computed `words[\"buffer\"]` must invalidate the cached pointer before fallback:\n\
         {artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["local_id"] == 1
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
                && matches!(
                    record["access_mode"].as_str(),
                    Some("unchecked_native" | "checked_native")
                )
        }),
        "the invalidated computed-property receiver must not reach native access:\n{artifact:#}"
    );
}

#[test]
fn named_typed_array_getter_invalidates_cached_pointer() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(1),
        ),
        // A non-canonical named key uses ordinary [[Get]]. It can therefore
        // invoke an accessor that reads `this.buffer` and rebinds storage.
        Stmt::Expr(index_get(1, Expr::String("probe".to_string()))),
        array_set(1, int(0), int(0x01020304)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("named_getter_buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["local_id"] == 1
                && record["access_mode"] == "dynamic_fallback"
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
        }),
        "a named typed-array getter must invalidate the cached pointer before [[Get]]:\n\
         {artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["local_id"] == 1
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
                && matches!(
                    record["access_mode"].as_str(),
                    Some("unchecked_native" | "checked_native")
                )
        }),
        "the pointer invalidated by a named getter must not reach native access:\n{artifact:#}"
    );
}

#[test]
fn canonical_numeric_string_get_preserves_cached_pointer() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(1),
        ),
        Stmt::Expr(index_get(1, Expr::String("0".to_string()))),
        array_set(1, int(0), int(7)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("numeric_string_buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["local_id"] == 1
                && record["access_mode"] == "dynamic_fallback"
                && record["buffer_view_pointer_state"]["state"] == "stable"
        }),
        "canonical numeric strings cannot invoke a named accessor:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["local_id"] == 1
                && matches!(
                    record["access_mode"].as_str(),
                    Some("unchecked_native" | "checked_native")
                )
                && record["buffer_view_pointer_state"]["state"] == "stable"
        }),
        "a canonical numeric-string get should preserve later native access:\n{artifact:#}"
    );
}

#[test]
fn backing_buffer_exposure_invalidates_copied_view_alias_group() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(1),
        ),
        Stmt::Let {
            id: 2,
            name: "alias".to_string(),
            ty: Type::Named("Uint32Array".to_string()),
            mutable: false,
            init: Some(local(1)),
        },
        Stmt::Expr(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(local(2)),
            property: "buffer".to_string(),
        }),
        array_set(1, int(0), int(0x01020304)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("aliased_buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["local_id"] == 1
                && record["access_mode"] == "dynamic_fallback"
                && record["buffer_view_pointer_state"]["state"] == "invalidated"
        }),
        "exposing an alias's backing buffer must invalidate every cached pointer sharing its \
         data slot:\n{artifact:#}"
    );
}

#[test]
fn native_memory_bulk_access_rejects_invalidated_cached_pointer() {
    let body = vec![
        typed_array_let(
            1,
            "words",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(4),
        ),
        Stmt::Expr(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "buffer".to_string(),
        }),
        Stmt::Expr(Expr::NativeMemoryFillU32 {
            view: Box::new(local(1)),
            value: Box::new(int(0)),
        }),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let mut opts = empty_opts();
    opts.verify_native_regions = true;
    let artifact = compile_artifact_json_for_module_with_opts(
        module("native_memory_buffer_pointer_lifetime_7220.ts", body),
        opts,
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeMemoryFillU32"
                && record["consumer"] == "NativeMemoryFillU32.runtime_fallback"
                && record["access_mode"] == "dynamic_fallback"
        }),
        "bulk access through an invalidated cached pointer must use the runtime fallback:\n\
         {artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "NativeMemoryFillU32"
                && matches!(
                    record["consumer"].as_str(),
                    Some("NativeMemoryFillU32.memset_zero" | "NativeMemoryFillU32.store_loop")
                )
        }),
        "bulk access through an invalidated cached pointer must not emit native writes:\n\
         {artifact:#}"
    );
}
