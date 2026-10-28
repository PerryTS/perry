//! Array subclass initialization, independent of node:stream.
use crate::closure::{js_closure_get_capture_ptr, ClosureHeader};
use crate::object::native_method_install::{install_methods_on_existing_object, StubFn};
use crate::object::{js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;
fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

#[inline]
unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    let gc_type = header.obj_type;
    (gc_type <= crate::gc::GC_TYPE_MAX).then_some(gc_type)
}

fn this_value(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    // Slot 0 was set by `build_object` to the NaN-boxed bits of the
    // host object value cast to i64; reverse the cast.
    if !closure.is_null() {
        let bits = js_closure_get_capture_ptr(closure, 0) as u64;
        // A `TAG_UNDEFINED` slot-0 marks a *prototype-method value* (e.g.
        // `Stream.prototype.on`, installed by `attach_event_emitter_prototype_methods`)
        // rather than an instance-bound method: it has no fixed receiver and must
        // read the call-site `this` (the method's receiver parameter, which
        // `Function.prototype.call`/`apply` supply), so `Stream.prototype.on.call(streamInstance, ev, fn)`
        // — readable-stream's `Readable.prototype.on` wrapper — registers the
        // listener on the instance, not on the prototype. `build_object` always
        // captures a real object pointer, so existing instance-bound closures
        // never hit this branch.
        if bits != crate::value::TAG_UNDEFINED {
            return crate::stream_hooks::ensure_lazy_stream(f64::from_bits(bits));
        }
    }
    crate::stream_hooks::ensure_lazy_stream(this.as_f64())
}

/// `super(n)` for a source-compiled `class X extends Array` (e.g. lru-cache's
/// `ZeroArray`: `class ZeroArray extends Array { constructor(n){ super(n);
/// this.fill(0) } }`). Perry models the subclass instance as a plain object,
/// not a real exotic Array, so `super(n)` initializes its elements store. In
/// the default representation, inherited methods resolve through
/// `Array.prototype` and are not stamped as enumerable own properties. The
/// legacy shape-carried kill switch retains its old compatibility closure.
/// The codegen `super()` lowering calls this entry point.
#[no_mangle]
pub extern "C" fn js_array_subclass_init(this: f64, n: f64) -> f64 {
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_root = scope.root_nanbox_f64(this);
    let obj = raw as *mut ObjectHeader;
    unsafe {
        crate::object::shapes::transition_object_shape_add_brand(
            obj,
            crate::array::ARRAY_EXOTIC_BRAND,
        );
    }
    let this = this_root.get_nanbox_f64();
    let obj = raw_ptr_from_value(this) as *mut ObjectHeader;
    // ToLength(n): undefined / NaN / <= 0 → 0; +Infinity (and any value past the
    // max array length) clamps to 2^53 - 1; otherwise floor(n).
    let len = {
        const MAX_SAFE_INTEGER: f64 = 9007199254740991.0; // 2^53 - 1
        let nv = JSValue::from_bits(n.to_bits());
        if nv.is_undefined() || n.is_nan() || n <= 0.0 {
            0.0
        } else if n.is_infinite() {
            MAX_SAFE_INTEGER
        } else {
            n.floor().min(MAX_SAFE_INTEGER)
        }
    };
    if crate::array::subclass_elements::array_subclass_elements_enabled() {
        // Elements-backed instance: `length` and the indices live in the
        // store, never as shape-carried properties.
        let scope = crate::gc::RuntimeHandleScope::new();
        let this_root = scope.root_nanbox_f64(this);
        unsafe {
            crate::array::subclass_elements::install_elements(obj, len.min(u32::MAX as f64) as u32)
        };
        return this_root.get_nanbox_f64();
    }
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    js_object_set_field_by_name(obj, length_key, len);
    let methods: [(&str, StubFn); 1] =
        [("fill", crate::fn_info!(ns_array_fill, 3; with_declared(3)))];
    install_methods_on_existing_object(obj, this, &methods, &[]);
    this
}

/// Array's overloaded constructor semantics for a source-compiled subclass.
/// One numeric argument is a length; every other argument list becomes the
/// initial indexed elements.
#[no_mangle]
pub unsafe extern "C" fn js_array_subclass_init_args(
    this: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let args = if args_ptr.is_null() || args_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    if args.len() == 1 && JSValue::from_bits(args[0].to_bits()).is_number() {
        return js_array_subclass_init(this, args[0]);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let args = scope.root_nanbox_f64_slice(args);
    js_array_subclass_init(this.get_nanbox_f64(), args.len() as f64);
    for (index, value) in args.iter().enumerate() {
        let name = index.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let receiver = this.get_nanbox_f64();
        let raw = raw_ptr_from_value(receiver) as *mut ObjectHeader;
        if !raw.is_null() {
            js_object_set_field_by_name(raw, key, value.get_nanbox_f64());
        }
    }
    this.get_nanbox_f64()
}

/// Legacy shape-carried compatibility closure for `Array.prototype.fill`.
extern "C" fn ns_array_fill(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
    start: f64,
    end: f64,
) -> f64 {
    // `fill(value, start?, end?)`. An omitted argument arrives as `undefined`
    // and selects the spec default (`0` / `length`); before this the stub had
    // arity 1, so `sub.fill(8, 1)` filled the WHOLE array instead of the tail
    // from index 1 (node: `7|8|8`, perry: `8|8|8`).
    let present = |v: f64| i32::from(!JSValue::from_bits(v.to_bits()).is_undefined());
    crate::array::js_array_fill_generic(
        this_value(closure, this),
        value,
        present(start),
        start,
        present(end),
        end,
    )
}
