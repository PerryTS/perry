// QueuingStrategy classes (#1545) and constructor strategy parsing (#4915),
// extracted from streams.rs (2000-line CI cap).

use super::*;

// ── #1545: node:stream/web QueuingStrategy classes ──────────────────────
//
// `new CountQueuingStrategy({ highWaterMark })` and
// `new ByteLengthQueuingStrategy({ highWaterMark })` produce plain objects
// with a numeric `highWaterMark` field and a `size` method, matching the
// WHATWG built-ins. CountQueuingStrategy.size always returns 1 (chunks are
// counted one-by-one); ByteLengthQueuingStrategy.size returns
// `chunk.byteLength`. Both are surfaced through codegen's builtin-`new`
// dispatch (lower_call/builtin.rs); the import binding lives in
// node_submodules.

/// `CountQueuingStrategy.prototype.size` — every chunk counts as 1.
extern "C" fn count_queuing_strategy_size(_c: *const ClosureHeader, _chunk: f64) -> f64 {
    1.0
}

/// `ByteLengthQueuingStrategy.prototype.size` — `chunk.byteLength`.
extern "C" fn byte_length_queuing_strategy_size(_c: *const ClosureHeader, chunk: f64) -> f64 {
    // Mirror Node's `return chunk.byteLength`: the generic property getter
    // resolves `.byteLength` for both registered buffers/typed arrays and
    // plain `{ byteLength }` objects.
    unsafe { perry_runtime::value::js_get_property(chunk, b"byteLength".as_ptr() as i64, 10) }
}

/// Build a `{ highWaterMark, size }` object for a queuing strategy. `hwm_bits`
/// is the raw JSValue bits read from the caller's options object.
unsafe fn build_queuing_strategy(
    hwm_bits: u64,
    size_fn: extern "C" fn(*const ClosureHeader, f64) -> f64,
) -> f64 {
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let k_hwm = js_string_from_bytes(b"highWaterMark".as_ptr(), 13);
    let k_size = js_string_from_bytes(b"size".as_ptr(), 4);
    js_array_push(keys, JSValue::string_ptr(k_hwm));
    js_array_push(keys, JSValue::string_ptr(k_size));
    js_object_set_field(obj, 0, JSValue::from_bits(hwm_bits));
    // `size` is a 1-arg native function value. Register the arity so closure
    // dispatch pads/forwards the single `chunk` argument correctly.
    let fn_ptr = size_fn as *const u8;
    perry_runtime::closure::js_register_closure_arity(fn_ptr, 1);
    let closure = perry_runtime::closure::js_closure_alloc(fn_ptr, 0);
    js_object_set_field(obj, 1, JSValue::pointer(closure as *const u8));
    js_object_set_keys(obj, keys);
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

/// Read `opts.highWaterMark` (raw JSValue bits) from a strategy's options
/// object; undefined when absent (matches `new CountQueuingStrategy({})`).
pub(super) unsafe fn read_high_water_mark(opts: f64) -> u64 {
    perry_runtime::value::js_get_property(opts, b"highWaterMark".as_ptr() as i64, 13).to_bits()
}

pub(super) unsafe fn read_queuing_strategy_size(strategy: f64) -> i64 {
    let size = perry_runtime::value::js_get_property(strategy, b"size".as_ptr() as i64, 4);
    closure_from_bits(size.to_bits())
}

/// Interpret a constructor "strategy" argument that may be a plain
/// highWaterMark number (the legacy ABI), a strategy object
/// (`{ highWaterMark, size }` — e.g. a ByteLengthQueuingStrategy), or
/// undefined. Returns `(high_water_mark, size_cb)` with the defaults
/// `(1.0, 0)` (#4915).
pub(crate) unsafe fn parse_strategy_value(value: f64) -> (f64, i64) {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top >= 0x7FF8 {
        if bits == TAG_UNDEFINED || bits == TAG_NULL {
            return (1.0, 0);
        }
        // INT32-tagged plain number (e.g. a literal highWaterMark).
        if top == 0x7FFE {
            return ((bits as u32 as i32) as f64, 0);
        }
        let hwm = f64::from_bits(read_high_water_mark(value));
        return (hwm, read_queuing_strategy_size(value));
    }
    (value, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_streams_strategy_high_water_mark(strategy: f64) -> f64 {
    f64::from_bits(read_high_water_mark(strategy))
}

/// `new CountQueuingStrategy({ highWaterMark })`.
#[no_mangle]
pub unsafe extern "C" fn js_count_queuing_strategy_new(opts: f64) -> f64 {
    let hwm = read_high_water_mark(opts);
    build_queuing_strategy(hwm, count_queuing_strategy_size)
}

/// `new ByteLengthQueuingStrategy({ highWaterMark })`.
#[no_mangle]
pub unsafe extern "C" fn js_byte_length_queuing_strategy_new(opts: f64) -> f64 {
    let hwm = read_high_water_mark(opts);
    build_queuing_strategy(hwm, byte_length_queuing_strategy_size)
}
