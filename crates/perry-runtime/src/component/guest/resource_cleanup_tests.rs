use super::*;
use perry_abi::component_guest::ComponentGuestOps;
use std::sync::atomic::{AtomicU64, Ordering};

// Each small handle has its own byte-sized count; this never holds an address.
per_test_global! {
    pub(super) static COLLECTED: AtomicU64 = AtomicU64::new(0);
}

pub(super) unsafe extern "C" fn record_drop(
    _: *mut c_void,
    _: u32,
    operation: u32,
    handle: u32,
) -> u32 {
    assert_eq!(operation, 1);
    assert!(handle < 8);
    COLLECTED.fetch_add(1 << (8 * handle), Ordering::SeqCst);
    0
}

unsafe extern "C" fn throwing_drop(pointer: *mut c_void, ty: u32, op: u32, handle: u32) -> u32 {
    record_drop(pointer, ty, op, handle);
    crate::component::guest::frame::frame(pointer).guard::<()>(|_| fail("drop failed"));
    0
}

pub(super) struct TestOps {
    _table: Box<ComponentGuestOps>,
    previous: *mut ComponentGuestOps,
}
impl TestOps {
    pub(super) fn new(resource: unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> u32) -> Self {
        let mut table = Box::new(ComponentGuestOps {
            abi_version: perry_abi::component_guest::GUEST_ABI_VERSION,
            size: std::mem::size_of::<ComponentGuestOps>(),
            contract: b"{}".as_ptr(),
            contract_len: 2,
            import: no_import,
            resource,
        });
        let previous = crate::component::guest::GUEST_OPS.swap(&mut *table, Ordering::AcqRel);
        COLLECTED.store(0, Ordering::SeqCst);
        Self {
            _table: table,
            previous,
        }
    }
}
impl Drop for TestOps {
    fn drop(&mut self) {
        crate::component::guest::GUEST_OPS.store(self.previous, Ordering::Release);
    }
}

pub(super) fn method(value: f64, name: &[u8]) -> f64 {
    unsafe {
        crate::closure::js_native_call_value(
            field(value, name),
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    }
}

#[test]
fn collected_owners_drop_once_while_borrows_and_reused_handles_remain_live() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(record_drop);
    let borrowed = scope.root_nanbox_f64(undefined());
    let weak = scope.root_nanbox_f64(undefined());
    let original;
    {
        let temporary = RuntimeHandleScope::new();
        wrap(4, 1, 0, true, undefined(), undefined());
        let owner = temporary.root_nanbox_f64(wrap(4, 2, 0, true, undefined(), undefined()));
        original = owner.get_nanbox_f64().to_bits();
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(owner.get_nanbox_f64()) as i64,
        ));
        borrowed.set_nanbox_f64(method(owner.get_nanbox_f64(), b"borrow"));
        let explicit = temporary.root_nanbox_f64(wrap(4, 3, 0, true, undefined(), undefined()));
        method(explicit.get_nanbox_f64(), b"dispose");
        method(explicit.get_nanbox_f64(), b"dispose");
        let mut transfer = Frame::allocate_guest_frame();
        let old = owned(&mut transfer, 4, 4);
        transfer.guard(|f| lower(f, old, 4, true, false));
        transfer.guard(commit);
        assert!(transfer.error().is_none());
    }
    // Canonical handles are reusable; this owner is distinct from the old
    // transferred wrapper even though its type and numeric handle are equal.
    let renewed = scope.root_nanbox_f64(wrap(4, 4, 0, true, undefined(), undefined()));
    crate::gc::gc_collect_minor();
    let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
    assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
    assert_ne!(
        moved.to_bits(),
        original,
        "borrowed owner must actually move"
    );
    crate::promise::js_promise_run_microtasks();
    assert_eq!(COLLECTED.load(Ordering::SeqCst), (1 << 8) + (1 << 24));
    assert_eq!(get(live(borrowed.get_nanbox_f64()), HANDLE), 2.0);
    assert_eq!(get(live(renewed.get_nanbox_f64()), HANDLE), 4.0);
    borrowed.set_nanbox_f64(undefined());
    method(renewed.get_nanbox_f64(), b"dispose");
    method(renewed.get_nanbox_f64(), b"dispose");
    crate::gc::gc_collect_minor();
    crate::promise::js_promise_run_microtasks();
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 0x0101010100);
    crate::gc::gc_collect_minor();
    crate::promise::js_promise_run_microtasks();
    assert_eq!(
        COLLECTED.load(Ordering::SeqCst),
        0x0101010100,
        "no destructor may repeat"
    );
}

#[test]
fn a_failed_drop_retires_the_finalizer_state_before_propagating_its_exception() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let _ops = TestOps::new(throwing_drop);
    let owner = scope.root_nanbox_f64(wrap(4, 1, 0, true, undefined(), undefined()));
    let fields = scope.root_nanbox_f64(data(owner.get_nanbox_f64()));
    assert!(crate::exception::catch_js_throw(|| release(fields.get_nanbox_f64())).is_err());
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
    assert!(crate::exception::catch_js_throw(|| release(fields.get_nanbox_f64())).is_ok());
    assert_eq!(COLLECTED.load(Ordering::SeqCst), 1 << 8);
}
