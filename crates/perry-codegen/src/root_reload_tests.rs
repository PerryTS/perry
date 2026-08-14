use super::*;
use crate::function::LlFunction;
use crate::types::{DOUBLE, I64, PTR};

/// Render a function the way `to_ir` does, minus the header, so a test can
/// assert on instruction text.
fn body(func: &LlFunction) -> String {
    let mut out = String::new();
    func.for_each_final_line::<std::convert::Infallible>(&mut |line| {
        out.push_str(line);
        out.push('\n');
        Ok(())
    })
    .unwrap_or_else(|e| match e {});
    out
}

/// `%slot` is a bound shadow slot holding a parameter; `mid` runs between
/// the load and the use.
///
/// This is #7280's zod object-spread frame, reduced: load the root, call
/// something, hand the LOADED REGISTER to a consumer that dereferences it.
fn one_block(mid: &str) -> LlFunction {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let v = b.load(DOUBLE, &slot);
    b.call(DOUBLE, mid, &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    f
}

#[test]
fn the_planted_hazard_is_rewritten_to_a_reload() {
    let mut f = one_block("js_object_alloc");
    assert_eq!(apply_to_function(&mut f), 1, "one stale operand to rewrite");
    let ir = body(&f);
    // The consumer must no longer read the pre-call register, and the
    // register it does read must be defined by a load of the same slot
    // immediately above it.
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    let use_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_assign_one"))
        .expect("the consumer survived the pass");
    let reload = lines[use_idx - 1];
    assert!(
        reload.starts_with("%r") && reload.contains("= load double, ptr %r1"),
        "the instruction above the consumer must re-read the slot, got {reload:?}\n{ir}"
    );
    let fresh = reload.split_whitespace().next().unwrap();
    assert!(
        lines[use_idx].contains(&format!("double {fresh},")),
        "the consumer must read the reloaded register, got {:?}",
        lines[use_idx]
    );
    assert!(
        !lines[use_idx].contains("double %r2,"),
        "the consumer must NOT still read the pre-call load"
    );
}

#[test]
fn pathological_instruction_value_product_is_declined() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let stale = b.load(DOUBLE, &slot);

    // 3,001 reloadable values times just over 3,000 instructions exceeds the
    // analysis cap. This is the minified-bundle shape that used to spend many
    // minutes repeatedly walking one enormous generated function.
    for _ in 0..3_000 {
        b.load(DOUBLE, &slot);
    }
    b.call(DOUBLE, "js_object_alloc", &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &stale), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);

    let before = body(&f);
    assert_eq!(apply_to_function(&mut f), 0);
    assert_eq!(body(&f), before, "declining the pass must preserve the IR");
}

#[test]
fn a_non_collecting_window_is_left_byte_for_byte_alone() {
    // The ONLY difference from the planted case is which helper sits in the
    // window. `js_write_barrier` cannot allocate, so nothing can move and
    // the register is still valid — a pass that fires here is firing on the
    // code shape rather than on the hazard, and would put a reload between
    // every pair of instructions in the compiler's output.
    let before = body(&one_block("js_write_barrier"));
    let mut f = one_block("js_write_barrier");
    assert_eq!(apply_to_function(&mut f), 0);
    assert_eq!(body(&f), before);
}

#[test]
fn a_slot_the_program_reassigns_in_the_window_is_not_reloaded() {
    // ★ The soundness half. `f(x, (x = other, 1))` must pass the ORIGINAL
    // `x`; re-reading the slot below the assignment would hand the consumer
    // the new value, which is a miscompile and not a rooting fix. See
    // `rooting::operand_is_reloadable`.
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let v = b.load(DOUBLE, &slot);
    let other = b.call(DOUBLE, "js_object_alloc", &[]);
    b.store(DOUBLE, &other, &slot); // the reassignment
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    assert_eq!(
        apply_to_function(&mut f),
        0,
        "a slot the program itself stores to in the window must be left alone"
    );
}

#[test]
fn the_window_is_a_cfg_path_not_a_line_range() {
    // Load in the entry block, collection point in one arm, use in the
    // merge. A line-order scan sees the same thing a path-based one does
    // here; the point of the test is that the merge block IS considered at
    // all, since the dominant population in both corpora is cross-block.
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let entry = f.create_block("entry").label.clone();
    let then = f.create_block("then").label.clone();
    let merge = f.create_block("merge").label.clone();
    let _ = entry;
    let slot;
    let v;
    {
        let b = f.block_mut(0).unwrap();
        slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        v = b.load(DOUBLE, &slot);
        b.cond_br("%c", &then, &merge);
    }
    {
        let b = f.block_mut(1).unwrap();
        b.call(DOUBLE, "js_object_alloc", &[]);
        b.br(&merge);
    }
    {
        let b = f.block_mut(2).unwrap();
        let r = b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, &r);
    }
    assert_eq!(apply_to_function(&mut f), 1);
    let ir = body(&f);
    assert!(
        ir.matches("= load double, ptr %r1").count() == 2,
        "the merge block must re-read the slot:\n{ir}"
    );
}

#[test]
fn a_back_edge_round_trip_is_not_an_intra_iteration_path() {
    // The load is INSIDE the loop, so every iteration re-executes it and the
    // register is fresh when the use runs. Counting the back edge would make
    // the pass reload on every loop body in the program.
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    f.create_block("entry");
    let looplbl = f.create_block("loop").label.clone();
    let done = f.create_block("done").label.clone();
    let slot;
    {
        let b = f.block_mut(0).unwrap();
        slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        b.br(&looplbl);
    }
    {
        let b = f.block_mut(1).unwrap();
        let v = b.load(DOUBLE, &slot);
        b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.call(DOUBLE, "js_object_alloc", &[]); // collects, but AFTER the use
        b.cond_br("%c", &looplbl, &done);
    }
    {
        let b = f.block_mut(2).unwrap();
        b.ret(DOUBLE, "0.0");
    }
    assert_eq!(apply_to_function(&mut f), 0);
}

/// ★ #7305 turned every throwing call into an `invoke` with TWO successors,
/// and perry emits the continuation label INLINE in the same builder block.
/// Both halves have to be modelled:
///
/// * the call half — an `invoke` of a collecting helper opens a window, so
///   a use below it must reload. Missing this drops every reload inside a
///   `try`, silently.
/// * the terminator half — the unwind edge is a real path, so a use in the
///   landing pad is reached from the load and must reload too.
///
/// The unwind edge is also why the rule is "reload at the USE" rather than
/// "reload after the call": a load from the slot reads whatever the
/// collector last wrote and is valid wherever it sits, so it is correct on
/// the unwind edge and the normal edge alike. There is no "after the call"
/// position that would have to be chosen correctly for two successors.
#[test]
fn an_invoke_opens_a_window_on_both_of_its_edges() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    f.create_block("entry");
    let cont = f.create_block("cont").label.clone();
    let lpad = f.create_block("lpad").label.clone();
    let slot;
    let v;
    {
        let b = f.block_mut(0).unwrap();
        slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        v = b.load(DOUBLE, &slot);
        b.emit_raw(format!(
            "invoke double @js_object_alloc(i32 4) to label %{cont} unwind label %{lpad}"
        ));
    }
    {
        let b = f.block_mut(1).unwrap();
        b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, "0.0");
    }
    {
        let b = f.block_mut(2).unwrap();
        b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "1.0")],
        );
        b.ret(DOUBLE, "0.0");
    }
    assert_eq!(
        apply_to_function(&mut f),
        2,
        "both the normal and the unwind successor use the stale register"
    );
    let ir = body(&f);
    assert_eq!(
        ir.matches("= load double, ptr %r1").count(),
        3,
        "the original load plus one reload on each edge:\n{ir}"
    );
}

#[test]
fn a_function_with_no_bound_slot_is_untouched() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    let v = b.load(DOUBLE, &slot);
    b.call(DOUBLE, "js_object_alloc", &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    assert_eq!(apply_to_function(&mut f), 0);
}

#[test]
fn the_bind_is_recorded_through_the_inline_form_too() {
    // #7088's inline slot store emits the SAME `js_shadow_slot_bind` call on
    // its slow arm, which is why the recording hook lives in `call_void`
    // rather than at the thirteen sites that build a bind. If a future bind
    // form stops going through `call_void`, this is the assertion that has
    // to be updated with it.
    let mut f = LlFunction::new("t", DOUBLE, vec![]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "3"), (PTR, &slot)],
    );
    b.ret(DOUBLE, "0.0");
    assert!(f
        .reg_counter()
        .shadow_slot_allocas()
        .contains(slot.as_str()));
}

#[test]
fn a_raw_operand_is_renamed_by_token_not_by_substring() {
    // `%r1` must not rewrite inside `%r10`. This is the reason the Raw arm
    // does not use `str::replace`.
    let line = "  %r99 = fadd double %r1, %r10";
    assert_eq!(
        rename_in_text(line, "r1", "r42"),
        "  %r99 = fadd double %r42, %r10"
    );
    // And the LHS is never a use.
    assert_eq!(
        rename_in_text("  %r1 = fadd double %r2, %r3", "r1", "r42"),
        "  %r1 = fadd double %r2, %r3"
    );
}

/// ★ The regression that cost the acceptance arm 30/30 -> 0/30.
///
/// `entry_post_init_setup` is spliced into block 0 at `entry_init_boundary`,
/// and for a function built by `enable_post_init_shadow_frame` that region
/// contains the `js_shadow_frame_enter` call itself. Bumping the boundary by
/// EVERY insertion into block 0 — rather than only the ones at or above it —
/// pushes the index past the block, `to_ir` clamps it with
/// `.min(instruction_count())`, and the frame push lands after every
/// `js_shadow_slot_bind` in the body. Nothing is rooted, and the symptom is
/// `TypeError: value is not a function` — the bug this pass fixes, wearing
/// its own fix as a disguise.
#[test]
fn an_insertion_below_the_post_init_splice_does_not_move_it() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    {
        let b = f.create_block("entry");
        // The "init prelude": everything before mark_entry_init_boundary.
        b.call_void("js_gc_init", &[]);
    }
    f.mark_entry_init_boundary();
    let boundary_before = f.entry_init_boundary();
    assert_eq!(boundary_before, Some(1));
    {
        let b = f.block_mut(0).unwrap();
        let slot = b.alloca(DOUBLE);
        b.store(DOUBLE, "%arg", &slot);
        b.call_void(
            "js_shadow_slot_bind",
            &[(crate::types::I32, "0"), (PTR, &slot)],
        );
        let v = b.load(DOUBLE, &slot);
        b.call(DOUBLE, "js_object_alloc", &[]);
        let r = b.call(
            DOUBLE,
            "js_object_assign_one",
            &[(DOUBLE, &v), (DOUBLE, "0.0")],
        );
        b.ret(DOUBLE, &r);
    }
    let n_insts = f.blocks()[0].instruction_count();
    assert_eq!(apply_to_function(&mut f), 1);
    assert_eq!(
        f.entry_init_boundary(),
        boundary_before,
        "the reload went in BELOW the splice, so the splice must not move"
    );
    assert!(
        f.entry_init_boundary().unwrap() <= f.blocks()[0].instruction_count(),
        "a boundary past the block gets clamped to the END by to_ir, which \
             relocates the whole post-init region including the frame push"
    );
    assert_eq!(f.blocks()[0].instruction_count(), n_insts + 1);
}

#[test]
fn an_i64_slot_reloads_at_its_own_width() {
    let mut f = LlFunction::new("t", DOUBLE, vec![]);
    let b = f.create_block("entry");
    let slot = b.alloca(I64);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let v = b.load(I64, &slot);
    b.call(DOUBLE, "js_object_alloc", &[]);
    b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(I64, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, "0.0");
    assert_eq!(apply_to_function(&mut f), 1);
    assert!(body(&f).contains("= load i64, ptr %r1"), "{}", body(&f));
}

/// A fixture where ONE instruction reads two registers loaded from two
/// different shadow slots — `js_object_assign_one(receiver, value)`, which
/// `index_set.rs` lowers `object`-before-`value`, so both really can be
/// slot loads.
fn two_slots_one_consumer() -> LlFunction {
    let mut f = LlFunction::new(
        "t",
        DOUBLE,
        vec![(DOUBLE, "%obj".into()), (DOUBLE, "%val".into())],
    );
    let b = f.create_block("entry");
    let s0 = b.alloca(DOUBLE);
    let s1 = b.alloca(DOUBLE);
    b.store(DOUBLE, "%obj", &s0);
    b.store(DOUBLE, "%val", &s1);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &s0)],
    );
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "1"), (PTR, &s1)],
    );
    let a = b.load(DOUBLE, &s0);
    let c = b.load(DOUBLE, &s1);
    b.call(DOUBLE, "js_object_alloc", &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &a), (DOUBLE, &c)],
    );
    b.ret(DOUBLE, &r);
    f
}

/// #7311 follow-up: BOTH stale operands of one instruction must be
/// reloaded. The original apply loop renamed-then-inserted per rewrite, so
/// the first insert shifted the consumer down and the second rename
/// addressed the freshly-inserted reload instead — leaving one operand
/// stale (the exact defect this pass exists to close) and emitting a load
/// nothing consumes.
#[test]
fn both_stale_operands_of_one_instruction_are_reloaded() {
    let mut f = two_slots_one_consumer();
    assert_eq!(
        apply_to_function(&mut f),
        2,
        "two stale operands on one instruction, not one"
    );
    let ir = body(&f);
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    let use_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_assign_one"))
        .expect("the consumer survived");
    let consumer = lines[use_idx];

    // The two instructions above the consumer must both be slot reloads,
    // and the consumer must read BOTH of their registers.
    let r1 = lines[use_idx - 1];
    let r2 = lines[use_idx - 2];
    for r in [r1, r2] {
        assert!(
            r.contains("= load double, ptr %r"),
            "expected a slot reload above the consumer, got {r:?}\n{ir}"
        );
    }
    for r in [r1, r2] {
        let fresh = r.split_whitespace().next().unwrap();
        assert!(
            consumer.contains(fresh),
            "reload {fresh} is dead — the consumer does not read it: {consumer:?}\n{ir}"
        );
    }
    // And neither PRE-call load may survive in the consumer. Derive those
    // registers rather than hard-coding them: they are the loads that sit
    // above the collecting call, not the reloads inserted below it.
    let call_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_alloc"))
        .expect("the collecting call survived");
    for l in &lines[..call_idx] {
        if let Some(dst) = l.split_whitespace().next() {
            if l.contains("= load double, ptr %r") {
                assert!(
                    !consumer.contains(&format!("double {dst},"))
                        && !consumer.contains(&format!("double {dst})")),
                    "stale operand {dst} survived in {consumer:?}\n{ir}"
                );
            }
        }
    }
}

/// The handle global for a string literal, held across a collecting call.
///
/// `%slot` is bound but unused: the point is that the value at risk lives
/// in a GLOBAL the collector rewrites, which is the half of #7664 that was
/// invisible because the pass keyed only on allocas.
fn one_block_global(root: &str, mid: &str) -> LlFunction {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let v = b.load(DOUBLE, root);
    b.call(DOUBLE, mid, &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    f
}

#[test]
fn a_string_handle_global_held_across_a_call_is_reloaded() {
    let mut f = one_block_global("@m_.str.5.handle", "js_object_alloc");
    assert_eq!(apply_to_function(&mut f), 1, "one stale operand to rewrite");
    let ir = body(&f);
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    let use_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_assign_one"))
        .expect("the consumer survived the pass");
    let reload = lines[use_idx - 1];
    assert!(
        reload.contains("= load double, ptr @m_.str.5.handle"),
        "the instruction above the consumer must re-read the handle global, \
             got {reload:?}\n{ir}"
    );
    let fresh = reload.split_whitespace().next().unwrap();
    assert!(
        lines[use_idx].contains(&format!("double {fresh},")),
        "the consumer must read the reloaded register, got {:?}",
        lines[use_idx]
    );
}

/// ★ The narrowness is the point, so it is asserted rather than argued.
///
/// `@perry_global_*` is a module-level variable the PROGRAM assigns, so a
/// re-read can observe a later assignment instead of the value the call was
/// given — `operand_needs_root` says so, and re-deriving it would be a
/// miscompile, not a rooting fix. Those two hits stay open (#7664) rather
/// than being closed by widening `is_string_handle_global`, and this test
/// is what makes widening it a test failure instead of a silent decision.
#[test]
fn a_module_global_is_not_a_reload_source() {
    let mut f = one_block_global("@perry_global_m__14", "js_object_alloc");
    let before = body(&f);
    assert_eq!(
        apply_to_function(&mut f),
        0,
        "a mutable module global must not be re-read"
    );
    assert_eq!(body(&f), before);
    assert!(!is_string_handle_global("perry_global_m__14"));
    assert!(!is_string_handle_global("m_.str.x.handle"));
    assert!(is_string_handle_global("m_.str.5.handle"));
}

/// `__perry_init_strings_*` is the one function that writes a handle
/// global, and `js_string_from_bytes` above the store allocates. The store
/// side-condition — the same one that protects a reassigned slot — is what
/// excludes it, so it is checked rather than assumed.
#[test]
fn a_store_to_the_handle_global_in_the_window_suppresses_the_reload() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let v = b.load(DOUBLE, "@m_.str.5.handle");
    let fresh = b.call(DOUBLE, "js_object_alloc", &[]);
    b.store(DOUBLE, &fresh, "@m_.str.5.handle");
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    let before = body(&f);
    assert_eq!(apply_to_function(&mut f), 0);
    assert_eq!(body(&f), before);
}

/// #7664 shape 2, and the reason `Counter__increment` took ZERO reloads
/// before: the register that crosses the call is the MASK, not the load.
///
/// `this.count++` reduced: load the receiver out of its slot, unmask it,
/// run the property GET (which can run a user getter), then hand the same
/// unmasked register to the SET.
fn masked_receiver(mid: &str) -> LlFunction {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let boxed = b.load(DOUBLE, &slot);
    let bits = b.bitcast_double_to_i64(&boxed);
    let raw = b.and(I64, &bits, "281474976710655");
    b.call(DOUBLE, mid, &[]);
    b.call_void(
        "js_object_set_field_by_name",
        &[(I64, &raw), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, "0.0");
    f
}

#[test]
fn the_masked_receiver_is_re_derived_not_just_the_load() {
    let mut f = masked_receiver("js_object_get_field_by_name_f64");
    assert_eq!(
        apply_to_function(&mut f),
        1,
        "the mask is one stale operand"
    );
    let ir = body(&f);
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    let use_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_set_field_by_name"))
        .expect("the consumer survived the pass");
    // The WHOLE derivation is re-emitted, in order, immediately above the
    // consumer: re-reading the slot alone would hand the sink a double
    // where it wants the masked i64.
    let recipe = &lines[use_idx - 3..use_idx];
    assert!(
        recipe[0].contains("= load double, ptr %r1")
            && recipe[1].contains("= bitcast double %r")
            && recipe[2].contains("= and i64 %r"),
        "expected load/bitcast/and above the consumer, got {recipe:?}\n{ir}"
    );
    let fresh = recipe[2].split_whitespace().next().unwrap();
    assert!(
        lines[use_idx].contains(&format!("i64 {fresh},")),
        "the consumer must read the re-derived mask, got {:?}\n{ir}",
        lines[use_idx]
    );
    assert!(
        !lines[use_idx].contains("i64 %r4,"),
        "the consumer must NOT still read the pre-call mask\n{ir}"
    );
}

/// The same frame with a NON-collecting helper in the window emits exactly
/// the IR it emitted before. The derivation closure must not turn every
/// mask in the program into three extra instructions.
#[test]
fn a_masked_receiver_with_no_collection_point_is_left_alone() {
    let mut f = masked_receiver("js_is_truthy");
    let before = body(&f);
    assert_eq!(apply_to_function(&mut f), 0);
    assert_eq!(body(&f), before);
}

/// A derivation is only extended through PURE ops. A call in the middle of
/// the chain ends it — its result is not a function of the root that can be
/// re-executed, and re-running it would be a second call.
#[test]
fn a_derivation_is_not_extended_through_a_call() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let boxed = b.load(DOUBLE, &slot);
    // NON-collecting, so it opens no window of its own; the question is
    // purely whether its RESULT joins the derivation.
    let derived = b.call(DOUBLE, "js_nanbox_get_pointer", &[(DOUBLE, &boxed)]);
    b.call(DOUBLE, "js_object_alloc", &[]);
    b.call_void(
        "js_object_set_field_by_name",
        &[(DOUBLE, &derived), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, "0.0");
    let before = body(&f);
    assert_eq!(
        apply_to_function(&mut f),
        0,
        "a call's result is not a re-materialisable derivation"
    );
    assert_eq!(body(&f), before);
}

/// ★ The regression this pass shipped and an A/B caught, not the checker.
///
/// A derived value whose DEFINITION sits below a store to the root is still
/// governed by the window since the ROOT LOAD, not the window since itself.
/// `main`'s class-object read has exactly this shape — the scope-end slot
/// clear lands between the load and the mask:
///
/// ```llvm
///   %a = load double, ptr %slot
///   %b = or i64 %a, POINTER_TAG
///   store double 0.0, ptr %slot     ; <- the clear
///   %c = and i64 %b, MASK           ; derived, DEFINED BELOW the clear
///   call collect()
///   call sink(%c)
/// ```
///
/// Anchoring `%c`'s window at `%c` never sees the clear, re-materialises
/// `load %slot` at the sink, and reads a slot the program had just nulled —
/// which turned `(makeAnon(77) as any).v` into `undefined`.
#[test]
fn a_derivation_defined_below_a_store_to_its_root_is_not_re_materialised() {
    let mut f = LlFunction::new("t", DOUBLE, vec![(DOUBLE, "%arg".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(DOUBLE);
    b.store(DOUBLE, "%arg", &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let boxed = b.load(DOUBLE, &slot);
    let bits = b.bitcast_double_to_i64(&boxed);
    b.store(DOUBLE, "0.0", &slot);
    let raw = b.and(I64, &bits, "281474976710655");
    b.call(DOUBLE, "js_object_alloc", &[]);
    b.call_void(
        "js_object_set_field_by_name",
        &[(I64, &raw), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, "0.0");
    let before = body(&f);
    assert_eq!(
        apply_to_function(&mut f),
        0,
        "the root was stored to below the load, so nothing may be re-read"
    );
    assert_eq!(body(&f), before);
}

/// #7725: `js_closure_get_capture_bits(ptr, idx)` reads out of the closure's own capture
/// array — a location the collector rewrites when it relocates the closure — so a chain
/// [shadow-slot load] -> [mask] -> [capture-bits call] -> [bitcast to double] is a
/// reloadable derivation exactly like the masked-receiver shape above, just with a CALL as
/// one of its steps instead of only bit ops.
///
/// `%slot` stands in for `current_closure_slot` (an `i64` shadow slot holding the tagged
/// closure pointer, #7055); the `and` is `try_current_closure_ptr_value`'s mask; `idx` is
/// always a literal in real emissions (`literals_vars.rs` et al. format a `u32` directly).
fn capture_get_chain(idx: &str, mid: &str) -> LlFunction {
    let mut f = LlFunction::new("t", DOUBLE, vec![(I64, "%this_closure".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(I64);
    let tagged = b.or(
        I64,
        "%this_closure",
        "281474976710654", /* POINTER_TAG */
    );
    b.store(I64, &tagged, &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let bits = b.load(I64, &slot);
    let ptr = b.and(I64, &bits, "281474976710655" /* POINTER_MASK */);
    let cap_bits = b.call(
        I64,
        "js_closure_get_capture_bits",
        &[(I64, &ptr), (crate::types::I32, idx)],
    );
    let v = b.bitcast_i64_to_double(&cap_bits);
    b.call(DOUBLE, mid, &[]);
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    f
}

#[test]
fn a_capture_get_call_held_across_a_collecting_call_is_reloaded() {
    let mut f = capture_get_chain("3", "js_object_alloc");
    assert_eq!(apply_to_function(&mut f), 1, "one stale operand to rewrite");
    let ir = body(&f);
    let lines: Vec<&str> = ir.lines().map(str::trim).collect();
    let use_idx = lines
        .iter()
        .position(|l| l.contains("@js_object_assign_one"))
        .expect("the consumer survived the pass");
    // The whole chain re-materialises immediately above the consumer: a fresh slot load, a
    // fresh mask, a FRESH CALL to js_closure_get_capture_bits with the SAME index, and a
    // fresh bitcast — not just the load, and not a re-lowering that skips the call.
    let recipe = &lines[use_idx - 4..use_idx];
    assert!(
        recipe[0].contains("= load i64, ptr %r1")
            && recipe[1].contains("= and i64 %r")
            && recipe[2].contains("= call i64 @js_closure_get_capture_bits(i64 %r")
            && recipe[2].contains(", i32 3)")
            && recipe[3].contains("= bitcast i64 %r"),
        "expected load/and/call/bitcast above the consumer, got {recipe:?}\n{ir}"
    );
    let fresh = recipe[3].split_whitespace().next().unwrap();
    assert!(
        lines[use_idx].contains(&format!("double {fresh},")),
        "the consumer must read the re-derived capture read, got {:?}\n{ir}",
        lines[use_idx]
    );
    // And the ORIGINAL (pre-call) bitcast must not survive as the consumer's operand.
    let orig_bitcast = lines
        .iter()
        .position(|l| l.contains("= bitcast i64 %r") && !recipe.contains(l))
        .map(|i| lines[i].split_whitespace().next().unwrap());
    if let Some(stale) = orig_bitcast {
        assert!(
            !lines[use_idx].contains(&format!("double {stale},")),
            "the consumer must NOT still read the pre-call capture read"
        );
    }
}

#[test]
fn a_capture_get_call_with_no_collection_point_is_left_alone() {
    let before = body(&capture_get_chain("3", "js_write_barrier"));
    let mut f = capture_get_chain("3", "js_write_barrier");
    assert_eq!(apply_to_function(&mut f), 0);
    assert_eq!(body(&f), before);
}

/// [`capture_get_chain`] plus a `js_closure_set_capture_bits(set_idx, …)` inserted between
/// the collecting call and the consumer — still inside the window the get call opened.
///
/// The set's own ptr operand is the bare `%this_closure` parameter, deliberately NOT the
/// reloaded/masked `ptr` register the get call uses: real codegen re-derives it fresh
/// (`current_closure_ptr_value` is called again at every capture-bits site), and reusing the
/// same SSA register here would make it a member of the closure-ptr sub-group's OWN
/// derivation, which would itself get "fixed" by this pass — an real but unrelated rewrite
/// that has nothing to do with what this fixture is testing.
fn capture_get_chain_with_set(get_idx: &str, set_idx: &str) -> LlFunction {
    let mut f = LlFunction::new("t", DOUBLE, vec![(I64, "%this_closure".into())]);
    let b = f.create_block("entry");
    let slot = b.alloca(I64);
    let tagged = b.or(
        I64,
        "%this_closure",
        "281474976710654", /* POINTER_TAG */
    );
    b.store(I64, &tagged, &slot);
    b.call_void(
        "js_shadow_slot_bind",
        &[(crate::types::I32, "0"), (PTR, &slot)],
    );
    let bits = b.load(I64, &slot);
    let ptr = b.and(I64, &bits, "281474976710655" /* POINTER_MASK */);
    let cap_bits = b.call(
        I64,
        "js_closure_get_capture_bits",
        &[(I64, &ptr), (crate::types::I32, get_idx)],
    );
    let v = b.bitcast_i64_to_double(&cap_bits);
    b.call(DOUBLE, "js_object_alloc", &[]);
    b.call_void(
        "js_closure_set_capture_bits",
        &[
            (I64, "%this_closure"),
            (crate::types::I32, set_idx),
            (I64, "0"),
        ],
    );
    let r = b.call(
        DOUBLE,
        "js_object_assign_one",
        &[(DOUBLE, &v), (DOUBLE, "0.0")],
    );
    b.ret(DOUBLE, &r);
    f
}

/// ★ The store side-condition, capture-bits' own shape: `js_closure_set_capture_bits` to the
/// SAME index in the window must suppress the reload — re-calling `js_closure_get_capture_bits`
/// below the set would observe the NEW value instead of the one live at the read, which is a
/// miscompile and not a rooting fix (identical reasoning to
/// `a_slot_the_program_reassigns_in_the_window_is_not_reloaded` above).
#[test]
fn a_capture_set_to_the_same_index_in_the_window_suppresses_the_reload() {
    let mut f = capture_get_chain_with_set("3", "3");
    let before = body(&f);
    assert_eq!(
        apply_to_function(&mut f),
        0,
        "a set to the SAME capture index in the window must suppress the reload"
    );
    assert_eq!(body(&f), before);
}

/// The per-index keying is the point: a set to a DIFFERENT index must NOT suppress a read of
/// this one — a blanket "any capture set invalidates every capture get" would be sound but
/// needlessly wide, and this is the test that would catch a keying regression to that.
#[test]
fn a_capture_set_to_a_different_index_does_not_suppress_the_reload() {
    let mut f = capture_get_chain_with_set("3", "7");
    assert_eq!(
        apply_to_function(&mut f),
        1,
        "a set to a DIFFERENT capture index must not suppress this reload"
    );
}
