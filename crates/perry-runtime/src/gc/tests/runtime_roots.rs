use super::super::*;
use super::support::*;
use std::cell::Cell;
mod callback_scanners;
mod hook_dispatch_handles;
mod prototype_addr_cache;
mod side_table_scanners;
mod string_slice;
mod transient_handles;

fn assert_panics_with(expected: &str, f: impl FnOnce()) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let Err(payload) = result else {
        panic!("expected panic containing {expected:?}");
    };
    let message = if let Some(s) = payload.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic>"
    };
    assert!(
        message.contains(expected),
        "panic message {message:?} did not contain {expected:?}"
    );
}

fn force_next_general_arena_alloc_slow() {
    const TEST_BLOCK_SIZE: usize = 1024 * 1024;
    let _ = crate::arena::arena_alloc(TEST_BLOCK_SIZE, 8);
}

fn assert_marked_user_ptr(ptr: usize, label: &str) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should be marked"
        );
    }
}

fn assert_unmarked_user_ptr(ptr: usize, label: &str) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        assert_eq!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should not be marked"
        );
    }
}

fn assert_automatic_minor_gc_progressed(before: u64, context: &str) -> bool {
    if gc_collection_count() > before {
        return false;
    }

    let mut status = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_status(&mut status),
        JS_GC_STEP_STATUS_ACTIVE,
        "{context} should either finish a bounded assist or leave a budgeted GC cycle active"
    );
    assert_eq!(status.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    true
}

fn drain_scheduled_minor_gc(before: u64, context: &str) {
    let active = assert_automatic_minor_gc_progressed(before, context);
    if !active {
        assert!(
            gc_collection_count() > before,
            "{context} should complete during bounded assist or after host drain"
        );
        return;
    }

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        gc_collection_count() > before,
        "{context} should collect after the host drains the budgeted cycle"
    );
}

fn test_empty_copy_only_root_scanner(_mark: &mut dyn FnMut(f64)) {}

fn assert_callable_closure(bits: u64) -> usize {
    assert_eq!(bits & TAG_MASK, POINTER_TAG);
    let ptr = (bits & POINTER_MASK) as usize;
    assert_eq!(
        crate::closure::js_closure_call0(ptr as *const crate::closure::ClosureHeader),
        0.0
    );
    ptr
}

fn assert_moved_closure_ptr(bits: u64, original: usize) -> usize {
    assert_eq!(bits & TAG_MASK, POINTER_TAG);
    let rewritten = (bits & POINTER_MASK) as usize;
    assert_ne!(
        rewritten, original,
        "runtime callback root should be rewritten after copied-minor GC"
    );
    assert!(crate::arena::pointer_in_nursery(rewritten));
    rewritten
}

// `register_runtime_handle_root_scanner_for_tests` moved to `super::support`
// so the layout/tracing tests can root handles the same way (#6930 review).

#[test]
fn test_scoped_root_scanner_registry_guard_restores_counts() {
    let before = root_scanner_registry_counts();
    {
        let _guard = ScopedRootScannerRegistryGuard::new();
        gc_register_root_scanner(test_empty_copy_only_root_scanner);
        register_runtime_handle_root_scanner_for_tests();
        let during = root_scanner_registry_counts();
        assert_eq!(during.0, before.0 + 1);
        assert_eq!(during.1, before.1 + 1);
        assert_eq!(during.2, before.2);
        assert_eq!(during.3, before.3);
    }
    assert_eq!(root_scanner_registry_counts(), before);
}

thread_local! {
    static JSON_TAPE_HOOK_TARGET: std::cell::Cell<Option<crate::json_tape::JsonTapeSafepoint>> =
        const { std::cell::Cell::new(None) };
    static JSON_TAPE_HOOK_FIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JSON_TAPE_HOOK_PTR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn json_tape_force_minor_gc_hook(point: crate::json_tape::JsonTapeSafepoint, ptr: usize) {
    let should_collect = JSON_TAPE_HOOK_TARGET.with(|target| target.get() == Some(point))
        && JSON_TAPE_HOOK_FIRED.with(|fired| !fired.get());
    if !should_collect {
        return;
    }
    JSON_TAPE_HOOK_FIRED.with(|fired| fired.set(true));
    JSON_TAPE_HOOK_PTR.with(|slot| slot.set(ptr));
    let _ = crate::gc::gc_collect_minor();
}

struct JsonTapeSafepointHookGuard {
    previous: Option<crate::json_tape::JsonTapeSafepointHook>,
}

impl JsonTapeSafepointHookGuard {
    fn new(target: crate::json_tape::JsonTapeSafepoint) -> Self {
        JSON_TAPE_HOOK_TARGET.with(|slot| slot.set(Some(target)));
        JSON_TAPE_HOOK_FIRED.with(|slot| slot.set(false));
        JSON_TAPE_HOOK_PTR.with(|slot| slot.set(0));
        let previous =
            crate::json_tape::test_set_safepoint_hook(Some(json_tape_force_minor_gc_hook));
        Self { previous }
    }

    fn fired_ptr(&self) -> usize {
        assert!(
            JSON_TAPE_HOOK_FIRED.with(|slot| slot.get()),
            "JSON tape safepoint hook did not fire"
        );
        JSON_TAPE_HOOK_PTR.with(|slot| slot.get())
    }
}

impl Drop for JsonTapeSafepointHookGuard {
    fn drop(&mut self) {
        crate::json_tape::test_set_safepoint_hook(self.previous);
        JSON_TAPE_HOOK_TARGET.with(|slot| slot.set(None));
        JSON_TAPE_HOOK_FIRED.with(|slot| slot.set(false));
        JSON_TAPE_HOOK_PTR.with(|slot| slot.set(0));
    }
}

#[test]
fn test_forwarding_pointer_roundtrip() {
    // Allocate a nursery object, simulate evacuation by copying
    // its bytes into an old-gen alloc, install the forwarding
    // address in the nursery header. Read back via
    // forwarding_address to confirm round-trip.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        // Pre-condition: not forwarded yet.
        let nursery_hdr = header_from_user_ptr(nursery_user);
        assert_eq!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        // Install forwarding pointer.
        set_forwarding_address(nursery_hdr as *mut GcHeader, old_user);
        // Post-condition: flag set, address readable.
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        assert_eq!(forwarding_address(nursery_hdr), old_user);
    }
}

#[test]
fn test_forwarding_does_not_disturb_other_flags() {
    // Setting FORWARDED must preserve every other gc_flags bit.
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let hdr = header_from_user_ptr(user) as *mut GcHeader;
        // Set a few unrelated flags.
        (*hdr).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED | GC_FLAG_HAS_SURVIVED;
        let before = (*hdr).gc_flags;
        set_forwarding_address(hdr, old);
        let after = (*hdr).gc_flags;
        assert_eq!(after & GC_FLAG_FORWARDED, GC_FLAG_FORWARDED);
        // Every bit that was set before stays set.
        assert_eq!(
            after & before,
            before,
            "forwarding installation cleared an existing flag"
        );
    }
}

#[test]
fn test_forwarding_pointer_value_is_8_bytes_at_user_offset_zero() {
    // The forwarding pointer is stored in the first 8 bytes of
    // the user payload. This invariant is load-bearing for any
    // future walker that wants to skip over forwarded objects
    // by reading the new address inline. Verify by direct
    // pointer arithmetic.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let target = 0x12345678_9ABCDEF0_u64 as *mut u8;
    unsafe {
        let hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(hdr, target);
        // Read directly: user_ptr cast to *const *mut u8.
        let raw = nursery_user as *const *mut u8;
        assert_eq!(*raw, target);
    }
}

#[test]
fn test_rewrite_mutable_root_slots_updates_shadow_and_global_roots() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    reset_global_roots();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let nursery_hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(nursery_hdr, old_user);
    }

    let shadow_bits = POINTER_TAG | ((nursery_user as u64) & POINTER_MASK);
    let expected_shadow_bits = POINTER_TAG | ((old_user as u64) & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    let mut global_bits = nursery_user as u64;
    js_gc_register_global_root((&mut global_bits as *mut u64) as i64);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_shadow_bits,
        "shadow stack slot should be rewritten to the forwarding target"
    );
    assert_eq!(
        global_bits, old_user as u64,
        "registered global root slot should be rewritten in place"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_rewrite_mutable_root_slots_follows_forwarding_chain() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();

    let first = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let second = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let final_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(header_from_user_ptr(first) as *mut GcHeader, second);
        set_forwarding_address(header_from_user_ptr(second) as *mut GcHeader, final_user);
    }

    let shadow_bits = POINTER_TAG | (first as u64 & POINTER_MASK);
    let expected_bits = POINTER_TAG | (final_user as u64 & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_bits,
        "shadow stack slot should be rewritten through every forwarding hop"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_runtime_root_visitor_marks_and_rewrites_nanbox_slot() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }

    let mut slot = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    RuntimeRootVisitor::for_mark(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    unsafe {
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_MARKED, 0);
    }

    RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    assert_eq!(
        slot.to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
}

#[test]
fn test_prototype_resolution_stack_rewrites_owner_for_gc_reentry() {
    let base_depth = crate::object::prototype_chain::resolution_stack_savepoint();
    let nursery_owner = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let relocated_owner = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_owner) as *mut GcHeader,
            relocated_owner,
        );
    }

    assert!(
        crate::object::prototype_chain::test_resolution_stack_enter_and_forget(
            nursery_owner as usize,
        )
    );
    crate::object::prototype_chain::scan_prototype_resolution_stack_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );
    let reentry_was_blocked =
        !crate::object::prototype_chain::test_resolution_stack_enter_and_forget(
            relocated_owner as usize,
        );
    crate::object::prototype_chain::resolution_stack_restore(base_depth);

    assert!(
        reentry_was_blocked,
        "a post-GC reentrant lookup must match the rewritten active-owner slot"
    );
}

#[test]
fn test_prototype_resolution_stack_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::object::prototype_chain::scan_prototype_resolution_stack_roots_mut
                    as MutableRootScanner as usize
        })
    });
    assert!(
        registered,
        "the active prototype-resolution identities must be visible to moving GC"
    );
}

extern "C" fn test_reviver_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _key: f64,
    value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    value
}

thread_local! {
    static TEST_REVIVER_CLOSURE_VISITS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn test_reviver_count_closure_leaf(
    _closure: *const crate::closure::ClosureHeader,
    _key: f64,
    value: f64,
) -> f64 {
    let bits = value.to_bits();
    if bits & TAG_MASK == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() && (ptr as usize) >= GC_HEADER_SIZE + 0x1000 {
            let header = unsafe { ptr.sub(GC_HEADER_SIZE) as *const GcHeader };
            let is_closure = unsafe { (*header).obj_type == GC_TYPE_CLOSURE };
            if is_closure {
                TEST_REVIVER_CLOSURE_VISITS.with(|visits| visits.set(visits.get() + 1));
            }
        }
    }
    value
}

extern "C" fn test_promise_identity_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    crate::promise::test_current_microtask_value()
}

extern "C" fn test_promise_finally_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_array_identity_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _index: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let _ = crate::gc::gc_collect_minor();
    value_handle.get_nanbox_f64()
}

thread_local! {
    static TEST_FOREACH_FORCE_MINOR_VISITS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn test_foreach_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    key: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let key_handle = scope.root_nanbox_f64(key);
    let _ = crate::gc::gc_collect_minor();
    TEST_FOREACH_FORCE_MINOR_VISITS.with(|visits| visits.set(visits.get() + 1));
    let _ = value_handle.get_nanbox_f64();
    let _ = key_handle.get_nanbox_f64();
    0.0
}

extern "C" fn test_async_hook_init_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _async_id: f64,
    _type_name: f64,
    _trigger_async_id: f64,
    _resource: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_async_hook_event_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    _async_id: f64,
) -> f64 {
    let _ = crate::gc::gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

thread_local! {
    static TEST_TIMER_ARG_BITS: Cell<u64> = const { Cell::new(0) };
    static TEST_TIMER_CALLED: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn test_timer_capture_arg(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    TEST_TIMER_ARG_BITS.with(|slot| slot.set(arg.to_bits()));
    TEST_TIMER_CALLED.with(|slot| slot.set(true));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn test_rest_first_value(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let rest_ptr = (rest.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
    if rest_ptr.is_null() || crate::array::js_array_length(rest_ptr) == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    f64::from_bits(crate::array::js_array_get(rest_ptr, 0).bits())
}

static ASYNC_HOOK_RUNTIME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct AsyncHookRuntimeTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl AsyncHookRuntimeTestGuard {
    fn new() -> Self {
        let lock = ASYNC_HOOK_RUNTIME_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::async_hooks::reset_for_tests();
        crate::object::test_clear_transition_cache_root();
        crate::object::test_clear_object_cache_roots();
        Self { _lock: lock }
    }
}

impl Drop for AsyncHookRuntimeTestGuard {
    fn drop(&mut self) {
        crate::async_hooks::reset_for_tests();
        crate::object::test_clear_transition_cache_root();
        crate::object::test_clear_object_cache_roots();
        crate::exception::js_clear_exception();
    }
}

fn test_string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(string_bits(ptr as usize))
}

fn assert_moved_string_value(value: f64, original: usize, expected: &[u8]) {
    let bits = value.to_bits();
    assert_eq!(bits & TAG_MASK, STRING_TAG);
    let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
    assert_ne!(
        ptr as usize, original,
        "heap string value should be refreshed after copied-minor GC"
    );
    assert!(crate::arena::pointer_in_nursery(ptr as usize));
    unsafe {
        assert_string_bytes(ptr, expected);
    }
}

fn assert_string_value(value: f64, expected: &[u8]) {
    let bits = value.to_bits();
    assert_eq!(bits & TAG_MASK, STRING_TAG);
    let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(ptr, expected);
    }
}

fn hook_options(fields: &[(&[u8], *mut crate::closure::ClosureHeader)]) -> f64 {
    let obj = crate::object::js_object_alloc(0, fields.len() as u32);
    for (name, callback) in fields {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(ptr_bits(*callback as usize)),
        );
    }
    f64::from_bits(ptr_bits(obj as usize))
}

fn enable_async_hook(fields: &[(&[u8], *mut crate::closure::ClosureHeader)]) -> i64 {
    let options = hook_options(fields);
    let handle = crate::async_hooks::js_async_hooks_create_hook(options);
    crate::async_hooks::js_async_hook_enable(handle);
    handle
}

fn test_array_from_values(values: &[f64]) -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(values.len() as u32);
    unsafe {
        (*arr).length = values.len() as u32;
    }
    for (i, value) in values.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, *value);
    }
    arr
}

fn test_pair_array(key: f64, value: f64) -> *mut crate::array::ArrayHeader {
    test_array_from_values(&[key, value])
}

fn test_array_from_pair(pair: *mut crate::array::ArrayHeader) -> *mut crate::array::ArrayHeader {
    test_array_from_values(&[f64::from_bits(ptr_bits(pair as usize))])
}

fn drain_promise_microtasks_for_test() {
    for _ in 0..16 {
        if crate::promise::js_promise_run_microtasks() == 0 {
            return;
        }
    }
    panic!("promise microtask drain did not quiesce");
}
