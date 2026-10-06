// Regression witnesses for the LLVM basic-block builder.
#[test]
fn byte_owner_lifetime_uses_follow_only_actual_calls() {
    use crate::types::{DOUBLE, I32, PTR};
    let counter = std::rc::Rc::new(super::RegCounter::new());
    let mut block = super::LlBlock::new("entry", counter.clone());
    block.retain_byte_owner_root_slot("%owner");
    block.retain_byte_owner_root_slot("%owner");
    block.add(I32, "1", "2");
    block.call_void("js_shadow_slot_set", &[]);
    block.call(PTR, "js_native_buffer_data_ptr", &[]);
    assert!(!block
        .insts()
        .iter()
        .any(|i| i.scan_str().contains("asm sideeffect")));
    block.call(DOUBLE, "collecting_callee", &[]);
    block.call_void("collecting_void_callee", &[]);
    let mut continuation = super::LlBlock::new("next", counter);
    continuation.call_indirect(DOUBLE, "%callback", &[(PTR, "null")]);
    let uses = block
        .insts()
        .iter()
        .chain(continuation.insts())
        .filter_map(|i| {
            let line = i.scan_str();
            line.contains("asm sideeffect").then_some(line)
        })
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 3);
    for line in uses {
        assert!(line.contains("\"r\""), "one exact root operand: {line}");
        assert!(
            line.contains("readnone"),
            "a lifetime use does not clobber memory"
        );
    }
}

#[test]
fn dropping_byte_owner_call_edge_turns_the_invariant_red() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "block::tests::byte_owner_lifetime_uses_follow_only_actual_calls",
            "--nocapture",
        ])
        .env("PERRY_B4_SABOTAGE", "owner_call_edge")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
    assert!(!child.status.success());
}
use super::*;
use crate::types::{DOUBLE, I32, I64, PTR};
use std::thread;

fn fresh() -> LlBlock {
    LlBlock::new("entry.0", Rc::new(RegCounter::new()))
}

fn fresh_with(fast_math: bool, fp_contract_mode: FpContractMode) -> LlBlock {
    LlBlock::new_with_fp_flags(
        "entry.0",
        Rc::new(RegCounter::new()),
        FpFlags::new(fast_math, fp_contract_mode),
    )
}

#[test]
fn nanbox_bitcast_roundtrip_folds_to_source() {
    // #5334 lever C: i64 -> double -> i64 collapses to the original i64,
    // and the reverse collapses to the original double. Only the first
    // bitcast of each pair is emitted.
    let mut b = fresh();
    let dbl = b.bitcast_i64_to_double("%arg"); // %r1 = bitcast i64 %arg to double
    let back = b.bitcast_double_to_i64(&dbl); // folds -> %arg (no new instr)
    assert_eq!(back, "%arg");

    let unboxed = b.bitcast_double_to_i64("%v"); // %r2 = bitcast double %v to i64
    let reboxed = b.bitcast_i64_to_double(&unboxed); // folds -> %v
    assert_eq!(reboxed, "%v");

    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = bitcast i64 %arg to double\n  %r2 = bitcast double %v to i64"
    );
}

#[test]
fn nanbox_bitcast_non_roundtrip_still_emits() {
    // A lone unbox with no inverse is untouched, and a fresh unbox of a
    // different value is not mistakenly folded.
    let mut b = fresh();
    let a = b.bitcast_double_to_i64("%a"); // %r1
    let c = b.bitcast_double_to_i64("%c"); // %r2 (different source, no fold)
    assert_eq!(a, "%r1");
    assert_eq!(c, "%r2");
    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = bitcast double %a to i64\n  %r2 = bitcast double %c to i64"
    );
}

#[test]
fn fadd_emits_expected_ir_default() {
    // Default mode: no fast-math FMF flags emitted, bit-exact with
    // Node.
    let mut b = fresh();
    let r = b.fadd("1.0", "2.0");
    assert_eq!(r, "%r1");
    assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd double 1.0, 2.0");
}

#[test]
fn fadd_emits_contract_when_fp_contract_on() {
    let mut b = fresh_with(false, FpContractMode::On);
    let r = b.fadd("1.0", "2.0");
    assert_eq!(r, "%r1");
    assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd contract double 1.0, 2.0");
}

#[test]
fn fadd_emits_reassoc_when_fast_math_without_contract() {
    let mut b = fresh_with(true, FpContractMode::Off);
    let r = b.fadd("1.0", "2.0");
    assert_eq!(r, "%r1");
    assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd reassoc double 1.0, 2.0");
}

#[test]
fn fadd_emits_reassoc_and_contract_when_both_enabled() {
    let mut b = fresh_with(true, FpContractMode::Fast);
    let r = b.fadd("1.0", "2.0");
    assert_eq!(r, "%r1");
    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = fadd reassoc contract double 1.0, 2.0"
    );
}

#[test]
fn fp_flags_do_not_bleed_between_parallel_blocks() {
    let strict = thread::spawn(|| {
        let mut b = fresh_with(false, FpContractMode::Off);
        b.fmul("1.0", "2.0");
        b.to_ir()
    });
    let relaxed = thread::spawn(|| {
        let mut b = fresh_with(true, FpContractMode::On);
        b.fmul("1.0", "2.0");
        b.to_ir()
    });
    assert_eq!(
        strict.join().unwrap(),
        "entry.0:\n  %r1 = fmul double 1.0, 2.0"
    );
    assert_eq!(
        relaxed.join().unwrap(),
        "entry.0:\n  %r1 = fmul reassoc contract double 1.0, 2.0"
    );
}

#[test]
fn call_with_args() {
    let mut b = fresh();
    let r = b.call(DOUBLE, "js_nanbox_string", &[(I64, "%handle")]);
    assert_eq!(r, "%r1");
    assert!(b
        .to_ir()
        .contains("call double @js_nanbox_string(i64 %handle)"));
}

#[test]
fn active_stable_packed_proofs_are_dirtied_only_by_executed_non_intrinsic_calls() {
    let mut b = fresh();
    b.counter
        .push_stable_packed_revalidation_slot("%proof_dirty".to_string());
    b.call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, "%value")]);
    b.call_void("js_shadow_slot_bind", &[(I64, "0"), (PTR, "%root")]);
    b.call_void("js_write_barrier_root_nanbox", &[(I64, "%bits")]);
    b.call(DOUBLE, "js_dyn_index_get", &[(DOUBLE, "%object")]);
    b.call_indirect(DOUBLE, "%callback", &[(DOUBLE, "%value")]);
    b.counter
        .pop_stable_packed_revalidation_slot("%proof_dirty");
    b.call(DOUBLE, "js_dyn_index_get", &[(DOUBLE, "%object")]);

    let ir = b.to_ir();
    assert_eq!(
        ir.matches("store i1 1, ptr %proof_dirty").count(),
        2,
        "{ir}"
    );
    assert!(
        ir.find("call double @llvm.fabs.f64") < ir.find("store i1 1, ptr %proof_dirty"),
        "proof-preserving calls must not dirty the proof: {ir}"
    );
    let first_dirty = ir.find("store i1 1, ptr %proof_dirty").unwrap();
    assert!(
        ir.find("@js_shadow_slot_bind").unwrap() < first_dirty
            && ir.find("@js_write_barrier_root_nanbox").unwrap() < first_dirty,
        "GC bookkeeping must preserve the proof: {ir}"
    );
}

#[test]
fn direct_gc_leaf_call_places_the_callsite_attribute_after_arguments() {
    let mut b = fresh();
    let r = b.call_gc_leaf(DOUBLE, "guarded_reader", &[(I64, "%handle")]);
    assert_eq!(r, "%r1");
    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = call double @guarded_reader(i64 %handle) \"gc-leaf-function\""
    );
}

#[test]
fn indirect_call_uses_opaque_pointer_syntax() {
    let mut b = fresh();
    let r = b.call_indirect(DOUBLE, "%callback", &[(I64, "%closure"), (DOUBLE, "%arg")]);
    assert_eq!(r, "%r1");
    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = call double %callback(i64 %closure, double %arg)"
    );
}

#[test]
fn indirect_gc_leaf_call_places_the_callsite_attribute_after_arguments() {
    let mut b = fresh();
    let r = b.call_indirect_gc_leaf(DOUBLE, "%callback", &[(I64, "%closure"), (DOUBLE, "%arg")]);
    assert_eq!(r, "%r1");
    assert_eq!(
        b.to_ir(),
        "entry.0:\n  %r1 = call double %callback(i64 %closure, double %arg) \"gc-leaf-function\""
    );
}

#[test]
fn indirect_invoke_uses_opaque_pointer_syntax() {
    let mut b = fresh();
    b.counter.push_eh_scope("catch.0".to_string());
    let r = b.call_indirect(DOUBLE, "%callback", &[(I64, "%closure"), (DOUBLE, "%arg")]);
    assert_eq!(r, "%r1");
    assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = invoke double %callback(i64 %closure, double %arg) to label %eh.cont2 unwind label %catch.0\neh.cont2:"
        );
}

#[test]
fn indirect_gc_leaf_invoke_places_the_attribute_before_the_successor() {
    let mut b = fresh();
    b.counter.push_eh_scope("catch.0".to_string());
    let r = b.call_indirect_gc_leaf(DOUBLE, "%callback", &[(I64, "%closure"), (DOUBLE, "%arg")]);
    assert_eq!(r, "%r1");
    assert_eq!(
            b.to_ir(),
            "entry.0:\n  %r1 = invoke double %callback(i64 %closure, double %arg) \"gc-leaf-function\" to label %eh.cont2 unwind label %catch.0\neh.cont2:"
        );
}

#[test]
fn cold_property_miss_keeps_the_active_exception_edge() {
    let mut b = fresh();
    b.counter.push_eh_scope("catch.0".to_string());
    b.call(
        DOUBLE,
        "js_object_get_field_ic_miss_packed",
        &[
            (I64, "%receiver"),
            (I64, "%key"),
            (PTR, "%cache"),
            (PTR, "%packed"),
        ],
    );
    let ir = b.to_ir();
    assert!(
        ir.contains("invoke double @js_object_get_field_ic_miss_packed("),
        "{ir}"
    );
    assert!(ir.contains("unwind label %catch.0"), "{ir}");
    b.call(I32, "js_string_compare", &[(I64, "%a"), (I64, "%b")]);
    assert!(b.to_ir().contains("call i32 @js_string_compare("));
}

#[test]
fn terminator_blocks_further_emits() {
    let mut b = fresh();
    b.ret(DOUBLE, "0.0");
    // This would silently drop; we don't want extra lines after ret.
    let _ = b.fadd("1.0", "2.0");
    let ir = b.to_ir();
    assert!(ir.contains("ret double 0.0"));
    assert!(!ir.contains("fadd"));
}

#[test]
fn regs_are_function_unique_not_block_unique() {
    let counter = Rc::new(RegCounter::new());
    let mut b1 = LlBlock::new("a", counter.clone());
    let mut b2 = LlBlock::new("b", counter);
    let r1 = b1.fadd("1.0", "2.0");
    let r2 = b2.fadd("3.0", "4.0");
    assert_eq!(r1, "%r1");
    assert_eq!(r2, "%r2");
}
