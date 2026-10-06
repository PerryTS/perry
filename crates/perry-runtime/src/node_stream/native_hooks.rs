//! Native-payload streams (#11919, STREAM-PAYLOAD-DESIGN): one stream state
//! machine.
//!
//! A codec family's object (a zlib `Gzip`, a crypto `Hash`, the test-only
//! rot13 family) is an ordinary runtime Transform/Writable. Its payload holds
//! only the codec, and the runtime reaches the codec through the
//! [`StreamHooks`] its payload cell's vtable names (`meta.native_state` ->
//! cell -> `PayloadVTable::stream`): no method looked up by name, no table, no
//! class switch. Output re-enters through the runtime's own `push_chunk`, so
//! there is one readable queue, one writable buffer and one listener list,
//! whichever way the stream is driven (direct write, `pipe`, `pipeline`,
//! async iteration).
//!
//! Every write reaches [`begin_write`] through the ordinary writable machinery
//! (`write_state::do_write`: Node's `writing`, the buffered-write array,
//! `onwrite`). The write in flight lives in the stream's own hidden slots
//! (traced); the codec never holds a JS value or a callback address.
//! [`run_native_steps`] is the one step loop for every hooked stream:
//!
//! ```text
//! loop: if destroyed -> return
//!   rec = the write / flush / final in flight            (none -> return)
//!   out = hooks.step(payload, rec.op, chunk[rec.consumed..])  // no JS, no GC
//!   rec.consumed += out.consumed
//!   out.out_len > 0 -> push_chunk(stream, exact-size Buffer copy)
//!   re-read state (listeners ran); destroyed -> return
//!   More -> continue | NeedInput -> complete the write (onwrite)
//!   Ended -> push(null), finish | Error -> destroy(hooks.error(code))
//!   readable side full -> PARKED; return   (Transform's kCallback; resumed
//!                                           by `readable_maybe_read_more`)
//! ```
//!
//! `Deferred` families (zlib) run the loop from an immediate scheduled once per
//! burst (a runtime closure capturing the stream as a traced value), so `data`
//! never fires inside `write()`, matching a Node threadpool completion.
//! `Inline` families (crypto) run it at once, as Node's `_transform` calls
//! back synchronously.

use super::*;
use std::ffi::c_void;

/// LazyTransform's state accessors, installed once on its prototype. Ordinary
/// stream methods initialize through this_value; these cover direct state reads.
pub(crate) fn install_lazy_state_getters(proto: *mut crate::object::ObjectHeader) {
    let _no_move = crate::gc::GcSuppressScope::new();
    for (name, which) in [("_readableState", 0.0), ("_writableState", 1.0)] {
        let getter = crate::closure::js_closure_alloc(
            crate::fn_info!(lazy_state_get, 0; with_declared(0)),
            1,
        );
        crate::closure::js_closure_set_capture_f64(getter, 0, which);
        let setter = crate::closure::js_closure_alloc(
            crate::fn_info!(lazy_state_set, 1; with_declared(1)),
            1,
        );
        crate::closure::js_closure_set_capture_f64(setter, 0, which);
        crate::object::install_fresh_accessor_property(
            proto as usize,
            name.to_string(),
            crate::object::AccessorDescriptor {
                get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                set: crate::value::js_nanbox_pointer(setter as i64).to_bits(),
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}

extern "C" fn lazy_state_set(
    c: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let which = js_closure_get_capture_f64(c, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let value = scope.root_nanbox_f64(value);
    let name = if which == 0.0 {
        "_readableState"
    } else {
        "_writableState"
    };
    if let Some(obj) = object_ptr_from_value(stream.get_nanbox_f64()) {
        crate::object::define_builtin_data_property(
            obj,
            hidden_key(name.as_bytes()),
            value.get_nanbox_f64(),
            name.to_string(),
            crate::object::PropertyAttrs::new(true, true, true),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn lazy_state_get(c: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let which = js_closure_get_capture_f64(c, 0);
    let stream = constructors::ensure_lazy_stream(this.as_f64());
    let Some(obj) = object_ptr_from_value(stream) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    unsafe {
        own_field_by_key_bytes(
            obj,
            if which == 0.0 {
                b"_readableState"
            } else {
                b"_writableState"
            },
        )
    }
    .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

/// Which stream the family is: a Transform (readable output) or a Writable
/// (`Sign`/`Verify`: no readable side).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamKind(pub u32);

impl StreamKind {
    pub const TRANSFORM: Self = Self(0);
    pub const WRITABLE: Self = Self(1);
}

/// When the runtime runs the step loop for a write.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTiming(pub u32);

impl StepTiming {
    /// After `write()` returns, after nextTicks and microtasks (zlib: node
    /// finishes on the threadpool, never inside `write()`).
    pub const DEFERRED: Self = Self(0);
    /// Inside `write()` (crypto: node's `_transform` calls back at once).
    pub const INLINE: Self = Self(1);
}

/// What one step is asked to do.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamOp(pub u32);

impl StreamOp {
    /// Consume `input` (a written chunk, from `consumed` on).
    pub const WRITE: Self = Self(0);
    /// `.flush(kind, cb)`: flush with `flush_kind`, no input.
    pub const FLUSH: Self = Self(1);
    /// `end()`: finish the stream, no input.
    pub const FINAL: Self = Self(2);
}

/// How a step left the record.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepStatus(pub u32);

impl StepStatus {
    /// The record is done (its input is consumed / the flush completed).
    pub const NEED_INPUT: Self = Self(0);
    /// Call again for the same record (more output, or input left).
    pub const MORE: Self = Self(1);
    /// The stream's output is complete (`Final` only): push(null).
    pub const ENDED: Self = Self(2);
    /// The codec failed with `code`; [`StreamHooks::error`] builds the error.
    pub const ERROR: Self = Self(3);
}

/// One step's input. `input` is borrowed from the traced chunk for this step
/// only; a codec copies whatever it must keep across steps.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StepIn {
    pub op: StreamOp,
    pub flush_kind: i32,
    pub input: *const u8,
    pub len: usize,
}

/// One step's result. `out` points into the payload's own scratch and is
/// copied into an exact-length Buffer before anything else runs.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StepOut {
    pub consumed: usize,
    pub out: *const u8,
    pub out_len: usize,
    pub status: StepStatus,
    pub code: u32,
    /// Native bytes the payload retains after this step (engine state,
    /// scratch capacity, held header bytes): the runner restates the cell's
    /// external bytes with it, so GC pacing sees what the codec really holds.
    pub external_bytes: usize,
}

impl StepOut {
    const fn empty() -> Self {
        Self {
            consumed: 0,
            out: std::ptr::null(),
            out_len: 0,
            status: StepStatus::NEED_INPUT,
            code: 0,
            external_bytes: 0,
        }
    }
}

/// A stream family's codec entry points, named by its payload cell's vtable.
#[repr(C)]
pub struct StreamHooks {
    pub kind: StreamKind,
    pub timing: StepTiming,
    /// LazyTransform (Hash/Hmac/Cipher): stream state is built on first use.
    pub lazy: bool,
    /// Run one step. Never calls JS, never allocates on the GC heap, keeps no
    /// reference into `op.input` after it returns.
    pub step: unsafe extern "C" fn(payload: *mut c_void, op: &StepIn, out: &mut StepOut),
    /// A node-shaped Error for `code`, built after the step returned.
    pub error: unsafe extern "C" fn(owner: f64, code: u32) -> f64,
    /// Release the payload at `destroy()` (`native_payload::close`, plus any
    /// node-visible field such as zlib's `_handle = null`).
    pub release: unsafe extern "C" fn(owner: f64),
    /// Update ordinary owner fields after the payload borrow has ended.
    pub after_step: Option<unsafe extern "C" fn(owner: f64)>,
}

// SAFETY: hooks are immutable statics of function pointers and integers.
unsafe impl Sync for StreamHooks {}

// ─── hidden runner slots (non-enumerable stream state) ─────────────────────

const NATIVE_OP_KEY: &[u8] = b"__perryNativeOp";
const NATIVE_CHUNK_KEY: &[u8] = b"__perryNativeChunk";
const NATIVE_LEN_KEY: &[u8] = b"__perryNativeLen";
const NATIVE_CB_KEY: &[u8] = b"__perryNativeCb";
const NATIVE_CONSUMED_KEY: &[u8] = b"__perryNativeConsumed";
const NATIVE_FLUSH_KIND_KEY: &[u8] = b"__perryNativeFlushKind";
/// The record's input is fully consumed but its completion is held because
/// the readable side is full (node's Transform `kCallback`).
const NATIVE_DONE_KEY: &[u8] = b"__perryNativeDone";
const NATIVE_PARKED_KEY: &[u8] = b"__perryNativeParked";
const NATIVE_SCHEDULED_KEY: &[u8] = b"__perryNativeScheduled";
const NATIVE_RUNNING_KEY: &[u8] = b"__perryNativeRunning";

/// Op slot values (0 = no record in flight).
const REC_NONE: f64 = 0.0;
const REC_WRITE: f64 = 1.0;
const REC_FLUSH: f64 = 2.0;
const REC_FINAL: f64 = 3.0;
// A super._transform/_flush continuation completes the user's callback;
// its caller owns onwrite/finish, so it must not complete those twice.
const CALLBACK_ONLY: f64 = 4.0;
fn operation(record: f64) -> f64 {
    if record > CALLBACK_ONLY {
        record - CALLBACK_ONLY
    } else {
        record
    }
}

pub(crate) fn begin_prototype_step(stream: f64, chunk: f64, callback: f64, final_op: bool) -> bool {
    let Some(hooks) = hooks_of(stream) else {
        return false;
    };
    install_record(
        stream,
        CALLBACK_ONLY + if final_op { REC_FINAL } else { REC_WRITE },
        chunk,
        0.0,
        callback,
        0.0,
    );
    drive(stream, hooks);
    true
}

#[inline]
fn number_slot(stream: f64, key: &'static [u8]) -> f64 {
    get_hidden_value(stream, hidden_key(key))
        .and_then(jsvalue_as_f64)
        .unwrap_or(0.0)
}

#[inline]
fn flag(stream: f64, key: &'static [u8]) -> bool {
    has_truthy_hidden(stream, hidden_key(key))
}

#[inline]
fn set_flag(stream: f64, key: &'static [u8], on: bool) {
    super::set_internal_value(
        stream,
        key,
        f64::from_bits(if on { TAG_TRUE } else { TAG_FALSE }),
    );
}

// ─── resolution ────────────────────────────────────────────────────────────

/// The hooks of `stream`'s attached payload, when its family is a stream
/// family: `meta.native_state` -> cell -> `PayloadVTable::stream`.
#[inline]
pub(crate) fn hooks_of(stream: f64) -> Option<&'static StreamHooks> {
    crate::native_payload::stream_hooks_of(stream).map(|(hooks, _)| hooks)
}

/// Does a write on `stream` reach its codec? A JS `_transform` captured at
/// init (an option or a subclass override) takes precedence over the hooks:
/// the override may call `super._transform`, which runs the same step.
#[inline]
pub(super) fn native_write_target(stream: f64) -> Option<&'static StreamHooks> {
    let hooks = hooks_of(stream)?;
    #[cfg(test)]
    if stream_sabotage("hooks_first") {
        return Some(hooks);
    }
    if transform_hidden_callback(stream).is_some() {
        return None;
    }
    Some(hooks)
}

// ─── entry points from the writable machinery ──────────────────────────────

/// `doWrite` for a hooked stream: install the record in flight (the writable
/// machinery has set `writing`), then run or schedule the step loop. A chunk
/// whose encoding slot holds a number is a `.flush(kind)` record.
pub(super) fn begin_write(
    stream: f64,
    hooks: &'static StreamHooks,
    chunk: f64,
    enc: f64,
    len: f64,
    callback: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let chunk = scope.root_nanbox_f64(chunk);
    let callback = scope.root_nanbox_f64(callback);
    let flush_kind = JSValue::from_bits(enc.to_bits())
        .is_number()
        .then(|| jsvalue_as_f64(enc).unwrap_or(0.0));
    install_record(
        s.get_nanbox_f64(),
        if flush_kind.is_some() {
            REC_FLUSH
        } else {
            REC_WRITE
        },
        chunk.get_nanbox_f64(),
        len,
        callback.get_nanbox_f64(),
        flush_kind.unwrap_or(0.0),
    );
    drive(s.get_nanbox_f64(), hooks);
}

/// `_final` for a hooked Transform: the Final record runs once every buffered
/// write is done (`end()` reaches this only when the writable side drained).
/// The default Transform final waits for its output before writable finish.
/// A binding's own `_final` and `_flush` retain their ordinary JS ordering.
pub(super) fn begin_final(stream: f64, hooks: &'static StreamHooks, callback: Option<f64>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let cb = scope.root_nanbox_f64(callback.unwrap_or(f64::from_bits(TAG_UNDEFINED)));
    install_record(
        s.get_nanbox_f64(),
        REC_FINAL,
        f64::from_bits(TAG_UNDEFINED),
        0.0,
        cb.get_nanbox_f64(),
        0.0,
    );
    drive(s.get_nanbox_f64(), hooks);
}

/// `.flush(kind, cb)` on a hooked stream: a write record with op `Flush`,
/// ordered behind the buffered writes like node's flush-buffer write.
pub(crate) fn request_flush(stream: f64, kind: i32, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let cb = scope.root_nanbox_f64(callback);
    let empty = scope.root_nanbox_f64(buffer_value_from_bytes(&[]));
    super::write_state::write_record(
        s.get_nanbox_f64(),
        empty.get_nanbox_f64(),
        f64::from(kind),
        0.0,
        cb.get_nanbox_f64(),
    );
}

fn install_record(stream: f64, op: f64, chunk: f64, len: f64, callback: f64, flush_kind: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let chunk = scope.root_nanbox_f64(chunk);
    let callback = scope.root_nanbox_f64(callback);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_CHUNK_KEY, chunk.get_nanbox_f64());
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_CB_KEY, callback.get_nanbox_f64());
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_LEN_KEY, len);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_CONSUMED_KEY, 0.0);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_FLUSH_KIND_KEY, flush_kind);
    set_flag(s.get_nanbox_f64(), NATIVE_DONE_KEY, false);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_OP_KEY, op);
}

/// Define the runner's slots at construction, in one order for every
/// instance, so a native stream's state has one layout (its shape is shared
/// by every instance of the family instead of growing a per-object list at
/// the first write).
pub(crate) fn init_runner_slots(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_OP_KEY, REC_NONE);
    super::set_internal_value(
        s.get_nanbox_f64(),
        NATIVE_CHUNK_KEY,
        f64::from_bits(TAG_UNDEFINED),
    );
    super::set_internal_value(
        s.get_nanbox_f64(),
        NATIVE_CB_KEY,
        f64::from_bits(TAG_UNDEFINED),
    );
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_LEN_KEY, 0.0);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_CONSUMED_KEY, 0.0);
    super::set_internal_value(s.get_nanbox_f64(), NATIVE_FLUSH_KIND_KEY, 0.0);
    for key in [
        NATIVE_DONE_KEY,
        NATIVE_PARKED_KEY,
        NATIVE_SCHEDULED_KEY,
        NATIVE_RUNNING_KEY,
    ] {
        set_flag(s.get_nanbox_f64(), key, false);
    }
}

fn clear_record(stream: f64) {
    super::set_internal_value(stream, NATIVE_OP_KEY, REC_NONE);
    super::set_internal_value(stream, NATIVE_CHUNK_KEY, f64::from_bits(TAG_UNDEFINED));
    super::set_internal_value(stream, NATIVE_CB_KEY, f64::from_bits(TAG_UNDEFINED));
    set_flag(stream, NATIVE_DONE_KEY, false);
}

/// Run the loop now (Inline) or once from an immediate (Deferred). Inside a
/// running loop the new record is picked up by that loop.
fn drive(stream: f64, hooks: &'static StreamHooks) {
    if flag(stream, NATIVE_RUNNING_KEY) {
        return;
    }
    if hooks.timing == StepTiming::INLINE {
        run_native_steps(stream);
    } else {
        schedule(stream);
    }
}

fn schedule(stream: f64) {
    if flag(stream, NATIVE_SCHEDULED_KEY) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    set_flag(s.get_nanbox_f64(), NATIVE_SCHEDULED_KEY, true);
    let job = crate::closure::js_closure_alloc(crate::fn_info!(native_step_job, 0), 1);
    crate::closure::js_closure_set_capture_f64(job, 0, s.get_nanbox_f64());
    #[cfg(test)]
    if stream_sabotage("keep_step_closure") {
        let first = KEPT_JOBS.with(|jobs| {
            let mut jobs = jobs.borrow_mut();
            jobs.push(job as usize);
            jobs.len() == 1
        });
        if first {
            crate::gc::gc_register_mutable_root_scanner_named(
                "streamrt_kept_jobs",
                kept_jobs_scanner,
            );
        }
    }
    crate::timer::js_set_immediate_callback(job as i64);
}

#[cfg(test)]
thread_local! {
    static KEPT_JOBS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn kept_jobs_scanner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    KEPT_JOBS.with(|jobs| {
        for job in jobs.borrow_mut().iter_mut() {
            visitor.visit_tagged_usize_slot(job, crate::value::POINTER_TAG);
        }
    });
}

/// The deferred step job: a runtime closure holding the stream as a traced
/// capture. A job that finds the stream destroyed returns; it keeps the stream
/// alive at most one turn, like a node write in flight.
extern "C" fn native_step_job(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    #[cfg(test)]
    tests::STEP_JOBS_SEEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    set_flag(stream, NATIVE_SCHEDULED_KEY, false);
    run_native_steps(stream);
    f64::from_bits(TAG_UNDEFINED)
}

/// Node's Transform `_read` for a hooked stream: the readable side has room
/// again. A record whose input is consumed completes now (its write callback
/// was held); a record parked mid-input resumes stepping.
pub(super) fn resume_parked(stream: f64) {
    let Some(hooks) = hooks_of(stream) else {
        return;
    };
    if !flag(stream, NATIVE_PARKED_KEY) {
        return;
    }
    set_flag(stream, NATIVE_PARKED_KEY, false);
    if flag(stream, NATIVE_DONE_KEY) {
        complete_record(stream);
    }
    if number_slot(stream, NATIVE_OP_KEY) != REC_NONE {
        drive(stream, hooks);
    }
}

/// Is the readable side at or above its highWaterMark (node's
/// `rState.length >= rState.highWaterMark` in Transform's `_write`)?
fn readable_full(stream: f64) -> bool {
    #[cfg(test)]
    if stream_sabotage("never_park") {
        return false;
    }
    if !js_node_stream_has_readable_side(stream) {
        return false;
    }
    let length = get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0);
    let hwm = get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    length >= hwm
}

/// The bytes of `chunk` from `from` on: a Buffer / Uint8Array's storage, or a
/// string's UTF-8 bytes. Borrowed for one step: the pointer is re-derived from
/// the rooted chunk before every step, because a collection run by listeners
/// may move the chunk.

/// The one step loop for every hooked stream (see the module docs).
pub(crate) fn run_native_steps(stream: f64) {
    let Some(hooks) = hooks_of(stream) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let st = || s.get_nanbox_f64();
    if flag(st(), NATIVE_RUNNING_KEY) {
        return;
    }
    set_flag(st(), NATIVE_RUNNING_KEY, true);
    #[cfg(test)]
    let mut held_input: Option<(*const u8, usize)> = None;
    loop {
        if stream_destroyed(st()) || flag(st(), NATIVE_PARKED_KEY) {
            break;
        }
        let op = operation(number_slot(st(), NATIVE_OP_KEY));
        if op == REC_NONE {
            break;
        }
        if flag(st(), NATIVE_DONE_KEY) {
            // A consumed record whose completion was held: only the readable
            // side draining (`resume_parked`) completes it.
            break;
        }
        // The native loop is an allocating loop too. There is no borrowed
        // input or output here; the owner and its current record are traced.
        crate::gc::js_gc_loop_safepoint();
        let Some((_, cell)) = crate::native_payload::stream_hooks_of(st()) else {
            break;
        };
        // SAFETY: the cell is the live stream's own (traced) payload cell.
        let payload = unsafe { crate::native_handle::rust_payload_ptr_on_owner_thread(cell) };
        if payload.is_null() {
            // Released (destroy) or never attached: nothing to run.
            break;
        }
        let consumed = number_slot(st(), NATIVE_CONSUMED_KEY) as usize;
        let chunk = get_hidden_value(st(), hidden_key(NATIVE_CHUNK_KEY))
            .unwrap_or(f64::from_bits(TAG_UNDEFINED));
        // Strings may need materialization, which precedes the no_gc borrow.
        let string = if op == REC_WRITE && JSValue::from_bits(chunk.to_bits()).is_any_string() {
            let mut bytes = Vec::new();
            append_chunk_bytes(chunk, &mut bytes, 0);
            Some(bytes)
        } else {
            None
        };
        let (total, start, out) = crate::buffer::bytes::no_gc(|scope| {
            let bytes = if op != REC_WRITE {
                &[][..]
            } else if let Some(bytes) = &string {
                bytes.as_slice()
            } else {
                crate::buffer::bytes::bytes(chunk, scope).unwrap_or(&[])
            };
            let (base, total) = (bytes.as_ptr(), bytes.len());
            #[cfg(test)]
            let (base, total) = if stream_sabotage("hold_slice_across_push") && op == REC_WRITE {
                // Deliberately violate the borrow boundary. Heap strings move,
                // unlike the owned UTF-8 scratch above: keeping their interior
                // address makes this fault deterministic after a listener GC.
                *held_input.get_or_insert_with(|| {
                    if JSValue::from_bits(chunk.to_bits()).is_string() {
                        crate::string::with_string_value_bytes(chunk, |bytes| {
                            (bytes.as_ptr(), bytes.len())
                        })
                        .unwrap()
                    } else {
                        (base, total)
                    }
                })
            } else {
                (base, total)
            };
            let start = consumed.min(total);
            let step_in = StepIn {
                op: if op == REC_WRITE {
                    StreamOp::WRITE
                } else if op == REC_FLUSH {
                    StreamOp::FLUSH
                } else {
                    StreamOp::FINAL
                },
                flush_kind: number_slot(st(), NATIVE_FLUSH_KIND_KEY) as i32,
                input: unsafe { base.add(start) },
                len: total - start,
            };
            let mut out = StepOut::empty();
            // No JS or GC while the input borrow is held. Each iteration
            // re-borrows from the traced chunk after listeners may move it.
            unsafe { (hooks.step)(payload, &step_in, &mut out) };
            (total, start, out)
        });
        super::set_internal_value(
            st(),
            NATIVE_CONSUMED_KEY,
            (start + out.consumed.min(total - start)) as f64,
        );
        // Each output root lasts one step. Rooting in the outer runner scope
        // would keep the entire decompressed output alive until EOF.
        let step_scope = crate::gc::RuntimeHandleScope::new();
        let cell = step_scope.root_raw_mut_ptr(cell);
        // Copy the output out of the payload's scratch before anything that
        // can allocate or run JS.
        let output = (out.out_len > 0 && !out.out.is_null()).then(|| {
            // SAFETY: the step returned `out_len` readable bytes at `out`.
            let bytes = unsafe { std::slice::from_raw_parts(out.out, out.out_len) };
            step_scope.root_nanbox_f64(buffer_value_from_bytes(bytes))
        });
        // SAFETY: the cell is alive (its owner is rooted above).
        cell.with_mut_ptr(|cell| unsafe {
            crate::native_handle::native_handle_set_external_bytes(cell, out.external_bytes)
        });
        if let Some(after_step) = hooks.after_step {
            unsafe { after_step(st()) };
            if stream_destroyed(st()) {
                break;
            }
        }
        let status = error_as_end(out.status);
        if status == StepStatus::ERROR {
            if let Some(buf) = &output {
                let _ = push_chunk(st(), buf.get_nanbox_f64());
            }
            fail_record(st(), hooks, out.code);
            break;
        }
        if let Some(buf) = &output {
            let _ = push_chunk(st(), buf.get_nanbox_f64());
            // `data` listeners ran: a write, an end or a destroy may have
            // happened. Re-read everything from the stream.
            if stream_destroyed(st()) {
                break;
            }
        }
        if status == StepStatus::MORE {
            if readable_full(st()) && !exhaust_before_park() {
                set_flag(st(), NATIVE_PARKED_KEY, true);
                break;
            }
            continue;
        }
        if status == StepStatus::ENDED {
            finish_final(st());
            continue;
        }
        // NeedInput: the record is done. Hold its completion while the
        // readable side is full (node's kCallback); complete it otherwise.
        if op != REC_FINAL && readable_full(st()) && !stream_hidden_ended(st()) {
            set_flag(st(), NATIVE_DONE_KEY, true);
            set_flag(st(), NATIVE_PARKED_KEY, true);
            break;
        }
        if op == REC_FINAL {
            // A Final step that needs no more input is complete.
            finish_final(st());
            continue;
        }
        #[cfg(test)]
        {
            // The sabotage holds one record's slice, not the next record's.
            held_input = None;
        }
        complete_record(st());
    }
    set_flag(st(), NATIVE_RUNNING_KEY, false);
}

#[inline]
fn error_as_end(status: StepStatus) -> StepStatus {
    #[cfg(test)]
    if status == StepStatus::ERROR && stream_sabotage("error_as_end") {
        return StepStatus::ENDED;
    }
    status
}

#[inline]
fn exhaust_before_park() -> bool {
    #[cfg(test)]
    {
        stream_sabotage("exhaust_before_park")
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// The write/flush record in flight completed: clear it and run node's
/// `onwrite` (which may start the next buffered write: a new record that the
/// running loop picks up).
fn complete_record(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let callback_only = number_slot(s.get_nanbox_f64(), NATIVE_OP_KEY) > CALLBACK_ONLY;
    let len = number_slot(s.get_nanbox_f64(), NATIVE_LEN_KEY);
    let cb = scope.root_nanbox_f64(
        get_hidden_value(s.get_nanbox_f64(), hidden_key(NATIVE_CB_KEY))
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    );
    clear_record(s.get_nanbox_f64());
    if callback_only {
        if is_callable_value(cb.get_nanbox_f64()) {
            call_listener_args(s.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
        }
        return;
    }
    complete_writable_write(
        s.get_nanbox_f64(),
        len,
        cb.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
}

/// The Final record ended: end the readable side and finish, as a JS
/// Transform's flush callback does (`push(null)`, then `finish`).
fn finish_final(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let cb = scope.root_nanbox_f64(
        get_hidden_value(s.get_nanbox_f64(), hidden_key(NATIVE_CB_KEY))
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    );
    let callback_only = number_slot(s.get_nanbox_f64(), NATIVE_OP_KEY) > CALLBACK_ONLY;
    clear_record(s.get_nanbox_f64());
    if callback_only {
        if is_callable_value(cb.get_nanbox_f64()) {
            call_listener_args(s.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
        }
        return;
    }
    set_hidden_value(
        s.get_nanbox_f64(),
        hidden_transform_finishing_key(),
        f64::from_bits(TAG_FALSE),
    );
    let callback = cb.get_nanbox_f64();
    finish_stream(
        s.get_nanbox_f64(),
        is_callable_value(callback).then_some(callback),
    );
}

/// The codec failed: destroy the stream with the family's node-shaped error.
/// The pending write callback receives the error first (node's onwriteError).
fn fail_record(stream: f64, hooks: &'static StreamHooks, code: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    // SAFETY: the family's error builder; the step has returned.
    let err = scope.root_nanbox_f64(unsafe { (hooks.error)(s.get_nanbox_f64(), code) });
    // A binding may terminate its owner from its native error notification,
    // as Node's zlib onerror does. Such a notification never completes the
    // pending transform callback; the normal destroy path owns teardown.
    if stream_destroyed(s.get_nanbox_f64()) {
        clear_record(s.get_nanbox_f64());
        return;
    }
    let callback_only = number_slot(s.get_nanbox_f64(), NATIVE_OP_KEY) > CALLBACK_ONLY;
    let op = operation(number_slot(s.get_nanbox_f64(), NATIVE_OP_KEY));
    let len = number_slot(s.get_nanbox_f64(), NATIVE_LEN_KEY);
    let cb = scope.root_nanbox_f64(
        get_hidden_value(s.get_nanbox_f64(), hidden_key(NATIVE_CB_KEY))
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    );
    clear_record(s.get_nanbox_f64());
    if callback_only {
        if is_callable_value(cb.get_nanbox_f64()) {
            call_listener_args(
                s.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                &[err.get_nanbox_f64()],
            );
        }
        return;
    }
    if op == REC_FINAL {
        set_hidden_value(
            s.get_nanbox_f64(),
            hidden_transform_finishing_key(),
            f64::from_bits(TAG_FALSE),
        );
        destroy_stream(s.get_nanbox_f64(), err.get_nanbox_f64());
        if is_callable_value(cb.get_nanbox_f64()) {
            call_listener_args(
                s.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                &[err.get_nanbox_f64()],
            );
        }
        return;
    }
    complete_writable_write(
        s.get_nanbox_f64(),
        len,
        cb.get_nanbox_f64(),
        err.get_nanbox_f64(),
    );
}

/// `destroy()` of a hooked stream: release the codec now, before any event is
/// queued (`hooks.release` -> `native_payload::close`), so its native memory
/// returns at once instead of at the next sweep. Runs after `destroyed` is
/// set, so a queued step job finds the stream destroyed and returns.
pub(super) fn release_on_destroy(stream: f64) {
    #[cfg(test)]
    if stream_sabotage("release_in_finalizer_only")
        || (stream_sabotage("skip_release_autodestroy")
            && has_truthy_hidden(stream, hidden_key(b"readableEnded")))
    {
        return;
    }
    if let Some(hooks) = hooks_of(stream) {
        // SAFETY: the family's release hook, on the owner thread.
        unsafe { (hooks.release)(stream) };
    }
}

/// Stream-runtime faults for the unit sabotage children (never compiled into
/// a shipped binary).
#[cfg(test)]
pub(crate) fn stream_sabotage(fault: &str) -> bool {
    std::env::var("PERRY_TEST_STREAM_SABOTAGE").as_deref() == Ok(fault)
}

/// The chunk of the write in flight (Z9's witness reads whether it moved).
#[cfg(test)]
pub(crate) fn test_inflight_chunk(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_key(NATIVE_CHUNK_KEY)).unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

#[cfg(test)]
#[path = "native_hooks_tests.rs"]
pub(crate) mod tests;
