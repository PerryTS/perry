//! Node `stream` module — `new Readable(opts)`, `new Writable(opts)`,
//! `new Duplex(opts)`, `new Transform(opts)`, `new PassThrough(opts)`,
//! and `Readable.from(iterable)`. Closes #631.
//!
//! Pre-fix, these constructors fell through to the generic `Expr::New`
//! placeholder (an empty `ObjectHeader`), so `r.on`, `r.pipe`, `.read`
//! etc. were all `undefined`. Any downstream code that touched stream
//! methods crashed with `(undefined).x is not a function`.
//!
//! This module mirrors the closure-fields pattern used by fs streams
//! (`crates/perry-runtime/src/fs.rs::build_stream_object`): allocate
//! an `ObjectHeader` keyed by method names whose values are NaN-boxed
//! closure pointers. Each closure captures the host object pointer in
//! slot 0, so chained calls like `.on(...).on(...).pipe(...)` return
//! `this` and the chain doesn't lose identity.
//!
//! Method semantics are minimal stubs — Node's stream surface (full
//! EventEmitter pump, backpressure, async iteration) is far beyond
//! the scope of this issue. The acceptance criterion (#631) is
//! byte-identical typeof output: every method name reports
//! `"function"`, and chained calls don't crash. Real data flow
//! through `read`/`write`/`pipe` is left for a dedicated streams
//! runtime rewrite.

use crate::closure::{js_closure_alloc, js_closure_get_capture_ptr, ClosureHeader};
use crate::object::{js_object_alloc_with_shape, js_object_set_field, ObjectHeader};
use crate::value::JSValue;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

// Shape ids — pick a band well clear of fs streams (`STREAM_SHAPE_ID =
// 0x7FFF_FE40` + method_count). Each method-set length must yield a
// unique cache key.
const READABLE_SHAPE_ID: u32 = 0x7FFF_FE60;
const WRITABLE_SHAPE_ID: u32 = 0x7FFF_FE70;
const DUPLEX_SHAPE_ID: u32 = 0x7FFF_FE80;

// ─────────────────────────────────────────────────────────────────
// Stub method bodies. Each receives the closure pointer (slot 0
// holds the host object's NaN-boxed bits cast to i64) plus its
// argument list. Bodies return either `this`, `null`, `true`, or
// `false`, matching the most useful subset of Node's contract for
// chained no-ops.
// ─────────────────────────────────────────────────────────────────

#[inline]
fn this_value(closure: *const ClosureHeader) -> f64 {
    // Slot 0 was set by `build_object` to the NaN-boxed bits of the
    // host object value cast to i64; reverse the cast.
    let bits = js_closure_get_capture_ptr(closure, 0) as u64;
    f64::from_bits(bits)
}

extern "C" fn ns_chain0(closure: *const ClosureHeader) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain1(closure: *const ClosureHeader, _a: f64) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain2(closure: *const ClosureHeader, _a: f64, _b: f64) -> f64 {
    this_value(closure)
}
extern "C" fn ns_chain3(closure: *const ClosureHeader, _a: f64, _b: f64, _c: f64) -> f64 {
    this_value(closure)
}

extern "C" fn ns_emit2(_closure: *const ClosureHeader, _e: f64, _a: f64) -> f64 {
    f64::from_bits(TAG_FALSE)
}
extern "C" fn ns_read1(_closure: *const ClosureHeader, _n: f64) -> f64 {
    f64::from_bits(TAG_NULL)
}
extern "C" fn ns_pipe1(_closure: *const ClosureHeader, dest: f64) -> f64 {
    // Node's `Readable.pipe(dest)` returns `dest` to allow `r.pipe(a).pipe(b)`.
    dest
}
extern "C" fn ns_write2(_closure: *const ClosureHeader, _chunk: f64, _enc: f64) -> f64 {
    f64::from_bits(TAG_TRUE)
}
extern "C" fn ns_listener_count(_closure: *const ClosureHeader, _e: f64) -> f64 {
    0.0
}
extern "C" fn ns_listeners(_closure: *const ClosureHeader, _e: f64) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}
extern "C" fn ns_undefined0(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

type StubFn = unsafe extern "C" fn();

#[allow(clippy::missing_transmute_annotations)]
fn cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
fn cast3(f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64) -> StubFn {
    unsafe { std::mem::transmute(f) }
}

// ─────────────────────────────────────────────────────────────────
// Build the host object: allocate an ObjectHeader sized to the
// method set, then fill each slot with a closure that captures the
// host object's NaN-boxed value (so `this` chains return identity).
// ─────────────────────────────────────────────────────────────────

fn build_object(methods: &[(&str, StubFn)], shape_id: u32) -> *mut ObjectHeader {
    // Pack the method names as a NUL-separated byte sequence, matching
    // the layout `js_object_alloc_with_shape` parses for shape keys.
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let field_count = methods.len() as u32;
    let obj =
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32);

    // NaN-box the object pointer — we'll capture it (as raw bits) in each
    // closure's slot 0 so the stub `this_value` helper can reconstruct
    // the f64 form for `return this` semantics.
    let this_bits = JSValue::pointer(obj as *const u8).bits();

    for (i, (_name, func)) in methods.iter().enumerate() {
        let closure = js_closure_alloc(*func as *const u8, 1);
        // Reuse `set_capture_ptr` (i64 payload). We only need 64 bits
        // and the NaN-boxed pattern fits cleanly when reinterpreted.
        crate::closure::js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        let val = JSValue::pointer(closure as *const u8);
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

// ─────────────────────────────────────────────────────────────────
// Method tables. Order is locked in — it determines the shape's
// packed-keys order. Each method set's length is a unique
// shape-cache key when added to its base shape id, so e.g. Readable's
// 17 methods at READABLE_SHAPE_ID don't collide with Writable's at
// WRITABLE_SHAPE_ID.
// ─────────────────────────────────────────────────────────────────

fn readable_methods() -> [(&'static str, StubFn); 17] {
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast1(ns_pipe1)),
        ("unpipe", cast1(ns_chain1)),
        ("pause", cast0(ns_chain0)),
        ("resume", cast0(ns_chain0)),
        ("destroy", cast1(ns_chain1)),
        ("setEncoding", cast1(ns_chain1)),
        ("isPaused", cast0(ns_undefined0)),
    ]
}

fn writable_methods() -> [(&'static str, StubFn); 16] {
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("write", cast2(ns_write2)),
        ("end", cast1(ns_chain1)),
        ("cork", cast0(ns_chain0)),
        ("uncork", cast0(ns_chain0)),
        ("destroy", cast1(ns_chain1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
        ("_write", cast3(ns_chain3)),
    ]
}

fn duplex_methods() -> [(&'static str, StubFn); 22] {
    // Union of readable + writable, deduped (`on/once/off/addListener/
    // removeListener/removeAllListeners/emit/listenerCount/listeners/
    // destroy` appear once each).
    [
        ("on", cast2(ns_chain2)),
        ("once", cast2(ns_chain2)),
        ("off", cast2(ns_chain2)),
        ("addListener", cast2(ns_chain2)),
        ("removeListener", cast2(ns_chain2)),
        ("removeAllListeners", cast1(ns_chain1)),
        ("emit", cast2(ns_emit2)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast1(ns_pipe1)),
        ("unpipe", cast1(ns_chain1)),
        ("pause", cast0(ns_chain0)),
        ("resume", cast0(ns_chain0)),
        ("setEncoding", cast1(ns_chain1)),
        ("isPaused", cast0(ns_undefined0)),
        ("write", cast2(ns_write2)),
        ("end", cast1(ns_chain1)),
        ("cork", cast0(ns_chain0)),
        ("uncork", cast0(ns_chain0)),
        ("destroy", cast1(ns_chain1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
    ]
}

// ─────────────────────────────────────────────────────────────────
// Public entry points — wired up by codegen's lower_builtin_new
// (`Readable`, `Writable`, `Duplex`, `Transform`, `PassThrough` arms)
// and by the `stream.from` NATIVE_MODULE_TABLE row for
// `Readable.from(iterable)`.
//
// Each takes a single `_opts` argument (NaN-boxed) for ABI parity
// with Node's constructor signature; the stub doesn't read fields
// off it (the `read` / `write` callback bodies aren't wired through),
// it's just kept around to avoid breaking the codegen's call shape.
// ─────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_node_stream_readable_new(_opts: f64) -> f64 {
    let methods = readable_methods();
    let obj = build_object(&methods, READABLE_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_new(_opts: f64) -> f64 {
    let methods = writable_methods();
    let obj = build_object(&methods, WRITABLE_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_new(_opts: f64) -> f64 {
    let methods = duplex_methods();
    let obj = build_object(&methods, DUPLEX_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Transform / PassThrough share Duplex's surface for the stub — the
/// `transform`/`flush` callbacks aren't wired through.
#[no_mangle]
pub extern "C" fn js_node_stream_transform_new(_opts: f64) -> f64 {
    js_node_stream_duplex_new(_opts)
}

#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_new(_opts: f64) -> f64 {
    js_node_stream_duplex_new(_opts)
}

/// `Readable.from(iterable)` — Node's static factory. Returns a
/// Readable object that would, in a real implementation, stream
/// chunks from the iterable. For the stub it's identical to
/// `new Readable()` — the iterable arg is accepted and discarded.
#[no_mangle]
pub extern "C" fn js_node_stream_readable_from(_iterable: f64) -> f64 {
    js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED))
}

// ─────────────────────────────────────────────────────────────────
// #1534: static introspection helpers `Readable.isDisturbed(s)` and
// `Readable.isErrored(s)`. Node returns booleans reflecting the
// stream's internal state machine; Perry's stream stubs don't track
// any of that state yet, so both return `false` — which is the
// correct answer for a freshly-constructed, untouched stream. The
// directional helpers `isReadable` / `isWritable` aren't here
// because Node's answer depends on the stream's actual direction
// (Readable returns `true` for isReadable + `null` for isWritable
// and so on); a uniform stub would lie for at least one case, so
// they're deferred until Perry's stream stub tracks direction.
// ─────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_node_stream_is_disturbed(_stream: f64) -> f64 {
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_node_stream_is_errored(_stream: f64) -> f64 {
    f64::from_bits(TAG_FALSE)
}

/// #1541: `stream.addAbortSignal(signal, stream)` — Node wires the
/// AbortSignal so aborting it destroys the stream, and returns the
/// stream for chaining. Perry's stream stubs don't implement the
/// destroy / abort propagation yet, so the helper just returns the
/// stream verbatim and ignores the signal. Caller chains
/// (`r = addAbortSignal(s, r)`) keep working with the same stream
/// reference they passed in.
#[no_mangle]
pub extern "C" fn js_node_stream_add_abort_signal(_signal: f64, stream: f64) -> f64 {
    stream
}
