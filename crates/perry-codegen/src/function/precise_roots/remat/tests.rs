//! Rematerialized global-backed roots (see `remat.rs`).
//!
//! Every positive assertion here is paired with a control that MUST stay a
//! relocated root, so a lowering that rematerialized nothing — or everything —
//! fails in one direction or the other.

use super::{apply, plan, REMAT_MARK};
use crate::function::precise_roots::lower_precise_roots_to_native_stack;

const HANDLE: &str = "@m_.str.0.handle";
const KEYS: &str = "@perry_class_keys_m__C";

/// A named local holding a string literal (`%s`), an entry-hoisted class-keys
/// cache (`%k`), and a control root holding a call result (`%ctl`), all live
/// across the same collecting call. Values are consumed by plain stores so the
/// only statepoints are the two `@may_collect` calls. `backing` substitutes the string slot's
/// global so the negative arms can reuse the fixture.
fn fixture(backing: &str, extra: &str) -> String {
    format!(
        r#"@m_.str.0.handle = internal global double 0.0
@perry_class_keys_m__C = internal global i64 0
@perry_global_m__x = internal global double 0.0
declare void @js_shadow_slot_bind(i32, ptr)
declare i64 @may_collect()
@sink_d = global double 0.0
@sink_i = global i64 0

define void @f() gc "statepoint-example" {{
entry:
  %s = alloca double
  store double 0x7FFC000000000001, ptr %s
  %k = alloca i64
  %ctl = alloca i64
  store i64 0, ptr %ctl
  call void @js_shadow_slot_bind(i32 0, ptr %s)
  call void @js_shadow_slot_bind(i32 1, ptr %k)
  call void @js_shadow_slot_bind(i32 2, ptr %ctl)
  %h = load double, ptr {backing}
  store double %h, ptr %s
  %kk = load i64, ptr {KEYS}
  store i64 %kk, ptr %k
  %c0 = call i64 @may_collect()
  store i64 %c0, ptr %ctl
{extra}  %c1 = call i64 @may_collect()
  %r1 = load double, ptr %s
  store double %r1, ptr @sink_d
  %r2 = load i64, ptr %k
  store i64 %r2, ptr @sink_i
  %r3 = load i64, ptr %ctl
  store i64 %r3, ptr @sink_i
  ret void
}}
"#
    )
}

fn lines(ir: &str) -> Vec<&str> {
    ir.lines().collect()
}

fn roots() -> Vec<String> {
    vec!["%s".into(), "%k".into(), "%ctl".into()]
}

fn rewrite(ir: &str) -> String {
    let lowered = lower_precise_roots_to_native_stack(ir, "f", 3);
    let target = crate::codegen::default_target_triple();
    crate::inprocess::statepoint_rewritten_ir(&lowered, &target, "remat_fixture")
        .expect("fixture survives the statepoint rewrite")
}

fn relocate_count(rewritten: &str) -> usize {
    rewritten
        .lines()
        .filter(|l| l.contains("= call") && l.contains("@llvm.experimental.gc.relocate"))
        .count()
}

#[test]
fn string_handle_and_class_keys_slots_are_planned_and_the_control_is_not() {
    let ir = fixture(HANDLE, "");
    let plans = plan(&lines(&ir), &roots());
    assert_eq!(plans.get("%s").map(|p| p.global.as_str()), Some(HANDLE));
    assert_eq!(plans.get("%s").map(|p| p.global_ty), Some("double"));
    assert_eq!(plans.get("%k").map(|p| p.global.as_str()), Some(KEYS));
    assert_eq!(plans.get("%k").map(|p| p.global_ty), Some("i64"));
    assert!(
        !plans.contains_key("%ctl"),
        "a slot holding a call result must stay a root"
    );
}

#[test]
fn a_mutable_module_global_is_not_rematerialized() {
    // `@perry_global_*` is a registered root too, but the PROGRAM assigns it,
    // so a re-read could observe a later assignment.
    let ir = fixture("@perry_global_m__x", "");
    let plans = plan(&lines(&ir), &roots());
    assert!(!plans.contains_key("%s"), "{plans:?}");
    assert!(plans.contains_key("%k"), "the unrelated keys slot still qualifies");
}

#[test]
fn a_function_that_stores_to_the_global_disqualifies_it() {
    let ir = fixture(HANDLE, "  store double 0.0, ptr @m_.str.0.handle\n");
    let plans = plan(&lines(&ir), &roots());
    assert!(!plans.contains_key("%s"), "{plans:?}");
}

#[test]
fn a_second_value_or_an_escaping_address_disqualifies_the_slot() {
    // A call result stored into the literal's slot: no longer global-backed.
    let mixed = fixture(HANDLE, "  store i64 %c0, ptr %s\n");
    assert!(!plan(&lines(&mixed), &roots()).contains_key("%s"));
    // The slot's address escaping: anything may write through it.
    let escape = fixture(HANDLE, "  call void @use_ptr(ptr %s)\n");
    assert!(!plan(&lines(&escape), &roots()).contains_key("%s"));
    // A different string literal stored into the same slot.
    let two = fixture(
        HANDLE,
        "  %h2 = load double, ptr @m_.str.1.handle\n  store double %h2, ptr %s\n",
    );
    assert!(!plan(&lines(&two), &roots()).contains_key("%s"));
    // A constant equal to the marker would make the marker ambiguous.
    let marker = fixture(
        HANDLE,
        &format!("  store double 0x{REMAT_MARK:016X}, ptr %s\n"),
    );
    assert!(!plan(&lines(&marker), &roots()).contains_key("%s"));
}

#[test]
fn constants_are_kept_and_global_stores_become_the_marker() {
    let ir = fixture(HANDLE, "");
    let plans = plan(&lines(&ir), &roots());
    let out = apply(&lines(&ir), &plans);
    assert!(out.contains("store double 0x7FFC000000000001, ptr %s"));
    assert!(out.contains(&format!("store double 0x{REMAT_MARK:016X}, ptr %s")));
    assert!(out.contains(&format!("store i64 {}, ptr %k", REMAT_MARK as i64)));
    assert!(!out.contains("store double %h, ptr %s"));
    assert!(out.contains("%r1.rmg = load double, ptr @m_.str.0.handle"));
    assert!(out.contains("%r2.rmg = load i64, ptr @perry_class_keys_m__C"));
}

/// The acceptance property, through the real RS4GC pipeline: the two
/// global-backed slots take no root alloca and no `gc.relocate`, the uses after
/// the safepoint read the global, and the control is still relocated.
#[test]
fn rematerialized_slots_take_no_relocate_and_reload_after_the_safepoint() {
    let ir = fixture(HANDLE, "");
    let lowered = lower_precise_roots_to_native_stack(&ir, "f", 3);
    assert!(lowered.contains("%s = alloca double"), "{lowered}");
    assert!(lowered.contains("%k = alloca i64"), "{lowered}");
    assert!(
        lowered.contains("%ctl = alloca ptr addrspace(1)"),
        "the control must stay a root:\n{lowered}"
    );
    assert_eq!(
        lowered.matches("alloca ptr addrspace(1)").count(),
        1,
        "{lowered}"
    );

    let rewritten = rewrite(&ir);
    assert_eq!(
        relocate_count(&rewritten),
        1,
        "only the control may be relocated (once, at the second safepoint):\n{rewritten}"
    );

    // Every use of the literal and the keys array reads its global BELOW the
    // last collecting statepoint, and that load is what the consumer gets.
    let body: Vec<&str> = rewritten.lines().collect();
    let last_sp = body
        .iter()
        .rposition(|l| l.contains("@llvm.experimental.gc.statepoint") && l.contains("@may_collect"))
        .expect("the collecting call is a statepoint");
    for (global, consumer) in [(HANDLE, "@sink_d"), (KEYS, "@sink_i")] {
        let (idx, reg) = body
            .iter()
            .enumerate()
            .skip(last_sp + 1)
            .find_map(|(i, l)| {
                let (reg, rhs) = l.trim().split_once(" = ")?;
                let names_global = rhs.ends_with(&format!("ptr {global}"))
                    || rhs.contains(&format!("ptr {global},"));
                (rhs.starts_with("load ") && names_global).then(|| (i, reg.to_string()))
            })
            .unwrap_or_else(|| panic!("no reload of {global} below the safepoint:\n{rewritten}"));
        // The reload itself, or the marker select SCCP proved takes it (the
        // `-Os` pipeline's InstCombine folds `select i1 true` away later).
        let mut names = vec![reg.clone()];
        for l in &body[idx..] {
            if let Some((sel, rhs)) = l.trim().split_once(" = select i1 true, ") {
                if rhs.split(", ").next().is_some_and(|t| t.ends_with(&format!(" {reg}"))) {
                    names.push(sel.to_string());
                }
            }
        }
        let consumes = |l: &&str| {
            l.contains(consumer)
                && l
                    .split(|c: char| !(c.is_alphanumeric() || "%._".contains(c)))
                    .any(|tok| names.iter().any(|n| n == tok))
        };
        assert!(
            body[idx..].iter().any(consumes),
            "{consumer} must consume the reload {reg} of {global}:\n{rewritten}"
        );
    }
}

/// The negative direction: the same fixture with the rematerialization
/// suppressed (a mutable global for the literal) must relocate that slot — so
/// the relocate count above is measuring the transform, not the fixture.
#[test]
fn without_remat_the_same_slot_is_relocated() {
    let rewritten = rewrite(&fixture("@perry_global_m__x", ""));
    assert_eq!(
        relocate_count(&rewritten),
        3,
        "the module-global slot (at both safepoints) and the control (at the second) are relocated:\n{rewritten}"
    );
}

/// End to end from HIR: `const s = "…"` held across an allocation lowers with
/// no root for the literal and a handle reload after the allocation.
#[test]
fn a_const_string_local_across_an_allocation_is_reloaded_from_its_handle() {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Stmt};

    let _native = crate::codegen::helpers::NativeRootsPin::native();
    let mut hir = HirModule::new("remat_hir");
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String("remat-literal-7Q".to_string())),
            },
            Stmt::Let {
                id: 2,
                name: "o".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![("a".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Return(Some(Expr::Array(vec![Expr::LocalGet(1), Expr::LocalGet(2)]))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let ir = String::from_utf8(crate::compile_module(&hir, opts).expect("compiles"))
        .expect("utf-8");
    let func = ir
        .split("\ndefine ")
        .find(|f| f.contains("__build("))
        .expect("the build function is emitted");
    // The literal's handle global, by its bytes constant.
    let handle_loads: Vec<&str> = func
        .lines()
        .filter(|l| l.contains("load double, ptr @") && l.contains(".str.") && l.contains(".handle"))
        .collect();
    assert!(!handle_loads.is_empty(), "the literal is read from its handle:\n{func}");
    // No root slot is ever fed from a handle load.
    for line in func.lines() {
        if let Some(rest) = line.trim().strip_prefix("%rs4gc.b") {
            let (_, rhs) = rest.split_once(" = ").unwrap_or(("", ""));
            if let Some(src) = rhs.strip_prefix("bitcast double ").and_then(|r| r.split(' ').next()) {
                let def = func
                    .lines()
                    .find(|l| l.trim().starts_with(&format!("{src} = ")))
                    .unwrap_or("");
                assert!(
                    !(def.contains(".str.") && def.contains(".handle")),
                    "a string-handle value was stored into a relocated root:\n{line}\n{def}\n{func}"
                );
            }
        }
    }
    // The select-guarded reload exists (the literal's slot was planned).
    assert!(func.contains(".rmg = load double, ptr @"), "{func}");
}
