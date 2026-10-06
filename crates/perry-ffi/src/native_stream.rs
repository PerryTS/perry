//! Native-payload streams for binding crates (#11919, STREAM-PAYLOAD-DESIGN):
//! a codec family (zlib's `Gzip`, crypto's `Hash`) whose objects are ordinary
//! runtime Transforms/Writables, with only the codec in the payload.
//!
//! The runtime reaches the codec through the [`StreamHooks`] that the
//! payload cell's vtable names: the cell's `finalizer` word is a
//! `&'static` [`PayloadVTable`] `{ drop, stream }`, so a family declares one
//! static vtable per payload type ([`payload_vtable`]) and the runtime finds
//! the hooks from the object it already holds (`meta.native_state` -> cell
//! -> vtable), with no name lookup, table or class switch. Every write,
//! `pipe`, `pipeline` and async iteration then goes through the runtime's
//! one stream state machine: its writable buffer, its readable queue, its
//! listeners. The codec never holds a JS value or a callback address.
//!
//! The family's allocation is the payload C ABI of #11919 P0 (perry-ffi
//! `native_payload`, NET-TRANSPORT P0): its descriptor names the payload's
//! [`PayloadVTable`] where the plain-payload descriptor names a bare drop
//! thunk. After allocating the object with its family prototype and writing
//! node's pre-fields, the family calls [`init_transform_in_place`] (or
//! [`init_writable_in_place`]) in node's constructor order.
//!
//! Every type here is `#[repr(C)]` and mirrored by the runtime
//! (`perry-runtime/src/node_stream/native_hooks.rs`); [`abi_matches`] compares
//! the two layout digests, and every entry point refuses to run against a
//! runtime whose digest differs.

use std::ffi::c_void;

/// Revision of the stream ABI this file is written against.
const STREAM_ABI_VERSION: u8 = 1;

/// Which stream the family is.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamKind(pub u32);

impl StreamKind {
    /// A Transform: writes in, readable output out (zlib, Hash, Cipher).
    pub const TRANSFORM: Self = Self(0);
    /// A Writable with no readable side (`Sign`/`Verify`).
    pub const WRITABLE: Self = Self(1);
}

/// When the runtime runs the step loop for a write.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTiming(pub u32);

impl StepTiming {
    /// From an immediate, after `write()` returns (zlib: `data` never fires
    /// inside `write()`, as with node's threadpool completion).
    pub const DEFERRED: Self = Self(0);
    /// Inside `write()` (crypto: node's `_transform` calls back at once).
    pub const INLINE: Self = Self(1);
}

/// What one step is asked to do.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamOp(pub u32);

impl StreamOp {
    /// Consume `input`.
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
    /// The record is done.
    pub const NEED_INPUT: Self = Self(0);
    /// Call again for the same record.
    pub const MORE: Self = Self(1);
    /// The output is complete (`FINAL` only).
    pub const ENDED: Self = Self(2);
    /// The codec failed with `code`.
    pub const ERROR: Self = Self(3);
}

/// One step's input, borrowed from the traced chunk for this step only.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StepIn {
    /// The operation.
    pub op: StreamOp,
    /// The flush kind of a `FLUSH` op.
    pub flush_kind: i32,
    /// The unconsumed input bytes (null and 0 for `FLUSH`/`FINAL`).
    pub input: *const u8,
    /// Length of `input`.
    pub len: usize,
}

/// One step's result. `out` points into the payload's own scratch; the
/// runtime copies it into an exact-length Buffer before anything else runs.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StepOut {
    /// Input bytes consumed by this step.
    pub consumed: usize,
    /// Output bytes produced by this step (may be null when `out_len` is 0).
    pub out: *const u8,
    /// Length of `out`.
    pub out_len: usize,
    /// How the record stands.
    pub status: StepStatus,
    /// The codec's error code when `status` is `ERROR`.
    pub code: u32,
    /// Native bytes the payload retains after this step; the runtime
    /// restates the cell's external bytes with it.
    pub external_bytes: usize,
}

/// A family's codec entry points. A `static` of the binding.
#[repr(C)]
pub struct StreamHooks {
    /// Transform or Writable.
    pub kind: StreamKind,
    /// Deferred (zlib) or inline (crypto).
    pub timing: StepTiming,
    /// LazyTransform: stream state is built on first stream use.
    pub lazy: bool,
    /// Run one step: no JS, no GC allocation, nothing kept from `op.input`.
    pub step: unsafe extern "C" fn(payload: *mut c_void, op: &StepIn, out: &mut StepOut),
    /// A node-shaped Error for `code` (called after the step returned).
    pub error: unsafe extern "C" fn(owner: f64, code: u32) -> f64,
    /// Release the payload at `destroy()` (close it; zlib: `_handle = null`).
    pub release: unsafe extern "C" fn(owner: f64),
}

// SAFETY: immutable statics of function pointers and integers.
unsafe impl Sync for StreamHooks {}

/// What a payload cell's `finalizer` word names: the payload type's drop and,
/// for a stream family, its hooks. One `static` per payload type.
#[repr(C)]
pub struct PayloadVTable {
    /// Frees one boxed payload (release, sweep or thread teardown).
    pub drop: unsafe extern "C" fn(resource: *mut c_void, hint: *mut c_void),
    /// The family's stream hooks (`None` for a family that is not a stream).
    pub stream: Option<&'static StreamHooks>,
}

// SAFETY: as `StreamHooks`.
unsafe impl Sync for PayloadVTable {}

unsafe extern "C" fn drop_box<T>(resource: *mut c_void, _hint: *mut c_void) {
    drop(Box::from_raw(resource as *mut T));
}

/// The vtable of a payload type `T`, for a `static`:
/// `static GZIP_VTABLE: PayloadVTable = payload_vtable::<Gzip>(Some(&GZIP_HOOKS));`
pub const fn payload_vtable<T: 'static>(stream: Option<&'static StreamHooks>) -> PayloadVTable {
    PayloadVTable {
        drop: drop_box::<T>,
        stream,
    }
}

/// Digest of this crate's copies of the stream ABI types; the runtime
/// computes the same expression over its own (`js_perry_stream_abi_layout`).
pub const fn layout_digest() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<StreamHooks>() as u64) << 56
        | (offset_of!(StreamHooks, step) as u64) << 48
        | (size_of::<StepIn>() as u64) << 40
        | (size_of::<StepOut>() as u64) << 32
        | (offset_of!(StepOut, status) as u64) << 24
        | (offset_of!(StepOut, external_bytes) as u64) << 16
        | (size_of::<PayloadVTable>() as u64) << 8
        | STREAM_ABI_VERSION as u64
}

#[cfg(any(not(test), feature = "runtime-link"))]
extern "C" {
    fn js_perry_stream_abi_layout() -> u64;
    fn js_perry_stream_init_transform(value: f64, opts: f64) -> i32;
    fn js_perry_stream_init_writable(value: f64, opts: f64) -> i32;
    fn js_perry_stream_flush(value: f64, kind: i32, callback: f64) -> i32;
    fn js_perry_stream_queue_immediate(closure: f64) -> i32;
}

// Unit tests of this crate link no runtime: the entry points answer "absent".
#[cfg(all(test, not(feature = "runtime-link")))]
unsafe fn js_perry_stream_abi_layout() -> u64 {
    0
}
#[cfg(all(test, not(feature = "runtime-link")))]
unsafe fn js_perry_stream_init_transform(_value: f64, _opts: f64) -> i32 {
    -1
}
#[cfg(all(test, not(feature = "runtime-link")))]
unsafe fn js_perry_stream_init_writable(_value: f64, _opts: f64) -> i32 {
    -1
}
#[cfg(all(test, not(feature = "runtime-link")))]
unsafe fn js_perry_stream_flush(_value: f64, _kind: i32, _callback: f64) -> i32 {
    -1
}
#[cfg(all(test, not(feature = "runtime-link")))]
unsafe fn js_perry_stream_queue_immediate(_closure: f64) -> i32 {
    -1
}

/// Does the linked runtime use this crate's stream ABI layout?
pub fn abi_matches() -> bool {
    // SAFETY: a pure query of the runtime.
    unsafe { js_perry_stream_abi_layout() == layout_digest() }
}

/// node's `Transform` constructor body on `stream`, an object the family
/// allocated with its own prototype and payload. Returns false (and does
/// nothing) against a runtime with another stream ABI or a non-object.
pub fn init_transform_in_place(stream: f64, opts: f64) -> bool {
    // SAFETY: the runtime validates `stream`.
    abi_matches() && unsafe { js_perry_stream_init_transform(stream, opts) } == 0
}

/// node's `Writable` constructor body (a `Sign`/`Verify` family).
pub fn init_writable_in_place(stream: f64, opts: f64) -> bool {
    // SAFETY: as `init_transform_in_place`.
    abi_matches() && unsafe { js_perry_stream_init_writable(stream, opts) } == 0
}

/// `.flush(kind, cb)`: a `FLUSH` record behind the buffered writes.
pub fn request_flush(stream: f64, kind: i32, callback: f64) -> bool {
    // SAFETY: as `init_transform_in_place`.
    abi_matches() && unsafe { js_perry_stream_flush(stream, kind, callback) } == 0
}

/// Run a runtime closure (a JS function value) from the immediate queue: a
/// one-shot codec job (`gzip(buf, cb)`) holding its inputs as traced captures.
pub fn queue_immediate(closure: f64) -> bool {
    // SAFETY: the runtime validates `closure`.
    unsafe { js_perry_stream_queue_immediate(closure) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_names_the_revision_and_layout() {
        let digest = layout_digest();
        assert_eq!(digest & 0xFF, STREAM_ABI_VERSION as u64);
        assert_eq!(
            (digest >> 8) & 0xFF,
            std::mem::size_of::<PayloadVTable>() as u64
        );
        // `Option<&'static StreamHooks>` is a nullable pointer.
        assert_eq!(
            std::mem::size_of::<PayloadVTable>(),
            2 * std::mem::size_of::<usize>()
        );
    }
}
