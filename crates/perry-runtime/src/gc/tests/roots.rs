use super::super::*;
use super::support::*;

struct ActiveShadowFrame(u64);

impl ActiveShadowFrame {
    fn push_empty() -> Self {
        reset_shadow_stack();
        Self(js_shadow_frame_push(0))
    }
}

impl Drop for ActiveShadowFrame {
    fn drop(&mut self) {
        js_shadow_frame_pop(self.0);
    }
}

thread_local! {
    static LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

static LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_safe_runtime_scanner_test_guard() -> std::sync::MutexGuard<'static, ()> {
    LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_lock_safe_runtime_scanners_registered() {
    LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        gc_register_mutable_root_scanner(crate::tui::hooks::scan_hook_slot_roots_mut);
        gc_register_mutable_root_scanner(crate::tui::state::scan_state_slot_roots_mut);
        #[cfg(feature = "ohos-napi")]
        {
            gc_register_mutable_root_scanner(
                crate::arkts_callbacks::arkts_callbacks_root_scanner_mut,
            );
            gc_register_mutable_root_scanner(
                crate::media_playback::media_callbacks_root_scanner_mut,
            );
        }
        registered.set(true);
    });
}

fn lock_safe_runtime_scanner_closure() -> (*mut u8, u64, f64) {
    let ptr = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        let closure = ptr as *mut crate::closure::ClosureHeader;
        (*closure).func_ptr = test_no_capture_singleton_func as *const u8;
        (*closure).capture_count = 0;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
        layout_init_pointer_free(ptr);
    }
    let bits = POINTER_TAG | (ptr as u64 & POINTER_MASK);
    (ptr, bits, f64::from_bits(bits))
}

struct GcUnsafeZoneResetGuard;

impl GcUnsafeZoneResetGuard {
    fn clear() -> Self {
        GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
        GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
        Self
    }

    fn enter() -> Self {
        let guard = Self::clear();
        GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
        guard
    }
}

impl Drop for GcUnsafeZoneResetGuard {
    fn drop(&mut self) {
        GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
        GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[test]
fn lock_safe_runtime_scanners_tui_state_defers_gc_check_trigger() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
        trigger.set(MALLOC_STATE.with(|s| s.borrow().objects.len()));
    });

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        gc_check_trigger();
        assert_eq!(
            gc_collection_count(),
            before,
            "gc_check_trigger should defer while a state root lock is held"
        );
    });

    if gc_collection_count() == before {
        let mut status = JsGcStepResult::default();
        assert_eq!(
            js_gc_step_status(&mut status),
            JS_GC_STEP_STATUS_ACTIVE,
            "deferred trigger check should start bounded assist work after unlock"
        );
        let completed = complete_budgeted_gc_cycle();
        assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    }
    assert!(
        gc_collection_count() > before,
        "deferred trigger check should complete after the budgeted cycle is drained"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "state slot root should survive the deferred collection"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_tui_hooks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
    crate::tui::hooks::test_seed_hook_slot_roots(bits);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::hooks::test_with_hook_slots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while a hook root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after the hook root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "hook slot root should survive the deferred collection"
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (bits, bits, bits)
    );
}

#[test]
fn lock_safe_runtime_scanners_tui_state_defers_manual_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should defer while a state root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred manual GC should run after the state root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "state slot root should survive deferred manual GC"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_manual_gc_unsafe_zone_stays_noop_after_unlock() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::enter();

    let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should no-op while unsafe zones are active"
        );
    });

    assert_eq!(
        gc_collection_count(),
        before,
        "manual GC skipped by an unsafe zone must not flush after the state root lock unlocks"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_auto_gc_unsafe_zone_stays_noop() {
    // #1467: the affected v0.5.1025 binary let automatic threshold GC
    // collect while native server work had marked worker JSValue refs unsafe.
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _unsafe_zone = GcUnsafeZoneResetGuard::enter();

    _trigger.make_arena_trigger_due();

    let before = gc_collection_count();
    gc_check_trigger();

    assert_eq!(
        gc_collection_count(),
        before,
        "automatic threshold GC must no-op while unsafe zones are active"
    );
}

#[test]
fn lock_safe_runtime_scanners_deferred_manual_gc_respects_unsafe_zone_at_flush() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

    let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should defer while a state root lock is held"
        );
        GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
    });

    assert_eq!(
        gc_collection_count(),
        before,
        "deferred manual GC should re-check unsafe zones before flushing after unlock"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_deferred_auto_gc_respects_unsafe_zone_at_flush() {
    // #1467: a trigger deferred by a root lock must re-check unsafe zones
    // before flushing, or it can collect worker-held JSValues after unlock.
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

    let (ptr, _bits, value) = lock_safe_runtime_scanner_closure();
    let _handle = crate::tui::state::js_perry_tui_state_alloc(value);

    _trigger.make_arena_trigger_due();

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        gc_check_trigger();
        assert_eq!(
            gc_collection_count(),
            before,
            "automatic threshold GC should defer while a state root lock is held"
        );
        GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
    });

    assert_eq!(
        gc_collection_count(),
        before,
        "deferred automatic threshold GC should re-check unsafe zones before flushing after unlock"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "state slot root should remain tracked when deferred auto GC is skipped"
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_tui_hooks_defers_direct_full_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
    crate::tui::hooks::test_seed_hook_slot_roots(bits);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::hooks::test_with_hook_slots_locked(|| {
        let freed = gc_collect_inner();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct full GC should defer while a hook root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct full GC should run after the hook root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "hook slot root should survive deferred direct full GC"
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (bits, bits, bits)
    );
}

#[cfg(feature = "ohos-napi")]
#[test]
fn lock_safe_runtime_scanners_arkts_callbacks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::arkts_callbacks::test_clear_arkts_callback_roots();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let callback_idx = 17;
    crate::arkts_callbacks::test_seed_arkts_callback_root(callback_idx, value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::arkts_callbacks::test_with_arkts_callback_roots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while ArkTS callback roots are locked"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after ArkTS callback roots unlock"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "ArkTS callback root should survive deferred GC"
    );
    assert_eq!(
        crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
        bits
    );
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn lock_safe_runtime_scanners_media_callbacks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = i64::MIN + 861;
    crate::media_playback::test_seed_media_callback_roots(handle, value, value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::media_playback::test_with_media_callback_roots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while media callback roots are locked"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after media callback roots unlock"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "media callback root should survive deferred GC"
    );
    assert_eq!(
        crate::media_playback::test_media_callback_roots(handle),
        (bits, bits)
    );
}

#[test]
fn test_conservative_stack_scan_auto_policy_skips_active_shadow_frame() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    assert_eq!(
        conservative_stack_scan_mode_from_value(None),
        ConservativeStackScanMode::Auto
    );
    assert_eq!(
        conservative_stack_scan_decision_for(ConservativeStackScanMode::Auto, false),
        ConservativeStackScanDecision::Scan
    );

    let h = js_shadow_frame_push(1);
    assert!(shadow_stack_has_active_frame());
    assert_eq!(
        conservative_stack_scan_decision_for(
            ConservativeStackScanMode::Auto,
            shadow_stack_has_active_frame()
        ),
        ConservativeStackScanDecision::SkipShadowStackActive
    );
    js_shadow_frame_pop(h);
}

#[test]
fn test_conservative_stack_scan_env_off_disables_decision() {
    for value in ["0", "off", "false"] {
        let mode = conservative_stack_scan_mode_from_value(Some(value));
        assert_eq!(mode, ConservativeStackScanMode::Disabled);
        assert_eq!(
            conservative_stack_scan_decision_for(mode, false),
            ConservativeStackScanDecision::SkipDisabled
        );
        assert_eq!(
            conservative_stack_scan_decision_for(mode, true),
            ConservativeStackScanDecision::SkipDisabled
        );
    }
}

#[test]
fn test_conservative_stack_scan_full_preserves_legacy_fallback_decision() {
    for value in ["1", "on", "true", "full", "debug"] {
        let mode = conservative_stack_scan_mode_from_value(Some(value));
        assert_eq!(mode, ConservativeStackScanMode::Full);
        assert_eq!(
            conservative_stack_scan_decision_for(mode, false),
            ConservativeStackScanDecision::Scan
        );
        assert_eq!(
            conservative_stack_scan_decision_for(mode, true),
            ConservativeStackScanDecision::Scan
        );
    }
}

#[test]
fn test_shadow_stack_push_pop_single_frame() {
    reset_shadow_stack();
    assert_eq!(shadow_stack_depth(), 0);
    let h = js_shadow_frame_push(3);
    assert_eq!(shadow_stack_depth(), 1);
    // Slots initialized to 0.
    for i in 0..3 {
        assert_eq!(js_shadow_slot_get(i), 0, "slot {} not zero", i);
    }
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
    // After pop, reads return 0 (no active frame).
    assert_eq!(js_shadow_slot_get(0), 0);
}

#[test]
fn test_shadow_stack_slot_store_load() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(4);
    // Store some pointer bit patterns.
    js_shadow_slot_set(0, 0x7FFD_0000_1234_5678); // POINTER_TAG
    js_shadow_slot_set(1, 0x7FFF_0000_9ABC_DEF0); // STRING_TAG
    js_shadow_slot_set(2, 0); // hole
    js_shadow_slot_set(3, 0x7FF9_0200_0000_6B6F); // SSO "ok"
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_1234_5678);
    assert_eq!(js_shadow_slot_get(1), 0x7FFF_0000_9ABC_DEF0);
    assert_eq!(js_shadow_slot_get(2), 0);
    assert_eq!(js_shadow_slot_get(3), 0x7FF9_0200_0000_6B6F);
    // Out-of-range read returns 0 (clamp).
    assert_eq!(js_shadow_slot_get(4), 0);
    js_shadow_frame_pop(h);
}

#[test]
fn test_shadow_stack_bound_slot_reads_and_rewrites_original_storage() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(1);
    let mut local_slot = ptr_bits(0x1234_5678);

    js_shadow_slot_bind(0, &mut local_slot as *mut u64);
    assert_eq!(js_shadow_slot_get(0), ptr_bits(0x1234_5678));

    local_slot = ptr_bits(0x2222_3333);
    assert_eq!(js_shadow_slot_get(0), ptr_bits(0x2222_3333));

    js_shadow_slot_set(0, ptr_bits(0x4444_5555));
    assert_eq!(local_slot, ptr_bits(0x4444_5555));

    js_shadow_slot_set(0, 0);
    assert_eq!(js_shadow_slot_get(0), 0);
    assert_eq!(local_slot, ptr_bits(0x4444_5555));

    js_shadow_frame_pop(h);
}

#[test]
fn test_shadow_stack_nested_frames() {
    reset_shadow_stack();
    let outer = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x1111);
    js_shadow_slot_set(1, 0x2222);
    assert_eq!(shadow_stack_depth(), 1);

    let inner = js_shadow_frame_push(3);
    js_shadow_slot_set(0, 0xAAAA);
    js_shadow_slot_set(1, 0xBBBB);
    js_shadow_slot_set(2, 0xCCCC);
    assert_eq!(shadow_stack_depth(), 2);
    // Inner frame sees its own slots, not the outer's.
    assert_eq!(js_shadow_slot_get(0), 0xAAAA);
    assert_eq!(js_shadow_slot_get(1), 0xBBBB);
    assert_eq!(js_shadow_slot_get(2), 0xCCCC);

    js_shadow_frame_pop(inner);
    assert_eq!(shadow_stack_depth(), 1);
    // Outer slots preserved across the inner push+pop — this is
    // the load-bearing invariant for codegen: a called function
    // can freely mutate its own frame without corrupting the
    // caller's.
    assert_eq!(js_shadow_slot_get(0), 0x1111);
    assert_eq!(js_shadow_slot_get(1), 0x2222);

    js_shadow_frame_pop(outer);
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_frame_with_zero_slots() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(0);
    assert_eq!(shadow_stack_depth(), 1);
    // No slots to read; get returns 0 anyway (out-of-range path).
    assert_eq!(js_shadow_slot_get(0), 0);
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_deep_nesting() {
    reset_shadow_stack();
    let mut handles = Vec::new();
    for i in 0..16 {
        let h = js_shadow_frame_push(2);
        js_shadow_slot_set(0, i as u64);
        js_shadow_slot_set(1, (i * 2) as u64);
        handles.push(h);
    }
    assert_eq!(shadow_stack_depth(), 16);
    // Pop back down; slots restore on each pop.
    for i in (0..16).rev() {
        assert_eq!(js_shadow_slot_get(0), i as u64);
        assert_eq!(js_shadow_slot_get(1), (i * 2) as u64);
        js_shadow_frame_pop(handles.pop().unwrap());
    }
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_root_scanner_empty() {
    reset_shadow_stack();
    let mut count = 0;
    shadow_stack_root_scanner(&mut |_| count += 1);
    assert_eq!(count, 0, "empty shadow stack yields no roots");
}

#[test]
fn test_shadow_stack_root_scanner_single_frame() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(4);
    // Mix of set / unset slots.
    js_shadow_slot_set(0, 0x7FFD_0000_1234_5678);
    // slot 1 left zero — must NOT be emitted
    js_shadow_slot_set(2, 0x7FFF_0000_9ABC_DEF0);
    js_shadow_slot_set(3, 0x7FFA_0000_DEAD_BEEF);
    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
    assert_eq!(emitted.len(), 3, "only non-zero slots should be emitted");
    assert!(emitted.contains(&0x7FFD_0000_1234_5678));
    assert!(emitted.contains(&0x7FFF_0000_9ABC_DEF0));
    assert!(emitted.contains(&0x7FFA_0000_DEAD_BEEF));
    js_shadow_frame_pop(h);
}

#[test]
fn test_shadow_stack_root_scanner_nested_frames() {
    reset_shadow_stack();
    let outer = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0xAAAA);
    js_shadow_slot_set(1, 0xBBBB);
    let inner = js_shadow_frame_push(3);
    js_shadow_slot_set(0, 0xCCCC);
    js_shadow_slot_set(1, 0xDDDD);
    js_shadow_slot_set(2, 0xEEEE);

    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));

    // Scanner should hit BOTH frames — outer frame's slots
    // must also be reported, not just the innermost. This is
    // the load-bearing invariant for Phase B+ where the GC
    // collects while deep in a call chain.
    assert_eq!(emitted.len(), 5);
    assert!(emitted.contains(&0xAAAA));
    assert!(emitted.contains(&0xBBBB));
    assert!(emitted.contains(&0xCCCC));
    assert!(emitted.contains(&0xDDDD));
    assert!(emitted.contains(&0xEEEE));

    js_shadow_frame_pop(inner);
    js_shadow_frame_pop(outer);
}

#[test]
fn test_shadow_stack_root_scanner_zero_slot_frames() {
    reset_shadow_stack();
    // Zero-slot frame (function with no pointer-typed locals)
    // contributes nothing. Nested non-zero frame still works.
    let a = js_shadow_frame_push(0);
    let b = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x1234);
    js_shadow_slot_set(1, 0x5678);
    let c = js_shadow_frame_push(0);

    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
    assert_eq!(emitted.len(), 2);

    js_shadow_frame_pop(c);
    js_shadow_frame_pop(b);
    js_shadow_frame_pop(a);
}

// Issue #1830: a `throw` unwinds via `longjmp`, which skips the
// `js_shadow_frame_pop` epilogues of the functions being unwound past, so
// `frame_top` is left pointing at orphaned (now-dead) callee frames. A GC in
// the catch body would then scan — and the copying collector would rewrite —
// slots in stack frames that no longer exist. `js_try_push` captures a
// `ShadowSavepoint` and `js_throw` restores it before the `longjmp` so the
// orphaned frames are dropped first. This exercises the savepoint/restore pair.
#[test]
fn test_shadow_stack_savepoint_restore_drops_orphaned_frames() {
    reset_shadow_stack();

    // run()'s frame: two live pointer slots.
    let run_frame = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x7FFD_0000_1111_1111);
    js_shadow_slot_set(1, 0x7FFD_0000_2222_2222);

    // js_try_push captures the savepoint here, before any callee frame.
    let sp = shadow_stack_savepoint();

    // Inlined/called deep1 -> deep2 -> deep3 each push a frame; deep3 throws,
    // so none of their pops run.
    let _d1 = js_shadow_frame_push(1);
    js_shadow_slot_set(0, 0x7FFD_0000_AAAA_AAAA);
    let _d2 = js_shadow_frame_push(1);
    js_shadow_slot_set(0, 0x7FFD_0000_BBBB_BBBB);
    let _d3 = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x7FFD_0000_CCCC_CCCC);
    js_shadow_slot_set(1, 0x7FFD_0000_DDDD_DDDD);

    // Pre-restore (the #1830 bug): a GC scans the orphaned deep frames too.
    let mut before: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| before.push(v.to_bits()));
    assert_eq!(shadow_stack_depth(), 4);
    assert_eq!(before.len(), 6);
    assert!(before.contains(&0x7FFD_0000_AAAA_AAAA));
    assert!(before.contains(&0x7FFD_0000_CCCC_CCCC));

    // js_throw restores to the savepoint before longjmp.
    shadow_stack_restore(sp);

    // Post-fix: only run()'s frame remains; orphaned frames are gone.
    let mut after: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| after.push(v.to_bits()));
    assert_eq!(shadow_stack_depth(), 1, "restored to run()'s frame");
    assert_eq!(after.len(), 2);
    assert!(after.contains(&0x7FFD_0000_1111_1111));
    assert!(after.contains(&0x7FFD_0000_2222_2222));
    assert!(
        !after.contains(&0x7FFD_0000_CCCC_CCCC),
        "orphaned callee slot must no longer be scanned"
    );

    // The restored frame still pops cleanly with its original handle.
    js_shadow_frame_pop(run_frame);
    assert_eq!(shadow_stack_depth(), 0);
}

// Issue #1830, bound-slot variant: a callee that binds a shadow slot to its
// real local alloca (`js_shadow_slot_bind`) is the dangerous case — the
// copying collector reads AND writes back through `slot_ptrs`, so an orphaned
// bound slot points into already-unwound stack. After restore, the scanner
// reads neither the value nor the pointer of the dropped callee slot.
#[test]
fn test_shadow_stack_restore_drops_orphaned_bound_slots() {
    reset_shadow_stack();

    let run_frame = js_shadow_frame_push(1);
    let mut run_local: u64 = 0x7FFD_0000_5555_5555;
    js_shadow_slot_bind(0, &mut run_local as *mut u64);

    let sp = shadow_stack_savepoint();

    // Callee binds a slot to a local that becomes dead stack after unwind.
    let _callee = js_shadow_frame_push(1);
    let mut callee_local: u64 = 0x7FFD_0000_6666_6666;
    js_shadow_slot_bind(0, &mut callee_local as *mut u64);

    // Pre-restore: the scanner reads through the (soon-dead) callee binding.
    let mut before: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| before.push(v.to_bits()));
    assert!(before.contains(&0x7FFD_0000_6666_6666));
    assert!(before.contains(&0x7FFD_0000_5555_5555));

    shadow_stack_restore(sp);

    // Post-restore: only the surviving run() binding is read.
    let mut after: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| after.push(v.to_bits()));
    assert_eq!(after.len(), 1);
    assert!(after.contains(&0x7FFD_0000_5555_5555));
    assert!(!after.contains(&0x7FFD_0000_6666_6666));

    js_shadow_frame_pop(run_frame);
    assert_eq!(shadow_stack_depth(), 0);
}
