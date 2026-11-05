use super::frame_mark;
use crate::exception::{catch_js_throw, js_has_exception, js_throw};

/// Every managed stack must supply a witness that pushes a real entry owned
/// by the frame `mark` names. Tests both nested landings with a nonempty
/// outer scope, through the production `js_throw` + trap shim pad (the
/// wasm catch shim on WASI), never a test-only replay of the restore.
pub(super) fn assert_nested_restore(name: &str, depth: fn() -> usize, push: fn(u32, usize)) {
    let _lock = crate::gc::global_side_table_test_lock();
    let base = depth();
    let here = frame_mark();
    push(1, here);
    let outer = depth();
    assert!(
        outer > base,
        "{name}: fixture must establish live outer state"
    );
    let outcome: Result<(), f64> = catch_js_throw(|| {
        push(2, frame_mark());
        let inner = depth();
        assert!(
            inner > outer,
            "{name}: fixture must push inside the outer trap"
        );
        let nested: Result<(), f64> = catch_js_throw(|| {
            push(3, frame_mark());
            assert!(
                depth() > inner,
                "{name}: fixture must push inside the inner trap"
            );
            js_throw(31.0)
        });
        assert_eq!(nested, Err(31.0));
        assert_eq!(
            depth(),
            inner,
            "{name}: the inner pad must keep the entries of the frames still live"
        );
        js_throw(47.0)
    });
    assert_eq!(outcome, Err(47.0));
    assert_eq!(
        depth(),
        outer,
        "{name}: the outer pad must keep the entries of the frames still live"
    );
    assert_eq!(js_has_exception(), 0);
    assert_eq!(catch_js_throw(|| push(4, frame_mark())), Ok(()));
    assert_eq!(
        depth(),
        outer + 1,
        "{name}: a normal return must not run the restore"
    );
    // Clean up as a pad in this frame's caller would: everything this frame
    // and its callees pushed.
    super::unwind_managed_stacks(here + 1, usize::MAX);
    assert_eq!(
        depth(),
        base,
        "{name}: the caller-side unwind must reach the base"
    );
}

pub(super) mod async_context {
    pub(in super::super) fn depth() -> usize {
        crate::async_context::context_guard_depth()
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        crate::async_context::push_context_guard_at(
            mark,
            crate::async_context::ContextGuardAction::TestNoop,
        );
    }
}

pub(super) mod temp_roots {
    pub(in super::super) fn depth() -> usize {
        crate::gc::temp_root_len()
    }
    pub(in super::super) fn push(marker: u32, mark: usize) {
        crate::gc::temp_root_push_at((marker as f64).to_bits(), mark);
    }
}

/// `new.target` saves live on the handle stack: a pad that truncates past
/// saves puts back the value the oldest one displaced, and a save below the
/// pad's height (a live frame's) stays in force.
#[test]
fn new_target_restores_the_oldest_abandoned_save() {
    use crate::object::{
        js_new_target_enter, js_new_target_get, js_new_target_leave, js_new_target_set,
    };
    fn save(value: f64) {
        let scope = crate::gc::RuntimeHandleScope::new();
        scope.root_new_target_save(js_new_target_set(value));
        std::mem::forget(scope);
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let original = js_new_target_set(1.0);
    js_new_target_enter(2.0);
    let outcome: Result<(), f64> = catch_js_throw(|| {
        save(3.0);
        let nested: Result<(), f64> = catch_js_throw(|| {
            js_new_target_enter(4.0);
            save(5.0);
            assert_eq!(js_new_target_get(), 5.0);
            js_throw(31.0)
        });
        assert_eq!(nested, Err(31.0));
        assert_eq!(
            js_new_target_get(),
            3.0,
            "the inner pad restores the oldest abandoned save"
        );
        js_throw(47.0)
    });
    assert_eq!(outcome, Err(47.0));
    assert_eq!(
        js_new_target_get(),
        2.0,
        "the outer pad keeps the live frame's save"
    );
    js_new_target_leave();
    assert_eq!(js_new_target_get(), 1.0);
    js_new_target_set(original);
}

/// The runtime-handle stack restores by height (module docs): handles that
/// abandoned scopes left behind are dropped by the trap shim's pad, at both
/// nesting levels, and the handles of the frames still live survive.
#[test]
fn runtime_handles_restore_to_the_trap_height() {
    fn depth() -> usize {
        crate::gc::RuntimeHandleScope::active_len_for_tests()
    }
    fn leak(marker: u32) {
        let scope = crate::gc::RuntimeHandleScope::new();
        scope.root_nanbox_f64(marker as f64);
        std::mem::forget(scope);
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let base = depth();
    let outer_scope = crate::gc::RuntimeHandleScope::new();
    outer_scope.root_nanbox_f64(1.0);
    let outer = depth();
    assert!(outer > base, "fixture must establish live outer state");
    let outcome: Result<(), f64> = catch_js_throw(|| {
        let inner_scope = crate::gc::RuntimeHandleScope::new();
        inner_scope.root_nanbox_f64(2.0);
        let inner = depth();
        let nested: Result<(), f64> = catch_js_throw(|| {
            leak(3);
            assert!(depth() > inner, "fixture must push inside the inner trap");
            js_throw(31.0)
        });
        assert_eq!(nested, Err(31.0));
        assert_eq!(
            depth(),
            inner,
            "the inner pad must keep the live frames' handles"
        );
        leak(4);
        std::mem::forget(inner_scope);
        js_throw(47.0)
    });
    assert_eq!(outcome, Err(47.0));
    assert_eq!(
        depth(),
        outer,
        "the outer pad must keep the live frames' handles"
    );
    assert_eq!(js_has_exception(), 0);
    drop(outer_scope);
    assert_eq!(depth(), base);
}

pub(super) mod call_method {
    pub(in super::super) fn depth() -> usize {
        crate::object::call_method_depth() as usize
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        crate::object::test_enter_catch_method(mark);
    }
}

pub(super) mod pump {
    pub(in super::super) fn depth() -> usize {
        crate::stdlib_pump::pump_depth() as usize
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        crate::stdlib_pump::test_enter_catch_pump(mark);
    }
}

pub(super) mod set_foreach {
    pub(in super::super) fn depth() -> usize {
        crate::set::set_foreach_depth()
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        crate::set::test_enter_catch_foreach(mark);
    }
}

pub(super) mod map_foreach {
    pub(in super::super) fn depth() -> usize {
        crate::map::map_foreach_depth()
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        crate::map::test_enter_catch_foreach(mark);
    }
}

pub(super) mod prototype_resolution {
    pub(in super::super) fn depth() -> usize {
        crate::object::prototype_chain::resolution_stack_depth()
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        let owner = crate::object::js_object_alloc(0, 0);
        assert!(
            crate::object::prototype_chain::test_resolution_stack_enter_and_forget(
                owner as usize,
                mark
            )
        );
    }
}

pub(super) mod static_private_owner {
    pub(in super::super) fn depth() -> usize {
        crate::object::static_private_owner_depth()
    }
    pub(in super::super) fn push(marker: u32, mark: usize) {
        crate::object::static_private_owner_push_at(mark, marker as f64);
    }
}

pub(super) mod private_lexical_brand {
    pub(in super::super) fn depth() -> usize {
        crate::object::private_lexical_brand_depth()
    }
    pub(in super::super) fn push(marker: u32, mark: usize) {
        crate::object::private_lexical_brand_push_at(mark, marker as f64);
    }
}

pub(super) mod derived_super_binding {
    pub(in super::super) fn depth() -> usize {
        crate::object::derived_super_binding_len()
    }
    pub(in super::super) fn push(_: u32, mark: usize) {
        // The production stack stores native alloca addresses. No binding is
        // evaluated here, so an empty address suffices for the lifetime witness.
        crate::object::derived_super_scope_push_at(
            mark,
            std::ptr::null_mut(),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            0,
        );
    }
}

pub(super) mod private_member_access_hints {
    pub(in super::super) fn depth() -> usize {
        crate::object::private_member_access_hints_depth()
    }
    pub(in super::super) fn push(marker: u32, mark: usize) {
        crate::object::test_push_catch_private_hint(marker, mark);
    }
}

pub(super) mod namespace_override {
    pub(in super::super) fn depth() -> usize {
        crate::object::namespace_override_depth()
    }
    pub(in super::super) fn push(marker: u32, mark: usize) {
        crate::object::test_push_catch_namespace_override(marker, mark);
    }
}

/// `frame_mark` is read in the frame it is inlined into: an `#[inline(always)]`
/// helper answers its caller's stack pointer, not a deeper one. A mark taken
/// in a helper frame that already returned would sit below the owner's later
/// callees (module docs), so this is the property every push relies on, and it
/// must hold in the unoptimized test build too.
#[test]
fn frame_mark_names_the_calling_frame() {
    #[inline(always)]
    fn helper() -> usize {
        frame_mark()
    }
    #[inline(never)]
    fn callee() -> usize {
        frame_mark()
    }
    let here = frame_mark();
    assert_eq!(
        helper(),
        here,
        "an always-inlined helper reads its caller's frame"
    );
    assert!(callee() < here, "a real callee's frame lies below");
}

/// Pop-while by mark: a pad pops exactly the entries of the frames below its
/// stack pointer and keeps its own frame's. The one inversion the marks allow
/// (an owner's own entry, marked at the top of its frame, above an entry a
/// helper made for it just below its frame) pops together at every pad
/// outside the owner.
#[test]
fn a_pad_pops_by_mark_and_the_helper_inversion_pops_together() {
    let mut stack = super::CatchStack::<u32>::new();
    stack.push(200, 1);
    stack.push(200, 2);
    stack.push(100, 3);
    assert_eq!(stack.unwind_to(150), 1);
    assert_eq!(stack.iter().copied().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        stack.unwind_to(200),
        0,
        "entries of the pad's own frame survive"
    );
    assert_eq!(stack.unwind_to(201), 2);
    assert!(stack.is_empty());

    // Owner frame spans [96, 128): a helper's entry at 88 (just below), then
    // the owner's own entry at 120.
    stack.push(88, 4);
    stack.push(120, 5);
    assert_eq!(stack.unwind_to(64), 0, "a pad in a later callee keeps both");
    assert_eq!(
        stack.unwind_to(128),
        2,
        "a pad in the owner's caller pops both"
    );
}

/// Sabotage (design §5 test 10): skipping one stack's unwind at the pad must
/// turn that stack's restore test red, and skipping the whole pad restore
/// must turn the trap-depth test red. Each runs in a child test process with
/// the fault armed.
#[test]
fn each_skipped_unwind_turns_its_witness_red() {
    let exe = std::env::current_exe().unwrap();
    let mut cases: Vec<(String, String)> = [
        "async_context",
        "temp_roots",
        "call_method",
        "pump",
        "set_foreach",
        "map_foreach",
        "prototype_resolution",
        "static_private_owner",
        "private_lexical_brand",
        "derived_super_binding",
        "private_member_access_hints",
        "namespace_override",
    ]
    .iter()
    .map(|s| {
        (
            format!("unwind_{s}"),
            format!("exception::marks::restore_tests::{s}"),
        )
    })
    .collect();
    cases.push((
        "unwind_runtime_handles".into(),
        "exception::marks::tests::runtime_handles_restore_to_the_trap_height".into(),
    ));
    cases.push((
        "unwind_runtime_handles".into(),
        "exception::marks::tests::new_target_restores_the_oldest_abandoned_save".into(),
    ));
    cases.push((
        "pad_enter".into(),
        "exception::tests::a_throw_at_depth_lands_in_the_trap_pad".into(),
    ));
    for (fault, witness) in cases {
        let output = std::process::Command::new(&exe)
            .args(["--exact", &witness, "--nocapture", "--test-threads=1"])
            .env("PERRY_TEST_CALLBACK_SABOTAGE", &fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "sabotage must execute exactly one witness: {witness}"
        );
        assert!(
            !output.status.success(),
            "{fault} must make {witness} RED; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("eh sabotage {fault}: RED ({})", output.status);
    }
}

/// Exercise the production wasm checkpoint restore with real subsystem
/// entries. Equal frame marks deliberately prevent native mark unwinding from
/// distinguishing the prefix; the lexical depths must make that distinction.
#[cfg(not(windows))]
pub(super) fn assert_wasm_depth_restore(name: &str, depth: fn() -> usize, push: fn(u32, usize)) {
    use crate::exception::wasm::{js_wasm_eh_checkpoint_save, js_wasm_eh_pad_enter};
    use perry_abi::WasmEhCheckpoint;
    let _lock = crate::gc::global_side_table_test_lock();
    let mut base = WasmEhCheckpoint::default();
    let mut outer = WasmEhCheckpoint::default();
    let mut inner = WasmEhCheckpoint::default();
    unsafe {
        js_wasm_eh_checkpoint_save(&mut base);
        let before = depth();
        push(1, 1);
        js_wasm_eh_checkpoint_save(&mut outer);
        assert_eq!(depth(), before + 1, "{name}: establish surviving scope");
        for _ in 0..100 {
            push(2, 1);
            js_wasm_eh_checkpoint_save(&mut inner);
            push(3, 1);
            js_wasm_eh_pad_enter(&inner);
            assert_eq!(depth(), before + 2, "{name}: nested prefix survives");
            js_wasm_eh_pad_enter(&outer);
            js_wasm_eh_pad_enter(&outer);
            assert_eq!(
                depth(),
                before + 1,
                "{name}: restore is idempotent, no growth"
            );
        }
        js_wasm_eh_pad_enter(&base);
        assert_eq!(depth(), before, "{name}: outer prefix restored");
    }
}

#[test]
#[cfg(not(windows))]
fn wasm_checkpoint_restores_new_target_and_shadow_without_an_own_frame() {
    use crate::exception::wasm::{js_wasm_eh_checkpoint_save, js_wasm_eh_pad_enter};
    use crate::gc::{js_shadow_frame_push, shadow_stack_depth};
    use crate::object::{js_new_target_enter, js_new_target_get};
    let _lock = crate::gc::global_side_table_test_lock();
    let mut base = perry_abi::WasmEhCheckpoint::default();
    let mut outer = perry_abi::WasmEhCheckpoint::default();
    let original = js_new_target_get().to_bits();
    let shadow = shadow_stack_depth();
    unsafe {
        js_wasm_eh_checkpoint_save(&mut base);
        js_new_target_enter(7.0);
        js_wasm_eh_checkpoint_save(&mut outer);
        js_new_target_enter(8.0);
        js_new_target_enter(9.0);
        js_shadow_frame_push(2);
        js_shadow_frame_push(1);
        js_wasm_eh_pad_enter(&outer);
        assert_eq!(js_new_target_get(), 7.0);
        assert_eq!(shadow_stack_depth(), shadow);
        js_wasm_eh_pad_enter(&outer);
        js_wasm_eh_pad_enter(&base);
        assert_eq!(js_new_target_get().to_bits(), original);
        assert_eq!(shadow_stack_depth(), shadow);
    }
}

/// Nonempty contexts prove that restoring a depth applies its displaced
/// actions in reverse order rather than simply truncating their stack.
#[test]
#[cfg(not(windows))]
fn wasm_checkpoint_restores_nonempty_async_contexts() {
    use crate::async_context::{
        capture_context, context_guard_depth, enter_context, get_store,
        js_async_context_als_exit_enter, js_async_context_als_run_enter, push_context_guard,
        restore_context, set_store, AsyncLocalStoragePayload, ContextGuardAction,
    };
    use crate::exception::wasm::{js_wasm_eh_checkpoint_save, js_wasm_eh_pad_enter};
    let _lock = crate::gc::global_side_table_test_lock();
    let original = capture_context();
    let payload = AsyncLocalStoragePayload::default();
    let token = payload.token();
    let guards = context_guard_depth();
    let mut base = perry_abi::WasmEhCheckpoint::default();
    let mut outer = perry_abi::WasmEhCheckpoint::default();
    let mut inner = perry_abi::WasmEhCheckpoint::default();
    unsafe {
        js_wasm_eh_checkpoint_save(&mut base);
        js_async_context_als_run_enter(token, 7.0);
        js_wasm_eh_checkpoint_save(&mut outer);
        for _ in 0..100 {
            js_async_context_als_run_enter(token, 8.0);
            js_wasm_eh_checkpoint_save(&mut inner);
            js_async_context_als_exit_enter(token);
            assert_eq!(get_store(token), None);
            js_async_context_als_run_enter(token, 9.0);
            let displaced = enter_context(&original);
            push_context_guard(ContextGuardAction::RestoreSnapshot(displaced));
            assert_eq!(get_store(token), None);
            js_wasm_eh_pad_enter(&inner);
            assert_eq!(get_store(token), Some(8.0), "restore inner async context");
            assert_eq!(context_guard_depth(), guards + 2);
            js_wasm_eh_pad_enter(&outer);
            js_wasm_eh_pad_enter(&outer);
            assert_eq!(get_store(token), Some(7.0), "preserve outer async context");
            assert_eq!(context_guard_depth(), guards + 1);
        }
        // A checkpoint contains depths only: a surviving store stays mutable.
        set_store(token, 70.0);
        js_wasm_eh_pad_enter(&outer);
        assert_eq!(get_store(token), Some(70.0));
        js_wasm_eh_pad_enter(&base);
        assert_eq!(get_store(token), None);
        assert_eq!(context_guard_depth(), guards);
    }
    restore_context(original);
}
