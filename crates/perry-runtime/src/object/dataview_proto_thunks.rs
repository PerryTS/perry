//! `DataView.prototype` accessor + method thunks (#168-DataView reflection).
//!
//! Pre-fix, `DataView.prototype` existed and was correctly linked
//! (`Object.getPrototypeOf(new DataView(buf)) === DataView.prototype`) but was
//! **empty** — its `byteLength`/`byteOffset`/`buffer` accessors and its
//! `get*`/`set*` numeric methods were not installed as own properties. So
//! `Object.getOwnPropertyDescriptor(DataView.prototype, "getInt32")` returned
//! `undefined`, `typeof DataView.prototype.getInt32` was `"undefined"`, and
//! test262's `built-ins/DataView/*` suite — which reads these off the prototype
//! and invokes them via `.call(receiver, …)` — failed with "Cannot read
//! properties of undefined".
//!
//! DataView *instances* already worked: `dv.getInt32(0)`, `dv.byteLength`, etc.
//! are routed through codegen / `buffer_dispatch::dispatch_buffer_method` on a
//! `BufferHeader` marked as a DataView. The thunks here only add the
//! *reflectable* own properties on the prototype; they read the receiver from
//! `IMPLICIT_THIS` (set by the `.call`/`.apply` dispatch), brand-check that it
//! is a DataView (throwing `TypeError` otherwise, per spec — covering test262's
//! `this-has-no-*` / `this-is-not-object` cases), then dispatch to the SAME
//! runtime helpers the instance path uses.
//!
//! Installed onto `DataView.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

/// Resolve the `IMPLICIT_THIS` receiver to a DataView `BufferHeader` address,
/// or `None` if the receiver is not a DataView. Mirrors `typed_array_receiver`
/// / `array_buffer_receiver_addr` in `global_this.rs` (NaN-boxed pointer or
/// raw-i64 form), then brand-checks `is_data_view`.
fn dataview_receiver_addr() -> Option<usize> {
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = crate::value::JSValue::from_bits(this_bits);
    let raw = if this_jsv.is_pointer() {
        (this_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if this_bits >> 48 == 0 && this_bits > 0x10000 {
        this_bits as usize
    } else {
        return None;
    };
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_data_view(raw) {
        Some(raw)
    } else {
        None
    }
}

/// Brand-check helper: returns the DataView address or throws a `TypeError`
/// for an incompatible receiver. Mirrors `typed_array_brand_error`.
fn require_dataview_receiver() -> usize {
    match dataview_receiver_addr() {
        Some(addr) => addr,
        None => super::object_ops::throw_object_type_error(
            b"Method DataView.prototype called on incompatible receiver",
        ),
    }
}

fn undef() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// ---------------------------------------------------------------------------
// Accessor getters (0-arg): byteLength / byteOffset / buffer.
// Reflect as `{ get, set: undefined, enumerable: false, configurable: true }`.
// ---------------------------------------------------------------------------

extern "C" fn dataview_byte_length_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let addr = require_dataview_receiver();
    let buf = addr as *const crate::buffer::BufferHeader;
    f64::from_bits(
        crate::value::JSValue::number(crate::buffer::js_buffer_length(buf) as f64).bits(),
    )
}

extern "C" fn dataview_byte_offset_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let addr = require_dataview_receiver();
    let offset = crate::buffer::buffer_byte_offset(addr);
    f64::from_bits(crate::value::JSValue::number(offset as f64).bits())
}

extern "C" fn dataview_buffer_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let addr = require_dataview_receiver();
    let backing = crate::buffer::buffer_backing_array_buffer(addr);
    f64::from_bits(crate::value::js_nanbox_pointer(backing as i64).to_bits())
}

// ---------------------------------------------------------------------------
// get* methods (spec length 1): getInt8 … getFloat64. Signature
// `(closure, byteOffset, rest)` where `rest` bundles the optional
// `littleEndian` flag (big-endian default).
// ---------------------------------------------------------------------------

fn rest_first_arg(rest: f64) -> f64 {
    let value = crate::value::JSValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return undef();
    }
    let arr = value.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() || crate::array::js_array_length(arr) == 0 {
        return undef();
    }
    crate::array::js_array_get_f64(arr, 0)
}

fn dataview_get(suffix: &str, offset: f64, rest: f64) -> f64 {
    let addr = require_dataview_receiver();
    let kind = match crate::buffer::DataViewKind::from_method_suffix(suffix) {
        Some(k) => k,
        None => return undef(),
    };
    let little = crate::value::js_is_truthy(rest_first_arg(rest)) != 0;
    let buf_f64 = f64::from_bits(crate::value::JSValue::pointer(addr as *mut u8).bits());
    crate::buffer::js_data_view_get(buf_f64, offset, kind, little)
}

fn dataview_set(suffix: &str, offset: f64, value: f64, rest: f64) -> f64 {
    let addr = require_dataview_receiver();
    let kind = match crate::buffer::DataViewKind::from_method_suffix(suffix) {
        Some(k) => k,
        None => return undef(),
    };
    let little = crate::value::js_is_truthy(rest_first_arg(rest)) != 0;
    let buf_f64 = f64::from_bits(crate::value::JSValue::pointer(addr as *mut u8).bits());
    crate::buffer::js_data_view_set(buf_f64, offset, value, kind, little)
}

macro_rules! dataview_get_thunk {
    ($name:ident, $suffix:literal) => {
        extern "C" fn $name(
            _closure: *const crate::closure::ClosureHeader,
            offset: f64,
            rest: f64,
        ) -> f64 {
            dataview_get($suffix, offset, rest)
        }
    };
}

macro_rules! dataview_set_thunk {
    ($name:ident, $suffix:literal) => {
        extern "C" fn $name(
            _closure: *const crate::closure::ClosureHeader,
            offset: f64,
            value: f64,
            rest: f64,
        ) -> f64 {
            dataview_set($suffix, offset, value, rest)
        }
    };
}

dataview_get_thunk!(dv_get_int8, "Int8");
dataview_get_thunk!(dv_get_uint8, "Uint8");
dataview_get_thunk!(dv_get_int16, "Int16");
dataview_get_thunk!(dv_get_uint16, "Uint16");
dataview_get_thunk!(dv_get_int32, "Int32");
dataview_get_thunk!(dv_get_uint32, "Uint32");
dataview_get_thunk!(dv_get_float32, "Float32");
dataview_get_thunk!(dv_get_float64, "Float64");

dataview_set_thunk!(dv_set_int8, "Int8");
dataview_set_thunk!(dv_set_uint8, "Uint8");
dataview_set_thunk!(dv_set_int16, "Int16");
dataview_set_thunk!(dv_set_uint16, "Uint16");
dataview_set_thunk!(dv_set_int32, "Int32");
dataview_set_thunk!(dv_set_uint32, "Uint32");
dataview_set_thunk!(dv_set_float32, "Float32");
dataview_set_thunk!(dv_set_float64, "Float64");

/// Install the `DataView.prototype` accessors + numeric methods. Called from
/// `global_this::populate_builtin_prototype_methods`'s `"DataView"` arm.
pub(crate) fn install_dataview_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::{install_proto_method_rest, install_proto_method_rest_with_length};
    if proto_obj.is_null() {
        return;
    }

    // Accessor getters (0-arg). Reflect as `{ get, set: undefined,
    // enumerable: false, configurable: true }`.
    unsafe {
        let mk = |f: *const u8| -> u64 {
            crate::closure::js_register_closure_arity(f, 0);
            let c = crate::closure::js_closure_alloc(f, 0);
            if c.is_null() {
                0
            } else {
                crate::value::js_nanbox_pointer(c as i64).to_bits()
            }
        };
        let bl = mk(dataview_byte_length_getter_thunk as *const u8);
        if bl != 0 {
            super::object_ops::install_builtin_getter(proto_obj, "byteLength", bl);
        }
        let bo = mk(dataview_byte_offset_getter_thunk as *const u8);
        if bo != 0 {
            super::object_ops::install_builtin_getter(proto_obj, "byteOffset", bo);
        }
        let bf = mk(dataview_buffer_getter_thunk as *const u8);
        if bf != 0 {
            super::object_ops::install_builtin_getter(proto_obj, "buffer", bf);
        }
    }

    // get* methods: spec `.length === 1`, fixed call arity 1 (`byteOffset`),
    // the optional `littleEndian` flag is collected into `rest`.
    let gets: &[(&str, *const u8)] = &[
        ("getInt8", dv_get_int8 as *const u8),
        ("getUint8", dv_get_uint8 as *const u8),
        ("getInt16", dv_get_int16 as *const u8),
        ("getUint16", dv_get_uint16 as *const u8),
        ("getInt32", dv_get_int32 as *const u8),
        ("getUint32", dv_get_uint32 as *const u8),
        ("getFloat32", dv_get_float32 as *const u8),
        ("getFloat64", dv_get_float64 as *const u8),
    ];
    for (name, ptr) in gets.iter().copied() {
        install_proto_method_rest_with_length(proto_obj, name, ptr, 1, 1);
    }

    // set* methods: spec `.length === 2`, fixed call arity 2
    // (`byteOffset`, `value`), the optional `littleEndian` flag → `rest`.
    let sets: &[(&str, *const u8)] = &[
        ("setInt8", dv_set_int8 as *const u8),
        ("setUint8", dv_set_uint8 as *const u8),
        ("setInt16", dv_set_int16 as *const u8),
        ("setUint16", dv_set_uint16 as *const u8),
        ("setInt32", dv_set_int32 as *const u8),
        ("setUint32", dv_set_uint32 as *const u8),
        ("setFloat32", dv_set_float32 as *const u8),
        ("setFloat64", dv_set_float64 as *const u8),
    ];
    for (name, ptr) in sets.iter().copied() {
        // `install_proto_method_rest` registers spec_length == call_fixed_arity.
        install_proto_method_rest(proto_obj, name, ptr, 2);
    }
}
