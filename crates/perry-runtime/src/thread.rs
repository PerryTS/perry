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
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::bigint::{self, BigIntHeader, BIGINT_LIMBS};
use crate::closure::{self, real_capture_count, ClosureHeader};
use crate::gc;
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
#[derive(Debug)]
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
        class_id: u32,
        parent_class_id: u32,
        fields: Vec<SerializedValue>,
        /// Key names for each field (for Object.keys() support).
        /// None for class instances where keys are defined by the class.
        keys: Option<Vec<Vec<u8>>>,
    },

    /// A closure: function pointer (global code, safe to share) + serialized captures.
    Closure {
        func_ptr: usize,
        capture_count: u32, // includes CAPTURES_THIS_FLAG
        captures: Vec<SerializedValue>,
    },

    /// A BigInt: 16 x u64 limbs in little-endian order.
    BigInt([u64; BIGINT_LIMBS]),

    /// A Date: its millisecond timestamp (may be NaN for an Invalid Date).
    /// Re-allocated as a fresh `DateCell` on the receiving thread (#2089) —
    /// deep-copy semantics, since the source cell's pointer is meaningless in
    /// another thread's arena.
    Date(f64),

    /// An `fs.promises.FileHandle` crossing a `perry/thread` boundary.
    /// Perry's fd registry is thread-local, so handles are not transferable;
    /// deserialize as a FileHandle-shaped object with `fd === -1`.
    DetachedFileHandle,

    /// A `SharedArrayBuffer` crossing a `perry/thread` boundary (#4913).
    /// Carries the process-global backing-store address by reference — NOT a
    /// byte copy — so the receiving agent's views alias the same physical
    /// memory and `Atomics.wait`/`notify` coordinate across threads. The
    /// backing is never freed (see `crate::shared_sab`), so the raw address
    /// stays valid for the life of the process.
    SharedArrayBuffer { addr: usize },
}

// Safety: SerializedValue contains no raw pointers to arena memory.
// func_ptr in Closure points to compiled code in the executable's text segment,
// which is process-global and immutable.
unsafe impl Send for SerializedValue {}
unsafe impl Sync for SerializedValue {}

// ============================================================================
// Serialization: JSValue (NaN-boxed, arena pointers) → SerializedValue
// ============================================================================

/// Serialize a NaN-boxed JSValue into a thread-safe SerializedValue.
///
/// Reads from the current thread's arena to extract pointer-based values
/// (strings, arrays, objects, closures, BigInts) into owned Rust data.
///
/// # Safety
/// The `bits` must be a valid NaN-boxed JSValue. Pointer-tagged values must
/// point to valid, live objects in the current thread's arena or malloc heap.
pub unsafe fn serialize_nanbox_for_thread(bits: u64) -> SerializedValue {
    let tag = bits & TAG_MASK;

    // Fast path: values that are just bit patterns (no pointers)
    match bits {
        TAG_UNDEFINED | TAG_NULL | TAG_TRUE | TAG_FALSE => {
            return SerializedValue::Inline(bits);
        }
        _ => {}
    }

    // Int32: just bit pattern, no pointer
    if tag == INT32_TAG {
        return SerializedValue::Inline(bits);
    }

    // String: copy UTF-8 bytes from StringHeader
    if tag == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            return SerializedValue::String(Vec::new());
        }
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len).to_vec();
        return SerializedValue::String(bytes);
    }

    // BigInt: copy limbs
    if tag == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const BigIntHeader;
        let ptr = bigint::clean_bigint_ptr(ptr);
        if ptr.is_null() {
            return SerializedValue::BigInt([0u64; BIGINT_LIMBS]);
        }
        return SerializedValue::BigInt((*ptr).limbs);
    }

    // Pointer: could be array, object, or closure
    if tag == POINTER_TAG {
        let raw_ptr = (bits & POINTER_MASK) as *const u8;
        if raw_ptr.is_null() || (raw_ptr as usize) < 0x1000 {
            return SerializedValue::Inline(TAG_UNDEFINED);
        }

        // SharedArrayBuffer: a process-global backing store (no GcHeader).
        // Pass it by reference so the receiving agent aliases the same bytes
        // (#4913). This MUST precede the GcHeader read below — a SAB header has
        // no preceding GcHeader, so reading one would misclassify it.
        if crate::shared_sab::is_shared_sab(raw_ptr as usize) {
            return SerializedValue::SharedArrayBuffer {
                addr: raw_ptr as usize,
            };
        }

        // Check GcHeader to determine type
        let header = raw_ptr.sub(gc::GC_HEADER_SIZE) as *const gc::GcHeader;
        let obj_type = (*header).obj_type;

        match obj_type {
            gc::GC_TYPE_ARRAY => {
                return serialize_array(raw_ptr as *const crate::array::ArrayHeader);
            }
            gc::GC_TYPE_OBJECT => {
                let value = f64::from_bits(bits);
                if crate::fs::is_fs_filehandle_value(value)
                    || crate::fs::filehandle_object_fd(value).is_some()
                {
                    return SerializedValue::DetachedFileHandle;
                }
                return serialize_object(raw_ptr as *const crate::object::ObjectHeader);
            }
            gc::GC_TYPE_CLOSURE => {
                return serialize_closure(raw_ptr as *const ClosureHeader);
            }
            gc::GC_TYPE_DATE_CELL => {
                // #2089: copy the timestamp; the receiving thread re-allocates
                // a fresh cell (deep-copy, like every other crossed value).
                return SerializedValue::Date((*(raw_ptr as *const crate::date::DateCell)).ts);
            }
            _ => {
                // Unknown pointer type — treat as undefined
                return SerializedValue::Inline(TAG_UNDEFINED);
            }
        }
    }

    // Regular f64 number (no tag in the NaN-boxing range we use)
    SerializedValue::Inline(bits)
}

/// Serialize an ArrayHeader into a SerializedValue::Array.
unsafe fn serialize_array(arr: *const crate::array::ArrayHeader) -> SerializedValue {
    if arr.is_null() || (arr as usize) < 0x1000 {
        return SerializedValue::Array(Vec::new());
    }
    let len = (*arr).length as usize;
    let elements_ptr =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;

    let mut elements = Vec::with_capacity(len);
    for i in 0..len {
        let elem_bits = (*elements_ptr.add(i)).to_bits();
        elements.push(serialize_nanbox_for_thread(elem_bits));
    }
    SerializedValue::Array(elements)
}

/// Serialize an ObjectHeader into a SerializedValue::Object.
unsafe fn serialize_object(obj: *const crate::object::ObjectHeader) -> SerializedValue {
    if obj.is_null() || (obj as usize) < 0x1000 {
        return SerializedValue::Object {
            class_id: 0,
            parent_class_id: 0,
            fields: Vec::new(),
            keys: None,
        };
    }

    let class_id = (*obj).class_id;
    let parent_class_id = (*obj).parent_class_id;
    let field_count = (*obj).field_count as usize;

    // Serialize field values
    let fields_ptr =
        (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const f64;
    let mut fields = Vec::with_capacity(field_count);
    for i in 0..field_count {
        let field_bits = (*fields_ptr.add(i)).to_bits();
        fields.push(serialize_nanbox_for_thread(field_bits));
    }

    // Serialize keys array if present (plain objects have keys, class instances don't)
    let keys = if !(*obj).keys_array.is_null() {
        let keys_arr = (*obj).keys_array;
        let keys_len = (*keys_arr).length as usize;
        let keys_elements = (keys_arr as *const u8)
            .add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;
        let mut key_strings = Vec::with_capacity(keys_len);
        for i in 0..keys_len {
            let key_bits = (*keys_elements.add(i)).to_bits();
            let key_tag = key_bits & TAG_MASK;
            if key_tag == STRING_TAG {
                let str_ptr = (key_bits & POINTER_MASK) as *const crate::string::StringHeader;
                if !str_ptr.is_null() && (str_ptr as usize) >= 0x1000 {
                    let len = (*str_ptr).byte_len as usize;
                    let data = (str_ptr as *const u8)
                        .add(std::mem::size_of::<crate::string::StringHeader>());
                    key_strings.push(std::slice::from_raw_parts(data, len).to_vec());
                } else {
                    key_strings.push(Vec::new());
                }
            } else {
                key_strings.push(Vec::new());
            }
        }
        Some(key_strings)
    } else {
        None
    };

    SerializedValue::Object {
        class_id,
        parent_class_id,
        fields,
        keys,
    }
}

/// Serialize a ClosureHeader into a SerializedValue::Closure.
unsafe fn serialize_closure(closure: *const ClosureHeader) -> SerializedValue {
    if closure.is_null() || (closure as usize) < 0x1000 {
        return SerializedValue::Inline(TAG_UNDEFINED);
    }

    let func_ptr = (*closure).func_ptr as usize;
    let capture_count_raw = (*closure).capture_count;
    let actual_count = real_capture_count(capture_count_raw) as usize;

    let captures_base =
        (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
    let mut captures = Vec::with_capacity(actual_count);
    for i in 0..actual_count {
        let cap_bits = (*captures_base.add(i)).to_bits();
        captures.push(serialize_nanbox_for_thread(cap_bits));
    }

    SerializedValue::Closure {
        func_ptr,
        capture_count: capture_count_raw,
        captures,
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

/// Deserialize a SerializedValue into a NaN-boxed JSValue.
///
/// Allocates any needed objects (strings, arrays, objects, closures) in the
/// **current thread's** arena. This is the key safety property: the caller
/// controls which arena receives the allocations by calling this function
/// on the appropriate thread.
///
/// # Returns
/// The raw u64 bits of the NaN-boxed JSValue.
pub unsafe fn deserialize_nanbox_on_current_thread(sv: &SerializedValue) -> u64 {
    match sv {
        SerializedValue::Inline(bits) => *bits,

        SerializedValue::String(bytes) => {
            let str_ptr = crate::string::js_string_from_bytes(
                if bytes.is_empty() {
                    ptr::null()
                } else {
                    bytes.as_ptr()
                },
                bytes.len() as u32,
            );
            JSValue::string_ptr(str_ptr).bits()
        }

        SerializedValue::Array(elements) => {
            let arr = crate::array::js_array_alloc(elements.len() as u32);
            let scope = crate::gc::RuntimeHandleScope::new();
            let arr_handle = scope.root_raw_mut_ptr(arr);
            for (i, elem) in elements.iter().enumerate() {
                let bits = deserialize_nanbox_on_current_thread(elem);
                let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
                // GC_STORE_AUDIT(BARRIERED): deserialized thread array slot uses the shared array slot-store helper.
                store_thread_array_slot(arr, i, bits);
            }
            let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            (*arr).length = elements.len() as u32;
            JSValue::pointer(arr as *const u8).bits()
        }

        SerializedValue::Object {
            class_id,
            parent_class_id,
            fields,
            keys,
        } => {
            let obj = crate::object::js_object_alloc_with_parent(
                *class_id,
                *parent_class_id,
                fields.len() as u32,
            );
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);

            // Set field values
            for (i, field) in fields.iter().enumerate() {
                let bits = deserialize_nanbox_on_current_thread(field);
                let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                // GC_STORE_AUDIT(BARRIERED): deserialized thread object field uses the shared object slot-store helper.
                store_thread_object_field(obj, i, bits);
            }

            // Reconstruct keys array if present
            if let Some(key_strings) = keys {
                let keys_arr = crate::array::js_array_alloc(key_strings.len() as u32);
                let keys_handle = scope.root_raw_mut_ptr(keys_arr);
                for (i, key_bytes) in key_strings.iter().enumerate() {
                    let str_ptr = crate::string::js_string_from_bytes(
                        if key_bytes.is_empty() {
                            ptr::null()
                        } else {
                            key_bytes.as_ptr()
                        },
                        key_bytes.len() as u32,
                    );
                    let key_val = JSValue::string_ptr(str_ptr);
                    let keys_arr = keys_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
                    // GC_STORE_AUDIT(BARRIERED): deserialized key array slot uses the shared array slot-store helper.
                    store_thread_array_slot(keys_arr, i, key_val.bits());
                }
                let keys_arr = keys_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
                (*keys_arr).length = key_strings.len() as u32;
                let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                crate::object::js_object_set_keys(obj, keys_arr);
            }

            let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            JSValue::pointer(obj as *const u8).bits()
        }

        SerializedValue::Closure {
            func_ptr,
            capture_count,
            captures,
        } => {
            let closure = closure::js_closure_alloc(*func_ptr as *const u8, *capture_count);
            for (i, cap) in captures.iter().enumerate() {
                let bits = deserialize_nanbox_on_current_thread(cap);
                crate::closure::js_closure_set_capture_f64(closure, i as u32, f64::from_bits(bits));
            }
            JSValue::pointer(closure as *const u8).bits()
        }

        SerializedValue::BigInt(limbs) => {
            let ptr = bigint::bigint_alloc_with_limbs(*limbs);
            // NaN-box with BIGINT_TAG
            BIGINT_TAG | (ptr as u64 & POINTER_MASK)
        }

        SerializedValue::Date(ts) => {
            // #2089: allocate a fresh DateCell in THIS thread's arena.
            crate::date::alloc_date_cell(*ts).to_bits()
        }

        SerializedValue::DetachedFileHandle => {
            crate::fs::build_detached_filehandle_object().to_bits()
        }

        SerializedValue::SharedArrayBuffer { addr } => {
            // Alias the same process-global backing store (#4913) — no copy.
            // Re-register it in THIS thread's buffer / SAB tables so local
            // predicates (`is_registered_buffer`, `is_shared_array_buffer`) and
            // `new Int32Array(sab)` view construction recognise it here too.
            crate::buffer::register_buffer(*addr as *const crate::buffer::BufferHeader);
            crate::buffer::mark_as_shared_array_buffer(*addr);
            JSValue::pointer(*addr as *const u8).bits()
        }
    }
}

#[cfg(test)]
pub(crate) unsafe fn test_deserialize_bigint_limbs(limbs: [u64; BIGINT_LIMBS]) -> u64 {
    deserialize_nanbox_on_current_thread(&SerializedValue::BigInt(limbs))
}

// ============================================================================
// parallelMap — data-parallel array processing
// ============================================================================

/// The compiled closure function signature: (closure_header, argument) -> result.
/// This matches Perry's closure calling convention where the first parameter
/// is a pointer to the ClosureHeader (for accessing captures) and the second
/// is the f64 argument.
type ClosureCallFn = unsafe extern "C" fn(*const ClosureHeader, f64) -> f64;

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
/// - `closure_val`: POINTER_TAG'd ClosureHeader pointer (contains func_ptr + captures)
///
/// Returns a POINTER_TAG'd ArrayHeader pointer to the result array.
#[no_mangle]
pub extern "C" fn js_thread_parallel_map(array_val: f64, closure_val: f64) -> f64 {
    let result_ptr = unsafe { parallel_map_impl(array_val, closure_val) };
    // NaN-box the result array pointer with POINTER_TAG
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

unsafe fn parallel_map_impl(array_val: f64, closure_val: f64) -> i64 {
    // ── 1. Extract array pointer from NaN-boxed value ────────────────
    let array_bits = array_val.to_bits();
    let arr = (array_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    if arr.is_null() || (arr as usize) < 0x1000 {
        return crate::array::js_array_alloc(0) as i64;
    }

    let len = (*arr).length as usize;
    if len == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }

    // ── 1b. Extract closure pointer and func_ptr ─────────────────────
    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    let func = if !closure.is_null() && (closure as usize) >= 0x1000 {
        (*closure).func_ptr
    } else {
        // No valid closure — can't call anything
        return crate::array::js_array_alloc(0) as i64;
    };
    let closure_ptr_raw = closure as i64;

    // ── 2. Determine thread count ────────────────────────────────────
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // Don't spawn more threads than elements
    let num_threads = num_threads.min(len);

    // ── 3. Fast path: single thread (small arrays) ───────────────────
    if num_threads <= 1 {
        return single_thread_map(arr, len, func, closure_ptr_raw);
    }

    // ── 4. Serialize all input elements ──────────────────────────────
    let elements_ptr =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    let mut serialized_elements = Vec::with_capacity(len);
    for i in 0..len {
        let bits = (*elements_ptr.add(i)).to_bits();
        serialized_elements.push(serialize_nanbox_for_thread(bits));
    }

    // ── 5. Serialize closure captures (shared across all threads) ────
    let serialized_captures: Option<(usize, u32, Vec<SerializedValue>)> = {
        if !closure.is_null() && (closure as usize) >= 0x1000 {
            let fp = (*closure).func_ptr as usize;
            let cc = (*closure).capture_count;
            let actual = real_capture_count(cc) as usize;
            let base =
                (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
            let mut caps = Vec::with_capacity(actual);
            for i in 0..actual {
                caps.push(serialize_nanbox_for_thread((*base.add(i)).to_bits()));
            }
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

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let captures_ref = captures_arc.clone();

            let handle = scope.spawn(move || {
                // Each thread has its own arena (via thread_local!).
                // Register this thread's root scanners BEFORE any allocation
                // can cross a GC trigger — a fresh worker otherwise collects
                // with an empty scanner registry (and an empty shadow stack)
                // and sweeps everything it just deserialized.
                crate::gc::ensure_gc_initialized();
                let mut results = Vec::with_capacity(chunk.len());

                // Reconstruct closure on this thread's arena, rooted for the
                // whole loop: per-element deserialization below can allocate
                // and trigger a worker GC, and a bare local is not a root.
                let gc_scope = crate::gc::RuntimeHandleScope::new();
                let closure_handle = if let Some(ref caps) = captures_ref {
                    let (fp, cc, ref cap_vals) = **caps;
                    let c = closure::js_closure_alloc(fp as *const u8, cc);
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

                let call_fn: ClosureCallFn = std::mem::transmute(func_usize);

                for elem_sv in &chunk {
                    let arg = f64::from_bits(deserialize_nanbox_on_current_thread(elem_sv));
                    let local_closure = closure_handle
                        .as_ref()
                        .map(|h| h.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader)
                        .unwrap_or(ptr::null());
                    let result = call_fn(local_closure, arg);
                    results.push(serialize_nanbox_for_thread(result.to_bits()));
                }

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
    // Root the input array BEFORE allocating the result (the allocation can
    // trigger a moving minor), and re-derive the elements pointer from the
    // rooted handle each iteration — the user callback can allocate too.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr as *mut crate::array::ArrayHeader);
    let result_arr = crate::array::js_array_alloc(len as u32);
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let closure = if closure_ptr != 0 {
        closure_ptr as *const ClosureHeader
    } else {
        ptr::null()
    };

    let call_fn: ClosureCallFn = std::mem::transmute(func as usize);

    for i in 0..len {
        let elements_ptr = (arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>() as *const u8)
            .add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;
        let arg = *elements_ptr.add(i);
        let result = call_fn(closure, arg);
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
    let result_ptr = unsafe { parallel_filter_impl(array_val, closure_val) };
    f64::from_bits(POINTER_TAG | (result_ptr as u64 & POINTER_MASK))
}

unsafe fn parallel_filter_impl(array_val: f64, closure_val: f64) -> i64 {
    let array_bits = array_val.to_bits();
    let arr = (array_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    if arr.is_null() || (arr as usize) < 0x1000 {
        return crate::array::js_array_alloc(0) as i64;
    }

    let len = (*arr).length as usize;
    if len == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }

    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    let func = if !closure.is_null() && (closure as usize) >= 0x1000 {
        (*closure).func_ptr
    } else {
        return crate::array::js_array_alloc(0) as i64;
    };

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(len);

    // Fast path: single thread for small arrays
    if num_threads <= 1 {
        return single_thread_filter(arr, len, func, closure);
    }

    // Serialize input elements
    let elements_ptr =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    let mut serialized_elements = Vec::with_capacity(len);
    for i in 0..len {
        let bits = (*elements_ptr.add(i)).to_bits();
        serialized_elements.push(serialize_nanbox_for_thread(bits));
    }

    // Serialize closure captures
    let serialized_captures: Option<(usize, u32, Vec<SerializedValue>)> = {
        let fp = (*closure).func_ptr as usize;
        let cc = (*closure).capture_count;
        let actual = real_capture_count(cc) as usize;
        let base = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        let mut caps = Vec::with_capacity(actual);
        for i in 0..actual {
            caps.push(serialize_nanbox_for_thread((*base.add(i)).to_bits()));
        }
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

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let captures_ref = captures_arc.clone();

            let handle = scope.spawn(move || {
                // See parallel_map's worker: scanner registration must precede
                // any allocation, and the rebuilt closure must be rooted
                // across the per-element deserialization allocations.
                crate::gc::ensure_gc_initialized();
                let mut kept = Vec::new();

                let gc_scope = crate::gc::RuntimeHandleScope::new();
                let closure_handle = if let Some(ref caps) = captures_ref {
                    let (fp, cc, ref cap_vals) = **caps;
                    let c = closure::js_closure_alloc(fp as *const u8, cc);
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

                let call_fn: ClosureCallFn = std::mem::transmute(func_usize);

                for elem_sv in &chunk {
                    let arg = f64::from_bits(deserialize_nanbox_on_current_thread(elem_sv));
                    let local_closure = closure_handle
                        .as_ref()
                        .map(|h| h.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader)
                        .unwrap_or(ptr::null());
                    let result = call_fn(local_closure, arg);
                    let keep = is_truthy_bits(result.to_bits());
                    if keep {
                        kept.push(serialize_nanbox_for_thread(arg.to_bits()));
                    }
                }

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
    // and every user callback can trigger a moving minor.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr as *mut crate::array::ArrayHeader);
    let result_arr = crate::array::js_array_alloc(len as u32);
    let result_handle = scope.root_raw_mut_ptr(result_arr);

    let call_fn: ClosureCallFn = std::mem::transmute(func as usize);
    let mut count = 0u32;

    for i in 0..len {
        let elements_ptr = (arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>() as *const u8)
            .add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;
        let arg = *elements_ptr.add(i);
        let result = call_fn(closure, arg);
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
type ClosureCall0Fn = unsafe extern "C" fn(*const ClosureHeader) -> f64;

/// FFI entry point for `spawn(closure)`.
///
/// Argument is a NaN-boxed f64 ClosureHeader pointer (POINTER_TAG).
/// Returns a NaN-boxed f64 Promise pointer (POINTER_TAG).
#[no_mangle]
pub extern "C" fn js_thread_spawn(closure_val: f64) -> f64 {
    let promise = unsafe { spawn_impl(closure_val) };
    // NaN-box the promise pointer with POINTER_TAG
    f64::from_bits(POINTER_TAG | (promise as u64 & POINTER_MASK))
}

unsafe fn spawn_impl(closure_val: f64) -> *mut crate::promise::Promise {
    // ── 0. Extract closure pointer and func_ptr ──────────────────────
    let closure_bits = closure_val.to_bits();
    let closure = (closure_bits & POINTER_MASK) as *const ClosureHeader;
    let func_usize = if !closure.is_null() && (closure as usize) >= 0x1000 {
        (*closure).func_ptr as usize
    } else {
        // No valid closure — return a resolved promise with undefined
        let promise = crate::promise::js_promise_new();
        crate::promise::js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
        return promise;
    };

    // ── 1. Allocate Promise on main thread ───────────────────────────
    // Cross-thread variant: this promise is referenced only by a raw usize
    // in PENDING_THREAD_RESULTS (no scanner) until drain — a nursery
    // resident would be destroyed by the copied-minor from-space flip even
    // while pinned. Malloc space is non-moving and sweeps honor the pin.
    let promise = crate::promise::js_promise_new_cross_thread();

    // Pin the promise so GC doesn't collect it while the thread is running
    let promise_header = (promise as *mut u8).sub(gc::GC_HEADER_SIZE) as *mut gc::GcHeader;
    (*promise_header).gc_flags |= gc::GC_FLAG_PINNED;

    let promise_usize = promise as usize;

    // ── 2. Serialize closure captures ────────────────────────────────
    let serialized_captures: Option<(u32, Vec<SerializedValue>)> = {
        let cc = (*closure).capture_count;
        let actual = real_capture_count(cc) as usize;
        if actual > 0 {
            let base =
                (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
            let mut caps = Vec::with_capacity(actual);
            for i in 0..actual {
                caps.push(serialize_nanbox_for_thread((*base.add(i)).to_bits()));
            }
            Some((cc, caps))
        } else {
            None
        }
    };

    // ── 3. Spawn background thread ───────────────────────────────────
    ACTIVE_THREAD_JOBS.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        // Register this thread's root scanners before any allocation can
        // cross a GC trigger (see the parallel_map worker for rationale).
        crate::gc::ensure_gc_initialized();
        // Reconstruct closure in this thread's arena, rooted across the
        // capture-deserialization allocations.
        let gc_scope = crate::gc::RuntimeHandleScope::new();
        let closure_handle = if let Some((cc, ref cap_vals)) = serialized_captures {
            let c = closure::js_closure_alloc(func_usize as *const u8, cc);
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
            gc_scope.root_raw_mut_ptr(closure::js_closure_alloc(func_usize as *const u8, 0))
        };

        // Call the function — catch panics to avoid aborting across FFI boundary
        let call_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let call_fn: ClosureCall0Fn = unsafe { std::mem::transmute(func_usize) };
            let local_closure =
                closure_handle.get_raw_mut_ptr::<ClosureHeader>() as *const ClosureHeader;
            unsafe { call_fn(local_closure) }
        }));

        match call_result {
            Ok(result) => {
                // Serialize result for transfer back to main thread
                let serialized_result = unsafe { serialize_nanbox_for_thread(result.to_bits()) };
                queue_thread_result(promise_usize, serialized_result);
            }
            Err(_) => {
                // Thread panicked — resolve with undefined to avoid hanging promise
                queue_thread_result(promise_usize, SerializedValue::Inline(TAG_UNDEFINED));
            }
        }
    });

    promise
}

/// Queue a thread's result for resolution on the main thread.
///
/// Uses the stdlib's PENDING_DEFERRED mechanism. The converter function
/// runs on the main thread during `js_stdlib_process_pending()`, which
/// deserializes the value into the main thread's arena.
fn queue_thread_result(promise_usize: usize, result: SerializedValue) {
    // We need to interact with perry-stdlib's deferred resolution queue.
    // Since perry-runtime cannot depend on perry-stdlib, we use the same
    // pattern as timer resolution: store the result and let the pump pick it up.
    //
    // Thread results are stored in a global Mutex queue. The main thread's
    // pump function (js_thread_process_pending) drains this queue and resolves
    // the promises.
    {
        let mut pending = match PENDING_THREAD_RESULTS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.push(PendingThreadResult {
            promise_ptr: promise_usize,
            result,
        });
    }
    ACTIVE_THREAD_JOBS.fetch_sub(1, Ordering::SeqCst);
    // Issue #84: wake the main thread so spawn()-returned promises
    // resolve as soon as the OS thread finishes, not at the next
    // event-loop quantum.
    crate::event_pump::js_notify_main_thread();
}

/// Register the start of a background job that will later resolve a promise on
/// the main thread via [`queue_promise_string_result`]. Keeps the event loop
/// alive until the result arrives (mirrors `spawn`'s job accounting). Used by
/// `Atomics.waitAsync` (#4913).
pub fn thread_job_begin() {
    ACTIVE_THREAD_JOBS.fetch_add(1, Ordering::SeqCst);
}

/// Pin `promise` so GC keeps it alive while a background job runs; the matching
/// unpin happens in [`js_thread_process_pending`] when the result resolves.
///
/// # Safety
/// `promise` must be a live promise allocation preceded by an 8-byte GcHeader.
pub unsafe fn pin_promise(promise: *mut crate::promise::Promise) {
    let header = (promise as *mut u8).sub(gc::GC_HEADER_SIZE) as *mut gc::GcHeader;
    (*header).gc_flags |= gc::GC_FLAG_PINNED;
}

/// Resolve the promise at `promise_usize` with a UTF-8 string on the main
/// thread. Routes through the same pending-result path `spawn` uses (which
/// unpins the promise, deserializes the value into the main arena, decrements
/// the active-job count, and wakes the event loop). Used by `Atomics.waitAsync`.
pub fn queue_promise_string_result(promise_usize: usize, value: &str) {
    queue_thread_result(
        promise_usize,
        SerializedValue::String(value.as_bytes().to_vec()),
    );
}

/// A pending thread result waiting to be resolved on the main thread.
struct PendingThreadResult {
    promise_ptr: usize,
    result: SerializedValue,
}

// Safety: SerializedValue is Send, usize is Send.
unsafe impl Send for PendingThreadResult {}

/// Global queue for pending thread results.
static PENDING_THREAD_RESULTS: std::sync::Mutex<Vec<PendingThreadResult>> =
    std::sync::Mutex::new(Vec::new());

/// Process pending thread results. Called from the main thread's event loop
/// (registered as a pump function, similar to js_stdlib_process_pending).
///
/// Drains the queue, deserializes each result into the main thread's arena,
/// and resolves the corresponding Promise.
///
/// # Returns
/// Number of results processed.
#[no_mangle]
pub extern "C" fn js_thread_process_pending() -> i32 {
    let mut pending = match PENDING_THREAD_RESULTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let count = pending.len() as i32;

    for item in pending.drain(..) {
        unsafe {
            let promise = item.promise_ptr as *mut crate::promise::Promise;

            // Deserialize the result into the main thread's arena
            let result_bits = deserialize_nanbox_on_current_thread(&item.result);

            // Unpin the promise now that we're resolving it
            let promise_header = (promise as *mut u8).sub(gc::GC_HEADER_SIZE) as *mut gc::GcHeader;
            (*promise_header).gc_flags &= !gc::GC_FLAG_PINNED;

            // Resolve the promise
            crate::promise::js_promise_resolve(promise, f64::from_bits(result_bits));
        }
    }

    count
}

/// Check if there are any pending thread results.
/// Used by the event loop to know whether to keep spinning.
#[no_mangle]
pub extern "C" fn js_thread_has_pending() -> i32 {
    if ACTIVE_THREAD_JOBS.load(Ordering::SeqCst) != 0 {
        return 1;
    }
    let pending = PENDING_THREAD_RESULTS.lock().unwrap();
    if pending.is_empty() {
        0
    } else {
        1
    }
}
