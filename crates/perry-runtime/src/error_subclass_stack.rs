//! #9410 — the own `stack` an `Error` SUBCLASS instance gets at construction.
//!
//! Split out of `error.rs` to keep that file under the 2,000-line CI cap
//! (`scripts/check_file_size.sh`). Included from there with
//! `#[path = "error_subclass_stack.rs"] mod subclass_stack;` plus a
//! `pub use`, so `use super::*` resolves against `error.rs` and the
//! `#[no_mangle]` entry keeps its symbol.
//!
//! `class A extends Error {}` produces an ordinary `GC_TYPE_OBJECT` class
//! instance, not a `GC_TYPE_ERROR` `ErrorHeader` — deliberately, so the
//! subclass's own fields have somewhere to live. `alloc_error`, the only site
//! that fills `ErrorHeader.stack`, is therefore never reached, and
//! `Error.prototype` carries `name` and `message` but no `stack`. So
//! `new A("x").stack` was `undefined` while `new Error("x").stack` was a
//! string.

use super::*;

/// #9410: read `key` off `obj` as an owned `String`, or `None` when absent /
/// undefined / null. Local twin of `value::to_string`'s private helper — the
/// lazy subclass `stack` getter needs `name` and `message` the same way
/// `Error.prototype.toString` does.
unsafe fn error_object_field_string(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> Option<String> {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let v = crate::object::js_object_get_field_by_name(obj, key_ptr);
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let s_ptr = crate::value::js_jsvalue_to_string(f64::from_bits(v.bits()));
    if s_ptr.is_null() {
        return None;
    }
    Some(read_string_header_owned(s_ptr))
}

/// `"<name>: <message>"` for an Error SUBCLASS instance, following
/// `Error.prototype.toString` (§20.5.3.4) exactly as `value::to_string`'s
/// subclass arm does: `name` alone when `message` is empty, `message` alone
/// when `name` is empty. Absent `name` defaults to `"Error"`, matching the
/// value a subclass inherits from `Error.prototype`.
unsafe fn error_subclass_stack_head(receiver: f64) -> String {
    let receiver_ptr = crate::value::js_nanbox_get_pointer(receiver);
    if receiver_ptr == 0
        || !crate::value::addr_class::is_above_handle_band(receiver_ptr as usize)
        || !crate::object::is_valid_obj_ptr(receiver_ptr as *const u8)
    {
        return "Error".to_string();
    }
    // Each field read allocates (the key, and the ToString of a non-string
    // value), so the receiver is re-read through a handle between them rather
    // than held as a raw pointer across a collection.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(receiver);
    let obj = || {
        crate::value::js_nanbox_get_pointer(handle.get_nanbox_f64())
            as *const crate::object::ObjectHeader
    };
    let name = error_object_field_string(obj(), b"name").unwrap_or_else(|| "Error".to_string());
    let message = error_object_field_string(obj(), b"message").unwrap_or_default();
    if name.is_empty() {
        message
    } else if message.is_empty() {
        name
    } else {
        format!("{name}: {message}")
    }
}

/// Lazy `stack` getter for an Error SUBCLASS instance (#9410).
///
/// Capture slot 0 holds the frame string captured at CONSTRUCTION; the head
/// (`"<name>: <message>"`) is formatted HERE, on read, because that is what V8
/// does — `class E extends Error { constructor(m) { super(m); this.name = "E" }}`
/// reports `"E: m"`, and the assignment happens after `super()` returns. A
/// user `Error.prepareStackTrace` still wins, same as `captureStackTrace`'s
/// getter.
extern "C" fn error_subclass_stack_getter(closure: *const crate::closure::ClosureHeader) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    unsafe {
        if let Some(prep) = error_prepare_stack_trace_override() {
            let structured = build_structured_stack(10);
            let prep_ptr =
                crate::value::js_nanbox_get_pointer(prep) as *const crate::closure::ClosureHeader;
            return crate::closure::js_closure_call2(prep_ptr, receiver, structured);
        }
        // #9486: capture slot 0 holds the ENCODED capture (native return
        // addresses, plus the #5247 line when one was recorded), not a
        // finished frame line — resolving addresses to names is the expensive
        // half and belongs here, on read, not in the constructor.
        let frame = {
            let bits = crate::closure::js_closure_get_capture_bits(closure, 0);
            let ptr = (bits & crate::value::POINTER_MASK) as *const StringHeader;
            if ptr.is_null()
                || !crate::value::addr_class::is_above_handle_band(ptr as usize)
                || !crate::object::is_valid_obj_ptr(ptr as *const u8)
            {
                current_stack_frame()
            } else {
                frames_payload_to_lines(read_string_header_owned(ptr).as_bytes())
            }
        };
        let head = error_subclass_stack_head(receiver);
        let s = format!("{head}\n{frame}");
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        crate::value::js_nanbox_string(ptr as i64)
    }
}

/// Setter half of the subclass `stack` accessor (#9410).
///
/// Node's `stack` is writable — `err.stack = ""` is a common way to shorten a
/// diagnostic, and a getter-only property would turn that into a silent no-op
/// (sloppy mode) or a TypeError (strict). Redefine it as a plain
/// non-enumerable own data property: after a write the lazy formatting is
/// gone, and reads return exactly what was assigned, which is the observable
/// contract. (V8 keeps the accessor shape and stores into an internal slot,
/// so `getOwnPropertyDescriptor(err, "stack")` after a write still reports
/// `get`/`set` there and reports `value`/`writable` here. Same reads, same
/// enumerability, different reflection — a deliberate simplification.)
extern "C" fn error_subclass_stack_setter(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    let ptr = crate::value::js_nanbox_get_pointer(receiver);
    if ptr != 0
        && crate::value::addr_class::is_above_handle_band(ptr as usize)
        && crate::object::is_valid_obj_ptr(ptr as *const u8)
    {
        // The key allocation can collect, so both the receiver and the value
        // are re-read through handles after it.
        let scope = crate::gc::RuntimeHandleScope::new();
        let this_handle = scope.root_nanbox_f64(receiver);
        let value_handle = scope.root_nanbox_f64(value);
        let key_handle = scope.root_string_ptr(js_string_from_bytes(b"stack".as_ptr(), 5));
        let target = crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        crate::object::clear_accessor_descriptor(target as usize, "stack");
        // The generic field setter can grow property storage or invoke user
        // code. The accessor's receiver is an ordinary Error-subclass object
        // and `stack` remains an own key after its descriptor is cleared, so
        // this route reaches the setter's self-rooting ordinary-object tail.
        // Still, put the whole call inside nested `across_*` windows: only
        // scoped entry pointers feed it, and both the key and receiver used by
        // the post-call attribute write are re-read after it returns.
        let (((), stack_key), receiver) = this_handle.across_nanbox(|| {
            key_handle.across_const::<StringHeader, _>(|| {
                key_handle.with_const_ptr::<StringHeader, _>(|key| {
                    let target = crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
                        as *mut crate::object::ObjectHeader;
                    crate::object::js_object_set_field_by_name(
                        target,
                        key,
                        value_handle.get_nanbox_f64(),
                    );
                })
            })
        });
        let target =
            crate::value::js_nanbox_get_pointer(receiver) as *mut crate::object::ObjectHeader;
        if !stack_key.is_null()
            && crate::value::addr_class::is_above_handle_band(target as usize)
            && crate::object::is_valid_obj_ptr(target as *const u8)
        {
            crate::object::set_property_attrs(
                target as usize,
                unsafe { read_string_header_owned(stack_key) },
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// #9410: give an `Error` SUBCLASS instance the own `stack` property Node
/// gives it.
///
/// `class X extends Error {}` produces an ordinary `GC_TYPE_OBJECT` class
/// instance, never a `GC_TYPE_ERROR` `ErrorHeader` — deliberately, so the
/// subclass's own fields have somewhere to live — and `alloc_error`, the only
/// place that fills `ErrorHeader.stack`, is therefore never reached. Neither
/// is `Error.prototype`, which carries `name` and `message` but no `stack`.
/// So `new X("m").stack` was `undefined` while `new Error("m").stack` was a
/// string: the cc bundle's 93 `extends Error` classes lost every trace they
/// printed.
///
/// Installed as an accessor rather than a precomputed string for the reason
/// the getter documents: the head is `name`/`message` at READ time, and a
/// subclass constructor almost always assigns `this.name` after `super()`.
/// The FRAME is captured here, at construction, which is the part that would
/// be wrong if it were deferred.
///
/// Idempotent and defensive: a non-pointer receiver, a receiver that already
/// has its own `stack` (a subclass with a `stack` class field, or a second
/// call on the same instance), and a failed closure allocation all return
/// without touching anything.
#[no_mangle]
pub extern "C" fn js_error_subclass_capture_stack(this_val: f64) {
    unsafe {
        let ptr = crate::value::js_nanbox_get_pointer(this_val);
        if ptr == 0
            || !crate::value::addr_class::is_above_handle_band(ptr as usize)
            || !crate::object::is_valid_obj_ptr(ptr as *const u8)
        {
            return;
        }
        // Every heap value that outlives an allocation below gets a handle:
        // the receiver, the `"stack"` key and the captured frame string all
        // survive two closure births, and the moving scavenge relocates
        // anything it is not shown. (The `alloc_error` twin above roots for
        // exactly this reason.)
        let scope = crate::gc::RuntimeHandleScope::new();
        let this_handle = scope.root_nanbox_f64(this_val);
        let key_handle = scope.root_string_ptr(js_string_from_bytes(b"stack".as_ptr(), 5));

        let target = crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        if key_handle.with_const_ptr::<StringHeader, _>(|stack_key| {
            crate::object::object_ops::own_key_present(target, stack_key)
        }) {
            return;
        }

        // Capture the frames NOW — this is the whole point of installing at
        // construction rather than formatting on first read. #9486 makes the
        // captured value the raw return addresses; the getter turns them into
        // named lines.
        let frame = capture_frames_payload();
        let frame_ptr = js_string_from_bytes(frame.as_ptr(), frame.len() as u32);
        if frame_ptr.is_null() {
            return;
        }
        let frame_handle = scope.root_string_ptr(frame_ptr);

        let getter_fn = error_subclass_stack_getter as *const u8;
        let setter_fn = error_subclass_stack_setter as *const u8;
        crate::closure::js_register_closure_arity(getter_fn, 0);
        crate::closure::js_register_closure_arity(setter_fn, 1);
        let getter = crate::closure::js_closure_alloc(getter_fn, 1);
        if getter.is_null() {
            return;
        }
        let getter_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(getter as i64));
        let setter = crate::closure::js_closure_alloc(setter_fn, 0);
        if setter.is_null() {
            return;
        }
        let setter_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(setter as i64));

        // Re-read every pointer through its handle: the closure births above
        // may have moved all of them. `getter_handle` is a NaN-box handle, so
        // it is unboxed rather than read as a raw-pointer handle
        // (`get_raw_const_ptr` on a nanbox slot panics with "runtime handle
        // kind mismatch").
        let getter_ptr = crate::value::js_nanbox_get_pointer(getter_handle.get_nanbox_f64())
            as *mut crate::closure::ClosureHeader;
        frame_handle.with_const_ptr::<StringHeader, _>(|frame| {
            crate::closure::js_closure_set_capture_bits(
                getter_ptr,
                0,
                crate::value::js_nanbox_string(frame as i64).to_bits(),
            );
        });

        // The key-array append can grow (and therefore allocate), so it runs
        // BEFORE the closure bits and the descriptor-table key — an address
        // recorded ahead of it would be the pre-move one.
        // `ensure_key_in_keys_array` can allocate, but roots both incoming
        // pointers at entry. Scope the entry key and re-read it only after the
        // whole operation; no pre-call receiver pointer survives the closure.
        let (((), stack_key), receiver) = this_handle.across_nanbox(|| {
            key_handle.across_const::<StringHeader, _>(|| {
                key_handle.with_const_ptr::<StringHeader, _>(|key| {
                    crate::object::ensure_key_in_keys_array(
                        crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
                            as *mut crate::object::ObjectHeader,
                        key,
                    );
                })
            })
        });
        let descriptor_key = if stack_key.is_null() {
            "stack".to_string()
        } else {
            read_string_header_owned(stack_key)
        };
        let target =
            crate::value::js_nanbox_get_pointer(receiver) as *mut crate::object::ObjectHeader;
        crate::object::set_builtin_accessor_descriptor(
            target as usize,
            descriptor_key,
            crate::object::AccessorDescriptor {
                get: getter_handle.get_nanbox_f64().to_bits(),
                set: setter_handle.get_nanbox_f64().to_bits(),
            },
            // writable is N/A for an accessor; Node's `stack` is
            // non-enumerable and configurable.
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}

/// Generated-code-only callee (#9410): anchor against the auto-optimize LTO
/// dead-strip.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ERROR_SUBCLASS_CAPTURE_STACK: extern "C" fn(f64) = js_error_subclass_capture_stack;

#[cfg(test)]
mod tests {
    use super::*;

    fn str_ptr(bytes: &[u8]) -> *mut StringHeader {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    /// #9410. Installing the accessor allocates twice AFTER the frame string is
    /// born and after the receiver is in hand, so every one of those values has
    /// to be reachable through a handle rather than held as a raw pointer. This
    /// test forces evacuation so a collection actually relocates them, which is
    /// the only condition under which an unrooted pointer misbehaves.
    ///
    /// It also catches the plainer failure the first cut of that rooting shipped
    /// with: reading a NaN-box handle back with `get_raw_const_ptr` aborts the
    /// process with "runtime handle kind mismatch", so EVERY Error-subclass
    /// construction panicked. Nothing in the unit suite constructed a subclass,
    /// so only a compiled fixture saw it.
    #[test]
    fn capture_stack_installs_a_non_enumerable_own_accessor() {
        unsafe {
            let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
            let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
            crate::gc::register_runtime_handle_root_scanner_for_tests();

            let scope = crate::gc::RuntimeHandleScope::new();
            let obj = crate::object::js_object_alloc(0, 4);
            let this_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));

            let obj_now = || {
                crate::value::js_nanbox_get_pointer(this_handle.get_nanbox_f64())
                    as *mut crate::object::ObjectHeader
            };
            crate::object::js_object_set_field_by_name(
                obj_now(),
                str_ptr(b"name"),
                crate::value::js_nanbox_string(str_ptr(b"Named") as i64),
            );
            crate::object::js_object_set_field_by_name_nonenum(
                obj_now(),
                str_ptr(b"message"),
                crate::value::js_nanbox_string(str_ptr(b"boom") as i64),
            );

            js_error_subclass_capture_stack(this_handle.get_nanbox_f64());

            let target = obj_now();
            assert!(
                crate::object::object_ops::own_key_present(target, str_ptr(b"stack")),
                "`stack` must be an OWN key — node reports \
                 hasOwnProperty(err, 'stack') === true for an Error subclass"
            );
            let accessor = crate::object::get_accessor_descriptor(target as usize, "stack")
                .expect("an accessor descriptor must be installed for `stack`");
            assert_ne!(accessor.get, 0, "the lazy getter half must be installed");
            assert_ne!(
                accessor.set, 0,
                "the setter half must be installed — `err.stack = \"\"` must not \
                 become a silent no-op"
            );
            let attrs = crate::object::get_property_attrs(target as usize, "stack")
                .expect("`stack` must carry explicit attributes, not the enumerable default");
            assert!(
                !attrs.enumerable(),
                "`stack` must stay out of Object.keys / JSON.stringify"
            );
            assert!(attrs.configurable(), "node's `stack` is configurable");

            // Idempotent: a second capture (the dynamic-`new` replay can reach
            // an instance the super-call path already stamped) must not replace
            // the construction-time frame with a later one.
            let first_getter = accessor.get;
            js_error_subclass_capture_stack(this_handle.get_nanbox_f64());
            let again = crate::object::get_accessor_descriptor(obj_now() as usize, "stack")
                .expect("the accessor must survive a second capture");
            assert_eq!(
                again.get, first_getter,
                "a second capture must leave the first frame in place"
            );
        }
    }
}
