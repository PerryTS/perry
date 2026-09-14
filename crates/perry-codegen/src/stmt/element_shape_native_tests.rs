//! The element-shape fast clone's accumulator as an `f64` and its counter as an
//! `i32` (`stmt/element_shape_native.rs`).
//!
//! A child module of `element_shape_fields_random_tests`, so `use super::*`
//! brings that file's `random`/`fields` fixtures, the shape-keyed file's
//! untyped-receiver fixtures and `element_shape_loop_tests`'s slicing helpers.
//!
//! Every assertion here names storage by what the emitted IR itself says it is
//! — the fast preheader's seeds, the deref block's tag-tested reload — rather
//! than by register number, so a renumbering cannot make one vacuous. Each one
//! fails on the pre-change clone, which kept `sum` in its GC root slot (a
//! `ptr addrspace(1)` alloca reloaded through the RS4GC launder every
//! iteration) and the `repeat` counter as a double compared with `fcmp`.

use super::*;

/// `for (let i = 0; i < count; i++) sum += rows[7].id` with an untyped `sum` —
/// the benchmark's `repeat` mode, where both scalars were off-domain.
fn repeat_ir() -> String {
    emit(&untyped_param_module(
        Type::Any,
        Type::Any,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::Integer(7),
            "v",
        ))],
    ))
}

/// The first `store <ty> <value>, ptr <slot>` in `block` whose type is `ty`,
/// as `(value, slot)`.
fn first_store<'a>(block: &'a str, ty: &str) -> (&'a str, &'a str) {
    block
        .lines()
        .find_map(|l| {
            let rest = l.trim_start().strip_prefix(&format!("store {ty} "))?;
            rest.split_once(", ptr ")
        })
        .unwrap_or_else(|| panic!("no `store {ty}` in:\n{block}"))
}

/// The root alloca a laundered reload `reg` was read from
/// (`<reg>.rs4p = load ptr addrspace(1), ptr <slot>`).
fn root_slot_of_reload<'a>(ir: &'a str, reg: &str) -> &'a str {
    let needle = format!("{reg}.rs4p = load ptr addrspace(1), ptr ");
    let at = ir
        .find(&needle)
        .unwrap_or_else(|| panic!("`{reg}` is not a root-slot reload"));
    ir[at + needle.len()..].lines().next().unwrap().trim()
}

/// Is `block`'s terminator an unconditional branch to the block `target`
/// names (`label %<name>`, without its numeric suffix)?
fn terminator_targets(block: &str, target: &str) -> bool {
    block
        .lines()
        .last()
        .and_then(|l| l.trim().strip_prefix(&format!("br {target}.")))
        .is_some_and(|suffix| suffix.chars().all(|c| c.is_ascii_digit()))
}

/// `(accumulator alloca, accumulator root slot, counter i32 slot)` for the
/// repeat clone, read off the fast preheader's two seeds.
fn repeat_storage(ir: &str) -> (String, String, String) {
    let pre = block_slice(ir, "element_shape.loop.fast.preheader");
    let (acc_value, acc_alloca) = first_store(pre, "double");
    let (start, counter_slot) = first_store(pre, "i32");
    assert_eq!(
        start, "0",
        "the counter's i32 slot must be seeded with the loop's literal start; \
         fast preheader:\n{pre}"
    );
    let root = root_slot_of_reload(ir, acc_value);
    (
        acc_alloca.to_string(),
        root.to_string(),
        counter_slot.to_string(),
    )
}

#[test]
fn the_accumulator_never_touches_its_root_slot_inside_the_clone() {
    let ir = repeat_ir();
    assert_shape_keyed_clone(&ir, "repeat with an untyped accumulator");
    let (acc_alloca, acc_root, _) = repeat_storage(&ir);
    let fast = fast_clone_slice(&ir);
    // The seed is the value the deref block tag-tested as a Number — the
    // soundness of every raw `fadd` below rests on that being the same value.
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    let seeded = first_store(
        block_slice(&ir, "element_shape.loop.fast.preheader"),
        "double",
    )
    .0;
    assert!(
        deref.contains(&format!("{seeded} = bitcast i64 {seeded}.rs4o to double"))
            && deref.contains(&format!("bitcast double {seeded} to i64")),
        "the f64 alloca must be seeded with the reload the deref block \
         Number-tested; deref:\n{deref}"
    );
    assert!(
        !fast.contains("addrspace(1)") && !fast.contains("asm \"\""),
        "no root slot may be loaded or stored inside the clone: every reload \
         passes through the RS4GC launder, which pins the loop-carried value to \
         a GPR and costs two GPR<->FPR transfers per iteration; emitted:\n{fast}"
    );
    assert_eq!(
        fast.matches(&format!("load double, ptr {acc_alloca}"))
            .count(),
        1,
        "the accumulator must be read from its f64 alloca once per iteration; \
         emitted:\n{fast}"
    );
    assert_eq!(
        fast.lines()
            .filter(|l| {
                l.trim_start().starts_with("store double ")
                    && l.ends_with(&format!(", ptr {acc_alloca}"))
            })
            .count(),
        1,
        "the accumulator must be committed to its f64 alloca exactly once per \
         iteration; emitted:\n{fast}"
    );
    assert!(
        !fast.contains(&format!("ptr {acc_root}")),
        "the root slot `{acc_root}` must not appear inside the clone; \
         emitted:\n{fast}"
    );
    assert_eq!(
        fast.matches("fadd double").count(),
        1,
        "exactly one IEEE add per iteration — the accumulator's; a second one \
         is the double counter this change retires; emitted:\n{fast}"
    );
}

#[test]
fn the_repeat_counter_is_an_i32_against_the_materialized_bound() {
    let ir = repeat_ir();
    let (_, _, counter_slot) = repeat_storage(&ir);
    let cond = block_slice(&ir, "for.element_shape_fast.cond");
    assert!(
        cond.contains(&format!("load i32, ptr {counter_slot}"))
            && cond.contains("icmp slt i32")
            && !cond.contains("fcmp")
            && !cond.contains("addrspace(1)"),
        "the condition must compare the i32 counter with the preheader's \
         materialized i32 bound — not reload `count` through its root slot and \
         `fcmp` a double counter against it; cond:\n{cond}"
    );
    let update = block_slice(&ir, "for.element_shape_fast.update");
    assert!(
        update.contains(&format!("load i32, ptr {counter_slot}"))
            && update.contains("add i32")
            && update.contains("store i32")
            && !update.contains("store double")
            && !update.contains("fadd"),
        "`i++` must advance ONLY the i32 slot inside the clone; update:\n{update}"
    );
    // The bound the i32 compare trusts is the validated materialization: a
    // fractional, NaN, negative or out-of-range `count` branches to the slow
    // clone before any of this is reachable.
    for label in [
        "element_shape.loop.bound.range",
        "element_shape.loop.bound.convert",
    ] {
        let block = block_slice(&ir, label);
        assert!(
            block.contains("label %element_shape.loop.slow.preheader"),
            "`{label}` must still route an unrepresentable bound to the slow \
             clone; emitted:\n{block}"
        );
    }
}

#[test]
fn every_side_exit_publishes_both_scalars_before_the_slow_clone() {
    let ir = repeat_ir();
    let (acc_alloca, acc_root, counter_slot) = repeat_storage(&ir);
    let fast = fast_clone_slice(&ir);
    assert_eq!(
        fast.matches("label %element_shape.loop.slow.preheader")
            .count(),
        0,
        "no side exit may reach the slow clone without publishing the \
         redirected scalars first; emitted:\n{fast}"
    );
    assert_eq!(
        fast.matches("label %element_shape.loop.side_exit").count(),
        2,
        "the residual check and the Number tag test must both leave through \
         the write-back trampoline; emitted:\n{fast}"
    );
    for (label, successor) in [
        (
            "element_shape.loop.side_exit",
            "label %element_shape.loop.slow.preheader",
        ),
        (
            "element_shape.loop.fast.write_back",
            "label %element_shape.loop.merge",
        ),
    ] {
        let block = block_slice(&ir, label);
        let acc_load = block
            .lines()
            .position(|l| l.contains(&format!("load double, ptr {acc_alloca}")));
        let acc_publish = block.lines().position(|l| {
            l.contains("store ptr addrspace(1)") && l.ends_with(&format!("ptr {acc_root}"))
        });
        let counter_publish = block
            .lines()
            .position(|l| l.contains("store double %") && !l.contains("addrspace"));
        assert!(
            acc_load.is_some()
                && acc_publish.is_some()
                && block.contains(&format!("load i32, ptr {counter_slot}"))
                && block.contains("sitofp i32")
                && counter_publish.is_some()
                && terminator_targets(block, successor)
                && !block.contains("call "),
            "`{label}` must publish the accumulator to its root slot `{acc_root}` \
             and the counter to its double slot, call-free, then branch to \
             `{successor}`; emitted:\n{block}"
        );
    }
    let exit = block_slice(&ir, "for.element_shape_fast.exit");
    assert!(
        exit.contains("label %element_shape.loop.fast.write_back"),
        "the fall-through exit must publish too; emitted:\n{exit}"
    );
}

/// The carried index already had its own commit protocol (#10185). The
/// trampoline must publish the accumulator and the counter but NEVER the
/// carried binding: its real slot has to keep the previous iteration's commit,
/// which is the value the slow clone re-runs the recurrence from.
#[test]
fn the_trampoline_leaves_the_carried_binding_to_its_own_commit() {
    let ir = emit(&access_module(random_body(true)));
    assert_shape_keyed_clone(&ir, "random");
    let fast = fast_clone_slice(&ir);
    let committed = last_block_with_prefix(&fast, "element_shape.number");
    let published = sitofp_results(committed);
    assert_eq!(
        published.len(),
        1,
        "one carried commit; block:\n{committed}"
    );
    let carried_slot = committed
        .lines()
        .find_map(|l| {
            l.trim_start()
                .strip_prefix(&format!("store double {}, ptr ", published[0]))
        })
        .expect("the carried commit stores the converted value");
    let tramp = block_slice(&ir, "element_shape.loop.side_exit");
    assert!(
        !tramp.contains(&format!("ptr {carried_slot}\n"))
            && !tramp.trim_end().ends_with(&format!("ptr {carried_slot}")),
        "the trampoline must not write the carried binding `{carried_slot}`: a \
         side exit happens AFTER this iteration's recurrence, so publishing it \
         would make the slow clone apply the recurrence twice; emitted:\n{tramp}"
    );
    assert_eq!(
        sitofp_results(tramp).len(),
        1,
        "the trampoline converts exactly one i32 — the loop counter; \
         emitted:\n{tramp}"
    );
    let cond = block_slice(&ir, "for.element_shape_fast.cond");
    assert!(
        cond.contains("icmp slt i32") && !cond.contains("fcmp"),
        "the carried form's counter must be an i32 too; cond:\n{cond}"
    );
    assert!(
        !fast.contains("addrspace(1)"),
        "no root slot inside the carried clone; emitted:\n{fast}"
    );
}

/// `fields`: the counter is already a canonical i32 (it indexes), so only the
/// accumulator is redirected — and all three folded adds stay in the double
/// domain.
#[test]
fn the_fields_clone_redirects_only_the_accumulator() {
    let ir = emit(&access_module(fields_body()));
    assert_shape_keyed_clone(&ir, "fields");
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains("addrspace(1)"),
        "no root slot inside the fields clone; emitted:\n{fast}"
    );
    assert_eq!(fast.matches("fadd double").count(), 3);
    let tramp = block_slice(&ir, "element_shape.loop.side_exit");
    assert!(
        tramp.contains("store ptr addrspace(1)")
            && sitofp_results(tramp).is_empty()
            && terminator_targets(tramp, "label %element_shape.loop.slow.preheader"),
        "a canonical-i32 counter has no double storage to publish, so the \
         trampoline carries only the accumulator; emitted:\n{tramp}"
    );
    assert_eq!(
        side_exit_count(&fast),
        fast.matches("label %element_shape.loop.side_exit").count(),
        "every side exit of the fields clone must go through the trampoline; \
         emitted:\n{fast}"
    );
}
