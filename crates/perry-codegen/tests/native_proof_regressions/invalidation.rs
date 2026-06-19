use super::*;

fn block_between<'a>(ir: &'a str, start: &str, end: &str) -> &'a str {
    let start_pos = ir.find(start).unwrap_or_else(|| {
        panic!("missing block start marker {start:?} in IR:\n{ir}");
    });
    let after_start = &ir[start_pos + 1..];
    let end_pos = after_start.find(end).unwrap_or_else(|| {
        panic!("missing block end marker {end:?} after {start:?} in IR:\n{ir}");
    });
    &after_start[..end_pos]
}

#[test]
fn localset_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(increment(3)),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_min_length_facts() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", true, Expr::MathMin(vec![length(1), length(2)])),
        Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
        for_loop(4, local(3), vec![buffer_set(2, local(4))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("min_length_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_active_bounded_buffer_index_facts() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop(
            3,
            local(1),
            vec![
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(2, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("bounded_buffer_index_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inner_loop_bounded_buffer_fact_is_removed_after_outer_fact_invalidation() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "a", local(1)),
        buffer_let(3, "b", int(8)),
        for_loop(
            4,
            local(1),
            vec![
                for_loop(
                    5,
                    length(3),
                    vec![Stmt::Expr(Expr::LocalSet(4, Box::new(int(16))))],
                ),
                buffer_set(3, local(5)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("nested_loop_scope_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(Expr::LocalSet(1, Box::new(int(16)))),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(increment(1)),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(-1),
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_loop_counter_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn decrementing_loop_update_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(0),
            length(1),
            Some(decrement(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("decrementing_loop_update_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn body_counter_mutation_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![Stmt::Expr(decrement(2)), buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("body_counter_mutation_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inclusive_length_loop_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_length_loop_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        cond_ir.contains("icmp sle i32"),
        "`i <= buf.length` with hoisted i32 length must lower as signed <=:\n{cond_ir}"
    );
    assert!(
        !cond_ir.contains("icmp slt i32"),
        "`i <= buf.length` must not be narrowed to signed <:\n{cond_ir}"
    );
}

#[test]
fn inclusive_array_length_write_uses_extension_capable_index_set_path() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![array_set(1, local(2), local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_array_length_write.ts", body);
    assert!(
        ir.contains("\nidxset.check_cap."),
        "`arr[i]` under `i <= arr.length` must keep the capacity check path:\n{ir}"
    );
    assert!(
        ir.contains("\nidxset.extend_inline."),
        "`arr[i]` under `i <= arr.length` must keep the inline length-extension path:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_array_set_f64_extend"),
        "`arr[i]` under `i <= arr.length` must keep the realloc-capable fallback:\n{ir}"
    );
}

fn array_alias_let(id: u32, name: &str, source_id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: false,
        init: Some(local(source_id)),
    }
}

fn assert_no_packed_f64_loop(ir: &str) {
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "invalidated array proof must not emit a packed-f64 loop guard:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_f64_fast"),
        "invalidated array proof must not emit the packed-f64 fast clone:\n{ir}"
    );
}

fn assert_no_packed_f64_loop_artifacts(artifact: &serde_json::Value) {
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["consumer"].as_str(),
                Some(
                    "packed_f64_loop_guard"
                        | "packed_f64_loop_load"
                        | "packed_f64_loop_store"
                        | "packed_f64_loop_store_side_exit"
                )
            ) || record["expr_kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("PackedF64Loop"))
                || record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                    && record["consumer"]
                        .as_str()
                        .is_some_and(|consumer| consumer.starts_with("packed_f64_loop"))
        }),
        "invalidated alias mutation must not emit packed-f64 loop artifact records:\n{artifact:#}"
    );
}

fn assert_no_packed_i32_loop(ir: &str) {
    assert!(
        !ir.contains("for.packed_i32_fast"),
        "invalidated array proof must not emit a packed-i32 fast clone:\n{ir}"
    );
}

fn assert_no_packed_u32_loop(ir: &str) {
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_u32_array_loop_guard"),
        "invalidated array proof must not emit a packed-u32 loop guard:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_u32_fast"),
        "invalidated array proof must not emit a packed-u32 fast clone:\n{ir}"
    );
}

fn assert_no_packed_i32_loop_artifacts(artifact: &serde_json::Value) {
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["consumer"].as_str(),
                Some(
                    "packed_i32_loop_guard"
                        | "packed_i32_loop_fallback"
                        | "packed_i32_loop_load"
                        | "packed_i32_loop_load_f64"
                )
            ) || record["expr_kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("PackedI32Loop"))
                || record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_i32")
        }),
        "invalidated alias mutation must not emit packed-i32 loop artifact records:\n{artifact:#}"
    );
}

fn assert_no_packed_u32_loop_artifacts(artifact: &serde_json::Value) {
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["consumer"].as_str(),
                Some(
                    "packed_u32_loop_guard"
                        | "packed_u32_loop_fallback"
                        | "packed_u32_loop_load"
                        | "packed_u32_loop_load_f64"
                )
            ) || record["expr_kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("PackedU32Loop"))
                || record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_u32")
        }),
        "invalidated alias mutation must not emit packed-u32 loop artifact records:\n{artifact:#}"
    );
}

fn record_has_effect_fact(
    record: &serde_json::Value,
    list: &str,
    state: &str,
    detail: &str,
) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts.iter().any(|fact| {
            fact["kind"] == "effect"
                && fact["state"] == state
                && fact["fact_id"]
                    .as_str()
                    .is_some_and(|fact_id| fact_id.ends_with(detail))
        })
    })
}

fn packed_read_sum_loop_body(prefix: Vec<Stmt>) -> Vec<Stmt> {
    let mut body = vec![number_array_let(1, "arr", vec![1, 2, 3])];
    body.extend(prefix);
    body.extend([
        number_let(3, "sum", true, int(0)),
        for_loop(
            4,
            length(1),
            vec![Stmt::Expr(Expr::LocalSet(
                3,
                Box::new(add(local(3), index_get(1, local(4)))),
            ))],
        ),
        Stmt::Return(Some(local(3))),
    ]);
    body
}

#[test]
fn packed_f64_read_loop_uses_stable_noalias_array_proof() {
    let ir = compile_ir(
        "packed_f64_read_loop_stable_array.ts",
        packed_read_sum_loop_body(Vec::new()),
    );

    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "stable noalias numeric array should get a packed-f64 loop guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_f64_fast"),
        "stable noalias numeric array should emit the packed-f64 fast clone:\n{ir}"
    );
}

#[test]
fn packed_i32_read_loop_uses_i32_specific_loop_guard_and_no_slow_helper_in_fast_clone() {
    let body = vec![
        int32_array_let(1, "arr", vec![1, 2, 3]),
        number_let(3, "sum", true, int(0)),
        for_loop(
            4,
            length(1),
            vec![Stmt::Expr(Expr::LocalSet(
                3,
                Box::new(add(local(3), index_get(1, local(4)))),
            ))],
        ),
        Stmt::Return(Some(local(3))),
    ];

    let ir = compile_ir("packed_i32_read_loop_stable_array.ts", body);
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard"),
        "stable noalias Int32[] should get a packed-i32 loop guard:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "packed-i32 proof must not reuse the f64 loop guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_i32_fast"),
        "stable noalias Int32[] should emit the packed-i32 fast clone:\n{ir}"
    );
    let fast_clone = block_between(
        &ir,
        "\nfor.packed_i32_fast.cond.",
        "\nfor.packed_i32_fast.exit.",
    );
    assert!(
        !fast_clone.contains("js_typed_feedback_array_index_get_fallback_boxed")
            && !fast_clone.contains("js_array_get_f64"),
        "packed-i32 fast clone should use raw-slot loads without slow helpers:\n{fast_clone}"
    );
}

#[test]
fn packed_f64_read_loop_rejects_prior_array_alias() {
    let ir = compile_ir(
        "packed_f64_read_loop_alias_hazard.ts",
        packed_read_sum_loop_body(vec![array_alias_let(2, "alias", 1)]),
    );

    assert_no_packed_f64_loop(&ir);
}

#[test]
fn preloop_dynamic_call_invalidates_cached_and_packed_array_proofs() {
    let body = packed_read_sum_loop_body(vec![Stmt::Expr(extern_call(
        "native_touch",
        Vec::new(),
        Type::Void,
    ))]);
    let opts = native_library_opts(vec![("native_touch", vec![], "void")]);

    let ir = compile_ir_with_opts("preloop_dynamic_call_array_hazard.ts", body, opts);
    assert_no_packed_f64_loop(&ir);
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        cond_ir.contains("plen."),
        "pre-loop dynamic escape should block cached array length reuse:\n{cond_ir}"
    );
}

fn assert_array_alias_blocks_loop_proof(ir: &str) {
    let cond_ir = block_between(ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        cond_ir.contains("plen."),
        "aliased array loop must keep a live length read in the condition:\n{cond_ir}"
    );
    assert!(
        ir.contains("\nidxset.check_cap."),
        "aliased array loop must keep the checked IndexSet path:\n{ir}"
    );
    assert!(
        !ir.contains("\nidxset.bounded_numeric_fast."),
        "aliased array loop must not install bounded-index facts:\n{ir}"
    );
}

fn aliased_array_loop(mutator: Expr) -> Vec<Stmt> {
    vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        array_alias_let(2, "alias", 1),
        for_loop(
            3,
            length(1),
            vec![Stmt::Expr(mutator), array_set(1, local(3), local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ]
}

#[test]
fn local_array_alias_push_blocks_length_and_bounds_proofs() {
    let body = aliased_array_loop(Expr::ArrayPush {
        array_id: 2,
        value: Box::new(int(1)),
    });

    let ir = compile_ir("array_alias_push_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn local_array_alias_pop_blocks_length_and_bounds_proofs() {
    let body = aliased_array_loop(Expr::ArrayPop(2));

    let ir = compile_ir("array_alias_pop_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn local_array_alias_splice_blocks_length_and_bounds_proofs() {
    let body = aliased_array_loop(Expr::ArraySplice {
        array_id: 2,
        start: Box::new(int(0)),
        delete_count: Some(Box::new(int(0))),
        items: vec![int(1)],
    });

    let ir = compile_ir("array_alias_splice_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn local_array_alias_length_set_blocks_length_and_bounds_proofs() {
    let body = aliased_array_loop(Expr::PropertySet {
        object: Box::new(local(2)),
        property: "length".to_string(),
        value: Box::new(int(0)),
    });

    let ir = compile_ir("array_alias_length_set_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn indirect_array_alias_from_container_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        Stmt::Let {
            id: 5,
            name: "box".to_string(),
            ty: Type::Array(Box::new(Type::Array(Box::new(Type::Number)))),
            mutable: false,
            init: Some(Expr::Array(vec![local(1)])),
        },
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Let {
                    id: 6,
                    name: "alias".to_string(),
                    ty: Type::Array(Box::new(Type::Number)),
                    mutable: false,
                    init: Some(index_get(5, int(0))),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 6,
                    value: Box::new(int(1)),
                }),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_container_alias_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn direct_array_length_set_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(local(1)),
                    property: "length".to_string(),
                    value: Box::new(int(0)),
                }),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_length_set_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn non_mutating_array_alias_preserves_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        array_alias_let(2, "alias", 1),
        for_loop(
            3,
            length(1),
            vec![array_set(1, local(3), local(3)), Stmt::Expr(local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_non_mutating_alias_keeps_loop_proof.ts", body);
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        !cond_ir.contains("plen."),
        "non-mutating alias should not force a live length read in the condition:\n{cond_ir}"
    );
    assert!(
        ir.contains("\nidxset.bounded_numeric_fast."),
        "non-mutating alias should keep the bounded IndexSet path:\n{ir}"
    );
}

#[test]
fn loop_length_effect_artifact_records_consumed_preservation_fact() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(length(1)),
                array_set(1, local(2), add(local(2), int(1))),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_loop_length_effect_preserves.ts", body.clone());
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        !cond_ir.contains("plen."),
        "accepted length effect should keep the hoisted length slot:\n{cond_ir}"
    );
    assert!(
        ir.contains("\nidxset.bounded_numeric_fast."),
        "accepted length effect should keep bounded IndexSet facts:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_array_loop_length_effect_preserves.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "loop_array_length_effect"
                && record_has_effect_fact(
                    record,
                    "consumed_facts",
                    "consumed",
                    "preserves_array_length",
                )
                && record_has_note(record, "loop_length_proof=accepted")
        }),
        "expected accepted loop length effect artifact:\n{artifact:#}"
    );
}

#[test]
fn async_microtask_effect_blocks_length_and_bounds_proofs_with_artifact_reason() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(Expr::Await(Box::new(Expr::Undefined))),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_await_blocks_loop_proof.ts", body.clone());
    assert_array_alias_blocks_loop_proof(&ir);

    let artifact = compile_artifact_json("artifact_array_await_blocks_loop_proof.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "loop_array_length_effect"
                && record_has_effect_fact(
                    record,
                    "rejected_facts",
                    "rejected",
                    "async_microtask_escape",
                )
                && record_has_note(record, "loop_length_proof=rejected")
        }),
        "expected rejected async/microtask loop length effect artifact:\n{artifact:#}"
    );
}

#[test]
fn local_array_alias_generic_receiver_call_blocks_length_and_bounds_proofs() {
    let body = aliased_array_loop(call(
        Expr::PropertyGet {
            object: Box::new(local(2)),
            property: "push".to_string(),
        },
        vec![int(1)],
    ));

    let ir = compile_ir("array_alias_generic_call_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn generic_call_blocks_length_and_bounds_proofs_even_without_direct_array_arg() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(extern_call("native_touch", Vec::new(), Type::Void)),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];
    let opts = native_library_opts(vec![("native_touch", vec![], "void")]);

    let ir = compile_ir_with_opts("array_unknown_call_blocks_loop_proof.ts", body, opts);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn nested_array_escape_to_call_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(extern_call(
                    "native_touch",
                    vec![Expr::Array(vec![local(1)])],
                    Type::Void,
                )),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];
    let opts = native_library_opts(vec![("native_touch", vec!["jsvalue"], "void")]);

    let ir = compile_ir_with_opts("array_nested_escape_call_blocks_loop_proof.ts", body, opts);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn object_nested_array_escape_to_call_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(extern_call(
                    "native_touch",
                    vec![Expr::Object(vec![("arr".to_string(), local(1))])],
                    Type::Void,
                )),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];
    let opts = native_library_opts(vec![("native_touch", vec!["jsvalue"], "void")]);

    let ir = compile_ir_with_opts("array_object_escape_call_blocks_loop_proof.ts", body, opts);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn native_method_call_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                Stmt::Expr(native_module_call("process", "cwd", Vec::new())),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("array_native_call_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn loop_local_array_alias_blocks_length_and_bounds_proofs() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop(
            2,
            length(1),
            vec![
                array_alias_let(3, "alias", 1),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 3,
                    value: Box::new(int(1)),
                }),
                array_set(1, local(2), local(2)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("loop_local_array_alias_blocks_loop_proof.ts", body);
    assert_array_alias_blocks_loop_proof(&ir);
}

#[test]
fn loop_local_array_alias_push_blocks_packed_f64_loop_and_artifacts() {
    let body = vec![
        number_array_let(1, "arr", vec![1, 2, 3]),
        number_let(3, "sum", true, int(0)),
        for_loop(
            4,
            length(1),
            vec![
                array_alias_let(2, "alias", 1),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 2,
                    value: Box::new(int(4)),
                }),
                Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(add(local(3), index_get(1, local(4)))),
                )),
            ],
        ),
        Stmt::Return(Some(local(3))),
    ];

    let ir = compile_ir("packed_f64_loop_local_alias_push.ts", body.clone());
    assert_no_packed_f64_loop(&ir);

    let artifact = compile_artifact_json("artifact_packed_f64_loop_local_alias_push.ts", body);
    assert_no_packed_f64_loop_artifacts(&artifact);
}

#[test]
fn loop_local_array_alias_push_blocks_packed_i32_loop_and_artifacts() {
    let body = vec![
        int32_array_let(1, "arr", vec![1, 2, 3]),
        number_let(3, "sum", true, int(0)),
        for_loop(
            4,
            length(1),
            vec![
                array_alias_let(2, "alias", 1),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 2,
                    value: Box::new(int(4)),
                }),
                Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(bit_or_zero(add(local(3), index_get(1, local(4))))),
                )),
            ],
        ),
        Stmt::Return(Some(local(3))),
    ];

    let ir = compile_ir("packed_i32_loop_local_alias_push.ts", body.clone());
    assert_no_packed_f64_loop(&ir);
    assert_no_packed_i32_loop(&ir);

    let artifact = compile_artifact_json("artifact_packed_i32_loop_local_alias_push.ts", body);
    assert_no_packed_i32_loop_artifacts(&artifact);
}

#[test]
fn loop_local_array_alias_push_blocks_packed_u32_loop_and_artifacts() {
    let body = vec![
        u32_array_let(1, "arr", vec![0, 4_000_000_000]),
        number_let(3, "word", true, ushr_zero(int(0))),
        for_loop(
            4,
            length(1),
            vec![
                array_alias_let(2, "alias", 1),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 2,
                    value: Box::new(int(5)),
                }),
                Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(ushr_zero(index_get(1, local(4)))),
                )),
            ],
        ),
        Stmt::Return(Some(local(3))),
    ];

    let ir = compile_ir("packed_u32_loop_local_alias_push.ts", body.clone());
    assert_no_packed_f64_loop(&ir);
    assert_no_packed_i32_loop(&ir);
    assert_no_packed_u32_loop(&ir);

    let artifact = compile_artifact_json("artifact_packed_u32_loop_local_alias_push.ts", body);
    assert_no_packed_u32_loop_artifacts(&artifact);
}

#[test]
fn inclusive_local_length_bound_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_op_start_and_update(
            3,
            int(0),
            CompareOp::Le,
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_start_and_update(
            3,
            int(-1),
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn body_mutation_of_local_bound_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", true, int(1)),
        buffer_let(2, "buf", local(1)),
        for_loop(
            3,
            local(1),
            vec![
                Stmt::Expr(Expr::LocalSet(1, Box::new(int(16)))),
                buffer_set(2, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("body_mutates_local_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_min_length_bound_fact() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", false, Expr::MathMin(vec![length(1), length(2)])),
        for_loop_with_start_and_update(
            4,
            int(-1),
            local(3),
            Some(increment(4)),
            vec![buffer_set(2, local(4))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_min_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn bitwise_truncated_division_does_not_emit_sdiv_i32() {
    let quotient = bit_or_zero(div(local(1), local(2)));
    let divide_by_zero = bit_or_zero(div(local(1), int(0)));
    let overflow = bit_or_zero(div(int(i32::MIN as i64), int(-1)));
    let body = vec![
        number_let(1, "x", false, int(8)),
        number_let(2, "y", false, int(2)),
        Stmt::Return(Some(add(add(quotient, divide_by_zero), overflow))),
    ];

    let ir = compile_ir("i32_division_regression.ts", body);
    assert!(
        !ir.contains("sdiv i32"),
        "`(a / b) | 0` must not lower to LLVM signed integer division:\n{ir}"
    );
    assert!(
        ir.contains("fdiv double"),
        "`(a / b) | 0` should lower through JS double division:\n{ir}"
    );
    assert!(
        ir.contains("@llvm.fabs.f64"),
        "ToInt32 after division should keep the NaN/Infinity guard:\n{ir}"
    );
}
