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
