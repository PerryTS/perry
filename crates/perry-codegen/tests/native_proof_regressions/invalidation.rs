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
