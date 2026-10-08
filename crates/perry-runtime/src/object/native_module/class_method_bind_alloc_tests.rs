//! A class-method value read checks the receiver for an own data property of
//! the method's name before it answers the shared prototype method. That
//! check compares the name's bytes against the keys in place: it must not
//! build a key string, on the shadowing hit or on the miss.
//!
//! Zod's `ZodType` constructor runs `this.m = this.m.bind(this)` for a dozen
//! methods per schema, so every read here used to mint one GC string.

use super::*;

const ALLOC_TEST_CLASS_ID: u32 = 0x0012_1700;
const METHOD: &[u8] = b"safeParseAsyncLongerName";

extern "C" fn return_receiver(this: f64) -> f64 {
    this
}

struct Scope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Scope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _suppress: crate::gc::GcSuppressScope::new(),
        }
    }
}

unsafe fn register_class() {
    crate::object::js_register_class_id(ALLOC_TEST_CLASS_ID);
    crate::object::class_registry::js_register_class_method(
        ALLOC_TEST_CLASS_ID as i64,
        METHOD.as_ptr(),
        METHOD.len() as i64,
        return_receiver as *const () as usize as i64,
        0,
        0,
        0,
    );
}

unsafe fn instance() -> (f64, *mut ObjectHeader) {
    let obj = crate::object::js_object_alloc(ALLOC_TEST_CLASS_ID, 0);
    (crate::value::js_nanbox_pointer(obj as i64), obj)
}

/// Arena bytes `f` bump-allocates. Collection is suppressed by the caller,
/// so the high-water mark only moves for a new allocation.
fn arena_bytes_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = crate::arena::arena_in_use_bytes();
    let result = f();
    (result, crate::arena::arena_in_use_bytes() - before)
}

#[test]
fn method_value_read_without_own_property_allocates_no_key() {
    let _scope = Scope::new();
    unsafe {
        register_class();
        let (receiver, _) = instance();
        // The first read builds the canonical per-class method value.
        let canonical = js_class_method_bind(receiver, METHOD.as_ptr(), METHOD.len());
        assert!(
            JSValue::from_bits(canonical.to_bits()).is_pointer(),
            "fixture: the class method must resolve to a function value"
        );
        let (again, bytes) = arena_bytes_during(|| {
            let mut last = 0.0;
            for _ in 0..64 {
                last = js_class_method_bind(receiver, METHOD.as_ptr(), METHOD.len());
            }
            last
        });
        assert_eq!(
            again.to_bits(),
            canonical.to_bits(),
            "method identity is canonical"
        );
        assert_eq!(
            bytes, 0,
            "an own-property miss must not allocate a key string"
        );
    }
}

#[test]
fn own_property_shadows_the_method_without_allocating_a_key() {
    let _scope = Scope::new();
    unsafe {
        register_class();
        let (receiver, obj) = instance();
        let canonical = js_class_method_bind(receiver, METHOD.as_ptr(), METHOD.len());
        let key = crate::string::js_string_from_bytes(METHOD.as_ptr(), METHOD.len() as u32);
        let own = 42.5f64;
        crate::object::js_object_set_field_by_name(obj, key, own);

        let ((plain, snapshot), bytes) = arena_bytes_during(|| {
            let mut plain = 0.0;
            let mut snapshot = 0.0;
            for _ in 0..64 {
                plain = js_class_method_bind(receiver, METHOD.as_ptr(), METHOD.len());
                snapshot = js_class_method_snapshot_bind(receiver, METHOD.as_ptr(), METHOD.len());
            }
            (plain, snapshot)
        });
        assert_eq!(
            plain.to_bits(),
            own.to_bits(),
            "[[Get]] answers the own property"
        );
        assert_eq!(
            snapshot.to_bits(),
            own.to_bits(),
            "the snapshot read answers it too"
        );
        assert_ne!(plain.to_bits(), canonical.to_bits());
        assert_eq!(
            bytes, 0,
            "an own-property hit must not allocate a key string"
        );
    }
}
