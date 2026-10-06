//! Multi-threading primitives for Perry
//!
//! Provides two core primitives for TypeScript programs:
//!
//! 1. **`parallelMap`** — Data-parallel processing across CPU cores.
//!    Splits an array into chunks, processes each chunk on a separate OS thread,
//!    and joins the results. Blocks until all threads complete.
//!
//! 2. **`spawn`** — Background thread execution.
//!    Runs a closure on a new OS thread and returns a Promise that resolves
//!    when the work completes. The calling thread continues immediately.
//!
//! # TypeScript API
//!
//! ```typescript
//! import { parallelMap, spawn } from "perry/thread";
//!
//! // ── Example 1: Parallel computation ──────────────────────────────
//! // Process a large dataset across all CPU cores.
//! // Each element is processed independently — perfect for CPU-bound work.
//!
//! const prices = [100, 200, 300, 400, 500, 600, 700, 800];
//! const adjusted = parallelMap(prices, (price) => {
//!     // This runs on a worker thread — heavy math is fine here
//!     let result = price;
//!     for (let i = 0; i < 1000000; i++) {
//!         result = Math.sqrt(result * result + i);
//!     }
//!     return result;
//! });
//! console.log(adjusted); // [computed results across all cores]
//!
//!
//! // ── Example 2: Background thread ─────────────────────────────────
//! // Run expensive work without blocking the main thread.
//! // Great for keeping UI responsive while computing.
//!
//! const handle = spawn(() => {
//!     // This entire block runs on a separate OS thread
//!     let sum = 0;
//!     for (let i = 0; i < 100_000_000; i++) {
//!         sum += Math.sin(i);
//!     }
//!     return sum;
//! });
//!
//! // Main thread continues immediately — UI stays responsive
//! console.log("Computing in background...");
//!
//! // Await the result when you need it
//! const result = await handle;
//! console.log("Result:", result);
//!
//!
//! // ── Example 3: Parallel with captured values ─────────────────────
//! // Closures can capture outer variables (read-only).
//! // Captured values are deep-copied to each worker thread automatically.
//!
//! const multiplier = 2.5;
//! const data = [10, 20, 30, 40];
//! const scaled = parallelMap(data, (x) => x * multiplier);
//! // scaled = [25, 50, 75, 100]
//!
//!
//! // ── Example 4: Parallel string processing ────────────────────────
//! // Strings, arrays, and objects are deep-copied across threads.
//!
//! const names = ["alice", "bob", "charlie"];
//! const upper = parallelMap(names, (name) => {
//!     return name.toUpperCase();
//! });
//! // upper = ["ALICE", "BOB", "CHARLIE"]
//!
//!
//! // ── Example 5: Multiple background tasks ─────────────────────────
//! // Spawn multiple independent computations in parallel.
//!
//! const task1 = spawn(() => computeHash(data1));
//! const task2 = spawn(() => computeHash(data2));
//! const task3 = spawn(() => computeHash(data3));
//!
//! // All three run concurrently on separate OS threads
//! const [hash1, hash2, hash3] = await Promise.all([task1, task2, task3]);
//!
//!
//! // ── Example 6: Background with object result ─────────────────────
//! // Spawned functions can return objects — they're serialized back.
//!
//! const stats = await spawn(() => {
//!     const values = computeExpensiveValues();
//!     return { mean: avg(values), median: mid(values), count: values.length };
//! });
//! console.log(stats.mean, stats.median);
//! ```
//!
//! # Safety Model
//!
//! - **No shared mutable state**: Closures passed to `parallelMap` and `spawn`
//!   cannot capture mutable variables. The Perry compiler rejects this at
//!   compile time with a clear error message.
//!
//! - **Deep copy across boundaries**: All values crossing thread boundaries
//!   (captures and return values) are serialized and deserialized. Numbers and
//!   booleans are zero-cost (just 64-bit copies). Strings, arrays, and objects
//!   are deep-copied.
//!
//! - **Independent arenas**: Each worker thread gets its own thread-local arena
//!   and GC. No synchronization overhead during computation. Arenas are freed
//!   when the thread exits.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Main Thread                                                │
//! │                                                             │
//! │  1. Read input array from main arena                        │
//! │  2. Serialize elements → Vec<SerializedValue> (Rust heap)   │
//! │  3. Serialize closure captures → Vec<SerializedValue>       │
//! │                                                             │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
//! │  │ Thread 1 │  │ Thread 2 │  │ Thread 3 │  │ Thread N │   │
//! │  │          │  │          │  │          │  │          │   │
//! │  │ deserial.│  │ deserial.│  │ deserial.│  │ deserial.│   │
//! │  │ ize into │  │ ize into │  │ ize into │  │ ize into │   │
//! │  │ local    │  │ local    │  │ local    │  │ local    │   │
//! │  │ arena    │  │ arena    │  │ arena    │  │ arena    │   │
//! │  │          │  │          │  │          │  │          │   │
//! │  │ run fn() │  │ run fn() │  │ run fn() │  │ run fn() │   │
//! │  │          │  │          │  │          │  │          │   │
//! │  │ serial.  │  │ serial.  │  │ serial.  │  │ serial.  │   │
//! │  │ results  │  │ results  │  │ results  │  │ results  │   │
//! │  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
//! │       └──────────────┴──────────────┴──────────────┘        │
//! │                         join                                │
//! │  4. Deserialize all results into main arena                 │
//! │  5. Return new array                                        │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::bigint::BIGINT_LIMBS;
use crate::closure::{self, real_capture_count, ClosureHeader};
use crate::gc;
#[cfg(test)]
use crate::value::JSValue;

// NaN-boxing tag constants (from value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;

// ============================================================================
// Truthiness check (mirrors js_is_truthy in value.rs)
// ============================================================================

/// Check if NaN-boxed bits represent a truthy value (JS semantics).
#[inline]
fn is_truthy_bits(bits: u64) -> bool {
    // Falsy: undefined, null, false, 0, -0, NaN, empty string
    if bits == TAG_UNDEFINED || bits == TAG_NULL || bits == TAG_FALSE {
        return false;
    }
    if bits == TAG_TRUE {
        return true;
    }
    // INT32_TAG: check if value is 0
    if (bits & TAG_MASK) == INT32_TAG {
        return (bits & INT32_MASK) != 0;
    }
    // String: empty string is falsy (check length == 0)
    if (bits & TAG_MASK) == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            return false;
        }
        return unsafe { (*ptr).byte_len > 0 };
    }
    // Inline SSO string (#11519): truthy iff non-empty. Its bits are a NaN
    // pattern, so the f64 fallthrough below would call every one falsy.
    if crate::value::JSValue::from_bits(bits).is_short_string() {
        return crate::value::JSValue::from_bits(bits).short_string_len() > 0;
    }
    // Pointer (object/array/closure): always truthy
    if (bits & TAG_MASK) == POINTER_TAG || (bits & TAG_MASK) == BIGINT_TAG {
        return true;
    }
    // Regular f64: 0.0, -0.0, NaN are falsy
    let f = f64::from_bits(bits);
    f != 0.0 && !f.is_nan()
}

// ============================================================================
// SerializedValue — thread-safe representation of a JSValue
// ============================================================================

/// A thread-safe, arena-independent representation of a JavaScript value.
///
/// JSValues in Perry use NaN-boxing with pointers into thread-local arenas.
/// These pointers are only valid on the thread that allocated them. To safely
/// move values between threads, we serialize them into this enum (which lives
/// on the Rust heap and is `Send`), then deserialize on the target thread
/// using that thread's arena.
///
/// # Zero-copy cases
///
/// Numbers, booleans, null, undefined, and int32 are stored as raw `u64` bits.
/// No heap allocation or copying is needed — they're just bit patterns.
///
/// # Deep-copy cases
///
/// Strings, arrays, objects, closures, and BigInts contain pointers to arena
/// or malloc memory. These are read from the source thread's memory and stored
/// as owned Rust data (`Vec<u8>`, `Vec<SerializedValue>`, etc.).
#[derive(Debug, Clone)]
pub enum SerializedValue {
    /// A raw 64-bit value that needs no pointer fixup.
    /// Covers: f64 numbers, TAG_UNDEFINED, TAG_NULL, TAG_TRUE, TAG_FALSE, INT32_TAG.
    Inline(u64),

    /// A UTF-8 string (copied from StringHeader + trailing bytes).
    String(Vec<u8>),

    /// An array of serialized elements.
    Array(Vec<SerializedValue>),

    /// An object: (class_id, parent_class_id, fields, optional keys).
    /// Keys are present only for plain objects (not class instances).
    Object {
        /// Optional immutable body/rep facts; never a source ShapeId or closure.
        final_constfn: Option<constfn_transfer::ConstFnTransferFacts>,
        class_id: u32,
        parent_class_id: u32,
        fields: Vec<SerializedValue>,
        /// Key names for each field (for Object.keys() support).
        /// None for class instances where keys are defined by the class.
        keys: Option<Vec<Vec<u8>>>,
    },

    /// A closure: its body's static `JsFunctionInfo` (process-global, safe
    /// to share: the receiving thread sees every fact about the body) +
    /// serialized captures.
    Closure {
        info: usize,
        capture_count: u32, // includes CAPTURES_THIS_FLAG
        captures: Vec<SerializedValue>,
    },

    /// A closure capture slot that, on the source thread, held a pointer to a
    /// mutable `Box` rather than a NaN-boxed value — the shape codegen produces
    /// for every `async`-fn body local (boxed by the async-to-generator
    /// transform) and every mutable capture. The box itself is thread-local and
    /// never crosses; this carries a deep copy of the value it held, and
    /// deserialization re-boxes it in the receiving thread's registry so the
    /// reconstructed closure's `js_box_get`/`js_box_set` slot reads work again
    /// (#6520). Only ever appears in a capture position; the inner value is any
    /// ordinary transferable `SerializedValue`.
    BoxedCapture(Box<SerializedValue>),

    /// A capture slot holding a scope context object (`box/scope.rs`): the
    /// deep-copied words of every slot, rebuilt as a fresh scope object on
    /// the receiving thread. Like `BoxedCapture`, only in capture position.
    ScopeCapture(Vec<SerializedValue>),

    /// A BigInt: 16 x u64 limbs in little-endian order.
    BigInt([u64; BIGINT_LIMBS]),

    /// A Date: its millisecond timestamp (may be NaN for an Invalid Date).
    /// Re-allocated as a fresh `DateCell` on the receiving thread (#2089) —
    /// deep-copy semantics, since the source cell's pointer is meaningless in
    /// another thread's arena.
    Date(f64),

    /// A constructor-created `Uint8Array`, copied byte-for-byte into fresh
    /// buffer storage on the receiving thread. Perry represents these with a
    /// `BufferHeader` plus an address-keyed brand, so the brand must be
    /// restored as well as the bytes (#10103).
    Uint8Array(Vec<u8>),

    /// An `fs.promises.FileHandle` crossing a `perry/thread` boundary.
    /// Perry's fd registry is thread-local, so handles are not transferable;
    /// deserialize as a FileHandle-shaped object with `fd === -1`.
    /// Recognised/built via [`FsThreadCodec`] so the fs surface is linked
    /// only when a FileHandle can actually exist.
    DetachedFileHandle,

    /// An array slot that holds no value (`[1, , 3]`). Only appears as an
    /// element of `Array`.
    Hole,

    /// The same object as the n-th object already written in this value, in
    /// the order the reader meets them. This keeps shared references and
    /// cycles (`a.self = a`) as they were.
    Ref(u32),

    /// An untransferred `ArrayBuffer`: a copy of all its bytes.
    ArrayBuffer(Vec<u8>),

    /// Exclusive native backing, committed before the message is published.
    TransferredArrayBuffer(crate::buffer::TransferredBacking),

    /// A typed array or `DataView` over `buffer` (an `ArrayBuffer`, a
    /// `SharedArrayBuffer` or a `Ref` to one). `kind` is a typed array
    /// `KIND_*` or [`VIEW_KIND_DATA_VIEW`]; `length` counts elements (bytes
    /// for a DataView). A Node `Buffer` crosses as a `Uint8Array`, as in Node.
    View {
        kind: u8,
        buffer: Box<SerializedValue>,
        byte_offset: u32,
        length: u32,
    },

    /// A `Map`'s entries in insertion order.
    Map(Vec<(SerializedValue, SerializedValue)>),

    /// A `Set`'s values in insertion order.
    Set(Vec<SerializedValue>),

    /// An Error. `name` picks the constructor (an unknown name becomes
    /// `Error`, as in V8). Other own properties, such as `code`, do not cross.
    Error {
        name: Vec<u8>,
        message: Option<Vec<u8>>,
        stack: Option<Vec<u8>>,
        cause: Option<Box<SerializedValue>>,
    },

    /// A RegExp: its source pattern and flags. `lastIndex` starts at 0.
    RegExp { source: Vec<u8>, flags: Vec<u8> },

    /// A `SharedArrayBuffer` crossing a `perry/thread` boundary (#4913).
    /// Carries the process-global backing-store address by reference — NOT a
    /// byte copy — so the receiving agent's views alias the same physical
    /// memory and `Atomics.wait`/`notify` coordinate across threads. The
    /// backing is never freed (see `crate::shared_sab`), so the raw address
    /// stays valid for the life of the process.
    SharedArrayBuffer { addr: usize },

    /// A value whose runtime type cannot cross a thread boundary (Promise,
    /// Symbol, Temporal, native handles, a detached ArrayBuffer; for a
    /// message also a function or an object marked uncloneable).
    ///
    /// The serializer used to lower every one of these to `Inline(TAG_UNDEFINED)`,
    /// so a capture/return of such a value crossed silently as `undefined`
    /// with no diagnostic (2026-07-09 GC audit §6 / #6185). Instead we now
    /// carry the human-readable type name here and raise a catchable
    /// `TypeError` at the transfer boundary **on the main thread** — the
    /// capture path throws synchronously from `spawn`/`parallelMap`, and the
    /// `spawn` return path rejects the returned promise. This value is never
    /// deserialized; its presence anywhere in a serialized tree is a hard error.
    Unsupported(&'static str),
}

/// [`SerializedValue::View`] kind for a `DataView` (typed arrays use their
/// `KIND_*`).
pub const VIEW_KIND_DATA_VIEW: u8 = 0xFF;

// Safety: published SerializedValue contains no raw pointers to arena memory.
// Private pending transfer sources are removed before serialize_message returns.
// `info` in Closure points to a body's static JsFunctionInfo, which is
// process-global and immutable.
unsafe impl Send for SerializedValue {}
unsafe impl Sync for SerializedValue {}

// ============================================================================
// Serialization: JSValue (NaN-boxed, arena pointers) → SerializedValue
// ============================================================================
//
// The writer is `thread/clone_write.rs`, the reader `thread/clone_read.rs`.

mod clone_read;
mod clone_write;
pub use clone_read::deserialize_nanbox_on_current_thread;
use clone_write::serialize_capture_for_thread;
pub use clone_write::{serialize_message, serialize_nanbox_for_thread};

/// Cross-thread codec hook for `fs.promises` FileHandle values (binary
/// size). The serializer's FileHandle probe and the deserializer's
/// detached-handle builder live in `crate::fs`; referencing them statically
/// from this always-linked codec pinned the whole fs surface into every
/// binary (the microtask pump drains diagnostics publishes through the
/// codec, so it is reachable from `main` unconditionally). `crate::fs` arms
/// the hook at the top of `build_filehandle_object` — before the first
/// FileHandle object can exist — so a program that never creates one links
/// none of it and can never observe the difference: with no FileHandle in
/// the process, the probe cannot match and the variant is never produced.
pub(crate) struct FsThreadCodec {
    /// `is_fs_filehandle_value(v) || filehandle_object_fd(v).is_some()`.
    pub is_filehandle: fn(f64) -> bool,
    /// `build_detached_filehandle_object` (FileHandle shape, `fd === -1`).
    pub build_detached: fn() -> f64,
}

static FS_THREAD_CODEC: AtomicPtr<FsThreadCodec> = AtomicPtr::new(ptr::null_mut());

pub(crate) fn arm_fs_thread_codec(codec: &'static FsThreadCodec) {
    // `black_box` for the same reason as `NM_INSTALL_ALL_HOOK`: a
    // single-store AtomicPtr gets speculatively devirtualized by
    // whole-program optimization (only one value is ever stored, so the
    // compiler proves it and re-materializes the direct reference —
    // re-pinning everything this hook exists to unpin).
    FS_THREAD_CODEC.store(
        std::hint::black_box(codec as *const FsThreadCodec as *mut FsThreadCodec),
        Ordering::Release,
    );
}

fn fs_thread_codec() -> Option<&'static FsThreadCodec> {
    let p = FS_THREAD_CODEC.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: only ever stores `&'static FsThreadCodec`.
        Some(unsafe { &*p })
    }
}

/// Human-readable name for a GC object type that cannot cross a thread
/// boundary. Used only to build the TypeError message (#6185).
///
/// Note: a Symbol is POINTER_TAG'd but allocated with `GC_TYPE_STRING`
/// (real strings arrive under `STRING_TAG` and never reach this match), so
/// `GC_TYPE_STRING` here means "Symbol".
fn unsupported_transfer_type_name(obj_type: u8) -> &'static str {
    match obj_type {
        gc::GC_TYPE_STRING => "Symbol",
        gc::GC_TYPE_PROMISE => "Promise",
        gc::GC_TYPE_BIGINT => "BigInt",
        gc::GC_TYPE_ERROR => "Error",
        gc::GC_TYPE_MAP => "Map",
        gc::GC_TYPE_LAZY_ARRAY => "lazy (unmaterialized) JSON array",
        t if gc::is_typed_array_type(t) => "TypedArray",
        t if gc::is_buffer_family_type(t) => "Buffer",
        gc::GC_TYPE_SET => "Set",
        gc::GC_TYPE_NATIVE_ARENA_OWNER
        | gc::GC_TYPE_NATIVE_TYPED_VIEW
        | gc::GC_TYPE_NATIVE_HANDLE
        | gc::GC_TYPE_NATIVE_POD_VIEW => "native handle",
        gc::GC_TYPE_TEMPORAL => "Temporal value",
        _ => "value of an unsupported type",
    }
}

/// Depth-first search for the first non-transferable value anywhere in a
/// serialized tree (a captured/returned Map, an object field holding a Set,
/// an array element that is a Promise, …). Returns its type name, or `None`
/// if the whole tree is transferable.
pub(crate) fn first_unsupported_transfer_type(sv: &SerializedValue) -> Option<&'static str> {
    match sv {
        SerializedValue::Unsupported(name) => Some(name),
        SerializedValue::Array(elements) => {
            elements.iter().find_map(first_unsupported_transfer_type)
        }
        SerializedValue::Object { fields, .. } => {
            fields.iter().find_map(first_unsupported_transfer_type)
        }
        SerializedValue::Closure { captures, .. } => {
            captures.iter().find_map(first_unsupported_transfer_type)
        }
        SerializedValue::BoxedCapture(inner) => first_unsupported_transfer_type(inner),
        SerializedValue::ScopeCapture(slots) => {
            slots.iter().find_map(first_unsupported_transfer_type)
        }
        SerializedValue::Map(entries) => entries.iter().find_map(|(key, value)| {
            first_unsupported_transfer_type(key).or_else(|| first_unsupported_transfer_type(value))
        }),
        SerializedValue::Set(values) => values.iter().find_map(first_unsupported_transfer_type),
        SerializedValue::Error { cause, .. } => {
            cause.as_deref().and_then(first_unsupported_transfer_type)
        }
        SerializedValue::View { buffer, .. } => first_unsupported_transfer_type(buffer),
        _ => None,
    }
}

/// Build (but do not throw) a `TypeError` value naming an unsupported
/// cross-thread transfer. Used by the `spawn` return path, which *rejects*
/// the returned promise rather than throwing.
///
/// # Safety
/// Must run on the thread whose arena should own the error object (the main
/// thread, at the drain boundary).
unsafe fn make_unsupported_transfer_error(type_name: &str) -> f64 {
    let msg =
        format!("Cannot transfer a {type_name} across a perry/thread boundary (unsupported type)");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::value::js_nanbox_pointer(err as i64)
}

/// Throw a catchable `TypeError` naming an unsupported cross-thread transfer.
///
/// # Safety
/// Must be called on the **main / calling thread** (never a worker): it
/// `longjmp`s to the nearest active `setjmp` frame, which only the calling
/// JS thread has established. Worker threads have no such frame, so a throw
/// there would be undefined behavior — worker-side failures are surfaced by
/// rejecting the returned promise on the main thread instead.
unsafe fn throw_unsupported_transfer(type_name: &str) -> ! {
    crate::exception::js_throw(make_unsupported_transfer_error(type_name));
}

/// Main-thread guard: if any value in `values` is non-transferable, throw a
/// named `TypeError`. Call this at a `spawn`/`parallelMap`/`parallelFilter`
/// serialization boundary, on the calling thread, before spawning any worker.
///
/// # Safety
/// Same as [`throw_unsupported_transfer`] — main/calling thread only.
unsafe fn guard_transferable(values: &[SerializedValue]) {
    if let Some(name) = values.iter().find_map(first_unsupported_transfer_type) {
        throw_unsupported_transfer(name);
    }
}

// ============================================================================
// Deserialization: SerializedValue → JSValue (into current thread's arena)
// ============================================================================

#[inline]
unsafe fn store_thread_array_slot(arr: *mut crate::array::ArrayHeader, index: usize, bits: u64) {
    crate::array::store_array_slot(arr, index, bits);
    (*arr).length = (index + 1) as u32;
}

#[inline]
unsafe fn store_thread_object_field(
    obj: *mut crate::object::ObjectHeader,
    index: usize,
    bits: u64,
) {
    crate::object::store_object_field_slot(obj, index, bits);
}

#[cfg(test)]
pub(crate) unsafe fn test_store_thread_array_slot(
    arr: *mut crate::array::ArrayHeader,
    index: usize,
    bits: u64,
) {
    store_thread_array_slot(arr, index, bits);
}

#[cfg(test)]
pub(crate) unsafe fn test_store_thread_object_field(
    obj: *mut crate::object::ObjectHeader,
    index: usize,
    bits: u64,
) {
    store_thread_object_field(obj, index, bits);
}

#[cfg(test)]
pub(crate) unsafe fn test_deserialize_bigint_limbs(limbs: [u64; BIGINT_LIMBS]) -> u64 {
    deserialize_nanbox_on_current_thread(&SerializedValue::BigInt(limbs))
}

/// Run image-local, string-only initialization on the current worker. This
/// callback has no user code and no heap captures; earlier strings are already
/// registered as roots before the next allocation in preparation can collect.
unsafe fn prepare_worker_literals(code: i64) {
    if code != 0 {
        let prepare: unsafe extern "C" fn() = std::mem::transmute(code as usize);
        prepare();
    }
}

// ============================================================================
// parallelMap — data-parallel array processing
// ============================================================================

/// The compiled closure function signature: (closure_header, argument) -> result.
/// This matches Perry's closure calling convention where the first parameter
/// is a pointer to the ClosureHeader (for accessing captures) and the second
/// is the f64 argument.
type ClosureCallFn = crate::closure::body_call::js_body_fn_ty!(argument);

/// Process an array in parallel across multiple OS threads.
///
/// # Arguments
/// - `array_ptr`: Raw pointer to an ArrayHeader (NaN-boxed with POINTER_TAG by caller)
/// - `func_ptr`: Pointer to the compiled mapping function
/// - `closure_ptr`: Pointer to ClosureHeader with captured values (0 if no captures)
/// - `chunk_count`: Number of threads to use (0 = auto-detect from CPU count)
///
/// # Returns
/// Raw pointer to a new ArrayHeader containing the mapped results (in main thread's arena).
///
/// # How it works
///
/// ```text
/// Input: [a, b, c, d, e, f, g, h]  (8 elements, 4 cores)
///
///   Thread 1: [a, b] → serialize → deserialize → map → serialize results
///   Thread 2: [c, d] → serialize → deserialize → map → serialize results
///   Thread 3: [e, f] → serialize → deserialize → map → serialize results
///   Thread 4: [g, h] → serialize → deserialize → map → serialize results
///
/// Join: deserialize all results into main thread's arena → [a', b', c', d', e', f', g', h']
/// ```
/// FFI entry point for `parallelMap(array, closure)`.
///
/// Both arguments are NaN-boxed f64 values as produced by the compiler:
/// - `array_val`: POINTER_TAG'd ArrayHeader pointer
/// - `closure_val`: POINTER_TAG'd ClosureHeader pointer (its body info + captures)
///
/// Returns a POINTER_TAG'd ArrayHeader pointer to the result array.
#[no_mangle]
pub extern "C" fn js_thread_parallel_map(array_val: f64, closure_val: f64) -> f64 {
    let result_ptr = unsafe { parallel_map_impl(array_val, closure_val, 0) };
    // NaN-box the result array pointer with POINTER_TAG
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

/// Compiler-only launch ABI: preparation is a code address in the spawning image.
#[no_mangle]
pub extern "C" fn js_thread_parallel_map_with_literals(
    array_val: f64,
    closure_val: f64,
    literal_prepare: i64,
) -> f64 {
    let result_ptr = unsafe { parallel_map_impl(array_val, closure_val, literal_prepare) };
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

unsafe fn parallel_map_impl(array_val: f64, closure_val: f64, literal_prepare: i64) -> i64 {
    // ── 1. Extract closure pointer and code, and root the closure ─
    // The closure is validated and rooted BEFORE `clean_arr_ptr`: resolving
    // the array can force-materialize a lazy array — a GC point — and a
    // moving minor there would strand a raw closure pointer held in an
    // unrooted local (#6521 review follow-up).
    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    if closure.is_null() || (closure as usize) < 0x1000 {
        // No valid closure — can't call anything
        return crate::array::js_array_alloc(0) as i64;
    }
    let func = (*closure).code();
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure as *mut ClosureHeader);

    // ── 1b. Extract array pointer from NaN-boxed value ───────────────
    let array_bits = array_val.to_bits();
    let arr = (array_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    // #6518: follow a push-grown array's forwarding stub (#233, the #6486
    // family) before reading length — `parallelMap` on a caller's stale
    // pre-grow pointer read the forwarding pointer's bytes as the element
    // count. `clean_arr_ptr` also validates the header and materializes
    // lazy arrays.
    let arr = crate::array::clean_arr_ptr(arr);
    if arr.is_null() {
        return crate::array::js_array_alloc(0) as i64;
    }

    let len = (*arr).length as usize;
    if len == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }

    // Re-derive the (possibly moved) closure now that the GC points above
    // are behind us; no further GC points before the derefs below.
    let closure = closure_handle.get_raw_const_ptr::<ClosureHeader>();
    let closure_ptr_raw = closure as i64;

    // ── 2. Determine thread count ────────────────────────────────────
    #[cfg(not(target_os = "wasi"))]
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // WASI preview 2 is single-threaded (#11377): always take the sequential
    // path below (`available_parallelism` errors there, so the `4` fallback
    // would otherwise try to spawn).
    #[cfg(target_os = "wasi")]
    let num_threads = 1;
    // Don't spawn more threads than elements
    let num_threads = num_threads.min(len);

    // ── 3. Fast path: single thread (small arrays) ───────────────────
    if num_threads <= 1 {
        return single_thread_map(arr, len, func, closure_ptr_raw);
    }

    // ── 4. Serialize all input elements ──────────────────────────────
    // Per-element via `js_array_get_f64`: a sparse array (length > capacity)
    // legally passes `clean_arr_ptr`, so a raw walk over `length` slots
    // reads out of bounds (same rule as in `serialize_array`).
    let mut serialized_elements = Vec::with_capacity(len);
    for i in 0..len {
        let bits = crate::array::js_array_get_f64(arr, i as u32).to_bits();
        serialized_elements.push(serialize_nanbox_for_thread(bits));
    }
    // #6185: a non-transferable element (e.g. a Map in the input array) would
    // otherwise cross as `undefined`. Fail loudly on the calling thread.
    guard_transferable(&serialized_elements);

    // ── 5. Serialize closure captures (shared across all threads) ────
    let serialized_captures: Option<(usize, u32, Vec<SerializedValue>)> = {
        if !closure.is_null() && (closure as usize) >= 0x1000 {
            let fp = (*closure).info as usize;
            let cc = (*closure).capture_count;
            let actual = real_capture_count(cc) as usize;
            let base =
                (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
            let mut caps = Vec::with_capacity(actual);
            for i in 0..actual {
                caps.push(serialize_capture_for_thread((*base.add(i)).to_bits()));
            }
            guard_transferable(&caps); // #6185: named throw for a captured Map/Set/…
            Some((fp, cc, caps))
        } else {
            None
        }
    };

    // ── 6. Split into chunks and process in parallel ─────────────────
    let chunk_size = len.div_ceil(num_threads);

    // Use a Vec of chunks that we can pass to scoped threads
    let mut chunks: Vec<Vec<SerializedValue>> = Vec::with_capacity(num_threads);
    let mut remaining = serialized_elements;
    for _ in 0..num_threads {
        if remaining.is_empty() {
            break;
        }
        let split_at = chunk_size.min(remaining.len());
        let rest = remaining.split_off(split_at);
        chunks.push(remaining);
        remaining = rest;
    }
    if !remaining.is_empty() {
        if let Some(last) = chunks.last_mut() {
            last.extend(remaining);
        }
    }

    // Wrap captures in Arc for sharing across threads
    let captures_arc = serialized_captures.map(std::sync::Arc::new);
    let func_usize = func as usize;

    // Scoped threads: all threads must complete before we return.
    // This guarantees no dangling references.
    let mut all_results: Vec<Vec<SerializedValue>> =
        (0..chunks.len()).map(|_| Vec::new()).collect();

    // #8546: workers never run module init; they dispatch through the
    // spawning image's class tables.
    let class_image = crate::object::class_image::current_image_handle();
    // Same image, so the same module initializers: a runtime `require` of a
    // Deferred module must find its initializer on the worker too.
    let path_inits = crate::module_require::current_path_init_image();
    // Charter step 5, P4: the worker installs the spawner's codegen ShapeIds
    // (with their reps) before any allocation; see `shapes_worker_seed`.
    let shape_seed = crate::object::shapes::worker_shape_seed();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let captures_ref = captures_arc.clone();
            let class_image = class_image.clone();
            let path_inits = path_inits.clone();
            let shape_seed = shape_seed.clone();

            let handle = scope.spawn(move || {
                crate::object::class_image::adopt_image(class_image);
                crate::module_require::adopt_path_init_image(path_inits);
                // #6185: own agent id before any allocation or enqueue, so this
                // worker's drains can't touch the spawner's queued work (and
                // anything it queues is tagged as its own).
                let worker_agent = crate::agent::enter_worker_agent();
                // Each thread has its own arena (via thread_local!).
                // Register this thread's root scanners BEFORE any allocation
                // can cross a GC trigger — a fresh worker otherwise collects
                // with an empty scanner registry (and an empty shadow stack)
                // and sweeps everything it just deserialized.
                crate::gc::ensure_gc_initialized();
                crate::object::shapes::install_worker_shape_seed(&shape_seed);
                unsafe { prepare_worker_literals(literal_prepare) };
                let mut results = Vec::with_capacity(chunk.len());

                // Reconstruct closure on this thread's arena, rooted for the
                // whole loop: per-element deserialization below can allocate
                // and trigger a worker GC, and a bare local is not a root.
                let gc_scope = crate::gc::RuntimeHandleScope::new();
                let closure_handle = if let Some(ref caps) = captures_ref {
                    let (fp, cc, ref cap_vals) = **caps;
                    let c =
                        closure::js_closure_alloc(fp as *const crate::closure::JsFunctionInfo, cc);
                    let h = gc_scope.root_raw_mut_ptr(c);
                    for (i, cap) in cap_vals.iter().enumerate() {
                        let bits = deserialize_nanbox_on_current_thread(cap);
                        crate::closure::js_closure_set_capture_f64(
                            h.get_raw_mut_ptr::<ClosureHeader>(),
                            i as u32,
                            f64::from_bits(bits),
                        );
                    }
                    Some(h)
                } else {
                    None
                };

                let call_fn: ClosureCallFn =
                    crate::closure::body_call::js_body_fn!(func_usize as *const u8; argument);

                for elem_sv in &chunk {
                    let arg = f64::from_bits(deserialize_nanbox_on_current_thread(elem_sv));
                    let local_closure = closure_handle
                        .as_ref()
                        .map(|h| h.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader)
                        .unwrap_or(ptr::null());
                    let result = call_fn(local_closure, crate::closure::plain_call_receiver(), arg);
                    results.push(serialize_nanbox_for_thread(result.to_bits()));
                }

                // #6185: results are already serialized into agent-independent
                // form; this arena is about to go away with the scope, so purge
                // anything this worker left in a global queue.
                drop(gc_scope);
                crate::agent::retire_agent(worker_agent);
                (idx, results)
            });
            handles.push(handle);
        }

        // Collect results in order
        for handle in handles {
            if let Ok((idx, results)) = handle.join() {
                all_results[idx] = results;
            }
        }
    });

    // ── 7. Deserialize results into main thread's arena ──────────────
    // #6185: a mapper that returns a non-transferable value (e.g. a Map) is a
    // loud TypeError on the calling thread, not a silent `undefined`. The
    // worker never throws (no setjmp frame there); the marker rode back here.
    for chunk_results in &all_results {
        guard_transferable(chunk_results);
    }
    let total_results: usize = all_results.iter().map(|r| r.len()).sum();
    let result_arr = crate::array::js_array_alloc(total_results as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let mut write_idx = 0;
    for chunk_results in &all_results {
        for sv in chunk_results {
            let bits = deserialize_nanbox_on_current_thread(sv);
            let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): parallelMap result slot uses the shared array slot-store helper.
            store_thread_array_slot(result_arr, write_idx, bits);
            write_idx += 1;
        }
    }
    let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    (*result_arr).length = total_results as u32;

    result_arr as i64
}

/// Fast path for single-threaded map (no serialization needed).
unsafe fn single_thread_map(
    arr: *const crate::array::ArrayHeader,
    len: usize,
    func: *const u8,
    closure_ptr: i64,
) -> i64 {
    // Root the input array AND the closure BEFORE allocating the result (the
    // allocation can trigger a moving minor), and re-derive both from their
    // rooted handles each iteration — the user callback can allocate too, and
    // a moved closure would leave later iterations calling through a dangling
    // capture block (#6521 review).
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr as *mut crate::array::ArrayHeader);
    let closure_handle = if closure_ptr != 0 {
        Some(scope.root_raw_mut_ptr(closure_ptr as *mut ClosureHeader))
    } else {
        None
    };
    let result_arr = crate::array::js_array_alloc(len as u32);
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let call_fn: ClosureCallFn =
        crate::closure::body_call::js_body_fn!(func as *const u8; argument);

    for i in 0..len {
        // Sparse-safe element read (see `parallel_map_impl`); re-derived from
        // the rooted handle each iteration because the callback can move it.
        let arg = crate::array::js_array_get_f64(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            i as u32,
        );
        let closure = closure_handle
            .as_ref()
            .map_or(ptr::null(), |h| h.get_raw_const_ptr::<ClosureHeader>());
        let result = call_fn(closure, crate::closure::plain_call_receiver(), arg);
        let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        // GC_STORE_AUDIT(BARRIERED): single-thread map result slot uses the shared array slot-store helper.
        store_thread_array_slot(result_arr, i, result.to_bits());
    }
    let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    (*result_arr).length = len as u32;

    result_arr as i64
}

// ============================================================================
// parallelFilter — data-parallel array filtering
// ============================================================================

/// FFI entry point for `parallelFilter(array, predicate)`.
///
/// Both arguments are NaN-boxed f64 values:
/// - `array_val`: POINTER_TAG'd ArrayHeader pointer
/// - `closure_val`: POINTER_TAG'd ClosureHeader pointer (predicate function)
///
/// Returns a POINTER_TAG'd ArrayHeader pointer containing only elements where
/// the predicate returned a truthy value.
#[no_mangle]
pub extern "C" fn js_thread_parallel_filter(array_val: f64, closure_val: f64) -> f64 {
    let result_ptr = unsafe { parallel_filter_impl(array_val, closure_val, 0) };
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

/// Compiler-only launch ABI: preparation is a code address in the spawning image.
#[no_mangle]
pub extern "C" fn js_thread_parallel_filter_with_literals(
    array_val: f64,
    closure_val: f64,
    literal_prepare: i64,
) -> f64 {
    let result_ptr = unsafe { parallel_filter_impl(array_val, closure_val, literal_prepare) };
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

unsafe fn parallel_filter_impl(array_val: f64, closure_val: f64, literal_prepare: i64) -> i64 {
    // Closure validated and rooted BEFORE `clean_arr_ptr` — same GC-point
    // ordering as `parallel_map_impl` above (#6521 review follow-up).
    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    if closure.is_null() || (closure as usize) < 0x1000 {
        return crate::array::js_array_alloc(0) as i64;
    }
    let func = (*closure).code();
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure as *mut ClosureHeader);

    let array_bits = array_val.to_bits();
    let arr = (array_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    // #6518: same forwarding-stub resolution as `parallel_map_impl` above.
    let arr = crate::array::clean_arr_ptr(arr);
    if arr.is_null() {
        return crate::array::js_array_alloc(0) as i64;
    }

    let len = (*arr).length as usize;
    if len == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }

    // Re-derive the (possibly moved) closure now that the GC points above
    // are behind us; no further GC points before the derefs below.
    let closure = closure_handle.get_raw_const_ptr::<ClosureHeader>();

    #[cfg(not(target_os = "wasi"))]
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(len);
    // WASI is single-threaded (#11377); see `parallel_map_impl`.
    #[cfg(target_os = "wasi")]
    let num_threads = 1;

    // Fast path: single thread for small arrays
    if num_threads <= 1 {
        return single_thread_filter(arr, len, func, closure);
    }

    // Serialize input elements (per-element accessor: sparse-safe, see
    // `parallel_map_impl`).
    let mut serialized_elements = Vec::with_capacity(len);
    for i in 0..len {
        let bits = crate::array::js_array_get_f64(arr, i as u32).to_bits();
        serialized_elements.push(serialize_nanbox_for_thread(bits));
    }
    // #6185: fail loudly on a non-transferable input element.
    guard_transferable(&serialized_elements);

    // Serialize closure captures
    let serialized_captures: Option<(usize, u32, Vec<SerializedValue>)> = {
        let fp = (*closure).info as usize;
        let cc = (*closure).capture_count;
        let actual = real_capture_count(cc) as usize;
        let base = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        let mut caps = Vec::with_capacity(actual);
        for i in 0..actual {
            caps.push(serialize_capture_for_thread((*base.add(i)).to_bits()));
        }
        guard_transferable(&caps); // #6185: named throw for a captured Map/Set/…
        Some((fp, cc, caps))
    };

    // Split into chunks
    let chunk_size = len.div_ceil(num_threads);
    let mut chunks: Vec<Vec<SerializedValue>> = Vec::with_capacity(num_threads);
    let mut remaining = serialized_elements;
    for _ in 0..num_threads {
        if remaining.is_empty() {
            break;
        }
        let split_at = chunk_size.min(remaining.len());
        let rest = remaining.split_off(split_at);
        chunks.push(remaining);
        remaining = rest;
    }
    if !remaining.is_empty() {
        if let Some(last) = chunks.last_mut() {
            last.extend(remaining);
        }
    }

    let captures_arc = serialized_captures.map(std::sync::Arc::new);
    let func_usize = func as usize;

    // Each thread returns (index, kept_elements) — kept elements in original order
    let mut all_results: Vec<Vec<SerializedValue>> =
        (0..chunks.len()).map(|_| Vec::new()).collect();

    let class_image = crate::object::class_image::current_image_handle();
    // Same image, so the same module initializers: a runtime `require` of a
    // Deferred module must find its initializer on the worker too.
    let path_inits = crate::module_require::current_path_init_image();
    // Charter step 5, P4: the worker installs the spawner's codegen ShapeIds
    // (with their reps) before any allocation; see `shapes_worker_seed`.
    let shape_seed = crate::object::shapes::worker_shape_seed();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let captures_ref = captures_arc.clone();
            let class_image = class_image.clone();
            let path_inits = path_inits.clone();
            let shape_seed = shape_seed.clone();

            let handle = scope.spawn(move || {
                // See parallel_map's worker: adopt the spawning image (#8546),
                // own agent (#6185) before anything can allocate or enqueue,
                // scanner registration must precede any allocation, and the
                // rebuilt closure must be rooted across the per-element
                // deserialization allocations.
                crate::object::class_image::adopt_image(class_image);
                crate::module_require::adopt_path_init_image(path_inits);
                let worker_agent = crate::agent::enter_worker_agent();
                crate::gc::ensure_gc_initialized();
                crate::object::shapes::install_worker_shape_seed(&shape_seed);
                unsafe { prepare_worker_literals(literal_prepare) };
                let mut kept = Vec::new();

                let gc_scope = crate::gc::RuntimeHandleScope::new();
                let closure_handle = if let Some(ref caps) = captures_ref {
                    let (fp, cc, ref cap_vals) = **caps;
                    let c =
                        closure::js_closure_alloc(fp as *const crate::closure::JsFunctionInfo, cc);
                    let h = gc_scope.root_raw_mut_ptr(c);
                    for (i, cap) in cap_vals.iter().enumerate() {
                        let bits = deserialize_nanbox_on_current_thread(cap);
                        crate::closure::js_closure_set_capture_f64(
                            h.get_raw_mut_ptr::<ClosureHeader>(),
                            i as u32,
                            f64::from_bits(bits),
                        );
                    }
                    Some(h)
                } else {
                    None
                };

                let call_fn: ClosureCallFn =
                    crate::closure::body_call::js_body_fn!(func_usize as *const u8; argument);

                for elem_sv in &chunk {
                    let arg = f64::from_bits(deserialize_nanbox_on_current_thread(elem_sv));
                    let local_closure = closure_handle
                        .as_ref()
                        .map(|h| h.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader)
                        .unwrap_or(ptr::null());
                    let result = call_fn(local_closure, crate::closure::plain_call_receiver(), arg);
                    let keep = is_truthy_bits(result.to_bits());
                    if keep {
                        kept.push(serialize_nanbox_for_thread(arg.to_bits()));
                    }
                }

                // #6185: see parallel_map's worker — purge this agent's queue
                // entries before its arena goes away.
                drop(gc_scope);
                crate::agent::retire_agent(worker_agent);
                (idx, kept)
            });
            handles.push(handle);
        }

        for handle in handles {
            if let Ok((idx, kept)) = handle.join() {
                all_results[idx] = kept;
            }
        }
    });

    // Deserialize kept elements into main thread's arena (preserving order)
    let total: usize = all_results.iter().map(|r| r.len()).sum();
    let result_arr = crate::array::js_array_alloc(total as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let mut write_idx = 0;
    for chunk_kept in &all_results {
        for sv in chunk_kept {
            let bits = deserialize_nanbox_on_current_thread(sv);
            let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): parallelFilter result slot uses the shared array slot-store helper.
            store_thread_array_slot(result_arr, write_idx, bits);
            write_idx += 1;
        }
    }
    let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    (*result_arr).length = total as u32;

    result_arr as i64
}

/// Fast path: single-threaded filter (no serialization).
unsafe fn single_thread_filter(
    arr: *const crate::array::ArrayHeader,
    len: usize,
    func: *const u8,
    closure: *const ClosureHeader,
) -> i64 {
    // Same rooting discipline as single_thread_map: the result allocation
    // and every user callback can trigger a moving minor, so the array AND
    // the closure are re-derived from rooted handles each iteration
    // (#6521 review).
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr as *mut crate::array::ArrayHeader);
    let closure_handle = if closure.is_null() {
        None
    } else {
        Some(scope.root_raw_mut_ptr(closure as *mut ClosureHeader))
    };
    let result_arr = crate::array::js_array_alloc(len as u32);
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let call_fn: ClosureCallFn =
        crate::closure::body_call::js_body_fn!(func as *const u8; argument);
    let mut count = 0u32;

    for i in 0..len {
        // Sparse-safe element read (see `parallel_map_impl`); re-derived from
        // the rooted handle each iteration because the callback can move it.
        let arg = crate::array::js_array_get_f64(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            i as u32,
        );
        let closure = closure_handle
            .as_ref()
            .map_or(ptr::null(), |h| h.get_raw_const_ptr::<ClosureHeader>());
        let result = call_fn(closure, crate::closure::plain_call_receiver(), arg);
        let keep = is_truthy_bits(result.to_bits());
        if keep {
            let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): single-thread filter result slot uses the shared array slot-store helper.
            store_thread_array_slot(result_arr, count as usize, arg.to_bits());
            count += 1;
        }
    }
    let result_arr = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    (*result_arr).length = count;

    result_arr as i64
}

// ============================================================================
// spawn — background thread execution
// ============================================================================

static ACTIVE_THREAD_JOBS: AtomicUsize = AtomicUsize::new(0);

/// The compiled closure function signature for zero-argument closures.
/// Takes only the closure header pointer, returns f64 result.
type ClosureCall0Fn = crate::closure::body_call::js_body_fn_ty!();

/// FFI entry point for `spawn(closure)`.
///
/// Argument is a NaN-boxed f64 ClosureHeader pointer (POINTER_TAG).
/// Returns a NaN-boxed f64 Promise pointer (POINTER_TAG).
#[no_mangle]
pub extern "C" fn js_thread_spawn(closure_val: f64) -> f64 {
    let promise = unsafe { spawn_impl(closure_val, 0) };
    // NaN-box the promise pointer with POINTER_TAG
    f64::from_bits(POINTER_TAG | (promise as u64 & POINTER_MASK))
}

/// Compiler-only launch ABI: preparation is a code address in the spawning image.
#[no_mangle]
pub extern "C" fn js_thread_spawn_with_literals(closure_val: f64, literal_prepare: i64) -> f64 {
    let promise = unsafe { spawn_impl(closure_val, literal_prepare) };
    f64::from_bits(POINTER_TAG | (promise as u64 & POINTER_MASK))
}

#[cfg_attr(target_os = "wasi", allow(unreachable_code, unused_variables))]
unsafe fn spawn_impl(closure_val: f64, literal_prepare: i64) -> *mut crate::promise::Promise {
    // WASI preview 2 has no threads (#11377). Running the worker body inline
    // is not faithful — it claims and retires its own agent — so until a
    // main-thread `spawn` lands with the WASI event loop, reject clearly
    // instead of aborting on the failed `std::thread::spawn`.
    #[cfg(target_os = "wasi")]
    {
        let msg = "perry/thread spawn() is not supported on WASI yet (#11377)";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_error_new_with_message(s);
        return crate::promise::js_promise_rejected(crate::value::js_nanbox_pointer(err as i64));
    }
    // ── 0. Extract closure pointer and body info ──────────────────────
    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    let info_usize = if !closure.is_null() && (closure as usize) >= 0x1000 {
        (*closure).info as usize
    } else {
        // No valid closure — return a resolved promise with undefined
        let promise = crate::promise::js_promise_new();
        crate::promise::js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
        return promise;
    };

    // ── 1. Serialize closure captures (before allocating the promise) ─
    // #6185: a captured non-transferable value (Map/Set/Promise/…) must throw
    // a named TypeError here on the calling thread. Serializing *before* the
    // promise is allocated keeps the throw clean — `js_throw` longjmps and does
    // not run Rust destructors, so a pinned promise allocated first would leak.
    let serialized_captures: Option<(u32, Vec<SerializedValue>)> = {
        let cc = (*closure).capture_count;
        let actual = real_capture_count(cc) as usize;
        if actual > 0 {
            let base =
                (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
            let mut caps = Vec::with_capacity(actual);
            for i in 0..actual {
                caps.push(serialize_capture_for_thread((*base.add(i)).to_bits()));
            }
            guard_transferable(&caps);
            Some((cc, caps))
        } else {
            None
        }
    };

    // ── 2. Allocate Promise on main thread ───────────────────────────
    // Cross-thread variant: this promise is referenced only by a raw usize
    // in PENDING_THREAD_RESULTS (no scanner) until drain — a nursery
    // resident would be destroyed by the copied-minor from-space flip even
    // while pinned. Malloc space is non-moving and sweeps honor the pin.
    // #9552: the cross-thread constructor pins the promise; the settlement in
    // `js_thread_process_pending` releases it.
    let promise = crate::promise::js_promise_new_cross_thread();

    let promise_usize = promise as usize;
    // #6185: the promise lives in the SPAWNING agent's heap, so that is the
    // agent allowed to settle it. Captured here, on the spawning thread —
    // reading it inside the worker would yield the worker's own agent.
    let owner_agent = crate::agent::current_agent();
    // #8546: the worker runs the closure body only, never module init, so its
    // class metadata (vtables, parents, constructors, …) must be the spawning
    // image's — captured here, adopted first thing on the worker.
    let class_image = crate::object::class_image::current_image_handle();
    // Same image, so the same module initializers: a runtime `require` of a
    // Deferred module must find its initializer on the worker too.
    let path_inits = crate::module_require::current_path_init_image();
    // Charter step 5, P4: the worker installs the spawner's codegen ShapeIds
    // (with their reps) before any allocation; see `shapes_worker_seed`.
    let shape_seed = crate::object::shapes::worker_shape_seed();

    // ── 3. Spawn background thread ───────────────────────────────────
    ACTIVE_THREAD_JOBS.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        crate::object::class_image::adopt_image(class_image);
        crate::module_require::adopt_path_init_image(path_inits);
        // #6185: claim an agent id for this worker BEFORE it can allocate or
        // enqueue anything, so every pointer it puts in a global queue is
        // tagged as its own — and so its own drains skip the spawner's work.
        let worker_agent = crate::agent::enter_worker_agent();
        // Register this thread's root scanners before any allocation can
        // cross a GC trigger (see the parallel_map worker for rationale).
        crate::gc::ensure_gc_initialized();
        crate::object::shapes::install_worker_shape_seed(&shape_seed);
        unsafe { prepare_worker_literals(literal_prepare) };
        // Reconstruct closure in this thread's arena, rooted across the
        // capture-deserialization allocations.
        let gc_scope = crate::gc::RuntimeHandleScope::new();
        let closure_handle = if let Some((cc, ref cap_vals)) = serialized_captures {
            let c =
                closure::js_closure_alloc(info_usize as *const crate::closure::JsFunctionInfo, cc);
            let h = gc_scope.root_raw_mut_ptr(c);
            for (i, cap) in cap_vals.iter().enumerate() {
                unsafe {
                    let bits = deserialize_nanbox_on_current_thread(cap);
                    crate::closure::js_closure_set_capture_f64(
                        h.get_raw_mut_ptr::<ClosureHeader>(),
                        i as u32,
                        f64::from_bits(bits),
                    );
                }
            }
            h
        } else {
            // No captures — create a minimal closure header
            gc_scope.root_raw_mut_ptr(closure::js_closure_alloc(
                info_usize as *const crate::closure::JsFunctionInfo,
                0,
            ))
        };

        // Call the function — catch panics to avoid aborting across FFI boundary
        let call_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let code = unsafe { (*(info_usize as *const crate::closure::JsFunctionInfo)).code };
            let call_fn: ClosureCall0Fn = unsafe { crate::closure::body_call::js_body_fn!(code;) };
            let local_closure =
                closure_handle.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader;
            call_fn(local_closure, crate::closure::plain_call_receiver())
        }));

        match call_result {
            Ok(result) => {
                // Serialize result for transfer back to the spawning agent.
                let serialized_result = unsafe { serialize_nanbox_for_thread(result.to_bits()) };
                queue_thread_result(owner_agent, promise_usize, serialized_result);
            }
            Err(_) => {
                // Thread panicked — resolve with undefined to avoid hanging promise
                queue_thread_result(
                    owner_agent,
                    promise_usize,
                    SerializedValue::Inline(TAG_UNDEFINED),
                );
            }
        }

        // #6185: this worker's arena is about to be unmapped. Drop the shadow
        // scope first (the result is already serialized into owner-independent
        // form above), then purge any queue entry still tagged with this agent —
        // nothing can ever legally settle those, and their pointers are about to
        // dangle. Must run AFTER the result is queued: that entry is tagged with
        // `owner_agent`, not `worker_agent`, so it survives the purge.
        drop(gc_scope);
        crate::agent::retire_agent(worker_agent);
    });

    promise
}

mod global_transfer;
pub use global_transfer::{js_thread_global_materialize, js_thread_global_publish};
mod pending_results;
pub(crate) use pending_results::purge_agent_thread_results;
use pending_results::queue_thread_result;
pub use pending_results::{
    js_thread_has_pending, js_thread_process_pending, queue_promise_string_rejection,
    queue_promise_string_result, thread_job_begin,
};

#[cfg(test)]
#[path = "thread_parent_class_id_tests.rs"]
mod parent_class_id_serialization_tests;
#[cfg(test)]
#[path = "thread_static_shape_tests.rs"]
mod static_shape_replay_tests;

#[cfg(test)]
#[path = "thread_transfer_guard_tests.rs"]
mod transfer_guard_tests;

#[path = "thread_constfn_transfer.rs"]
mod constfn_transfer;

#[cfg(test)]
#[path = "thread_constfn_transfer_tests.rs"]
mod constfn_transfer_tests;

#[cfg(all(test, not(target_os = "wasi")))]
#[path = "thread_literal_launch_tests.rs"]
mod literal_launch_tests;
