//! Buffer module - provides binary data handling similar to Node.js Buffer

use std::alloc::{alloc, Layout};
use std::ptr;

use crate::string::{js_string_from_bytes, StringHeader};
use crate::array::ArrayHeader;

/// Type ID constant for Buffer/Uint8Array - matches class_id 0xFFFF0004
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

/// Buffer header - similar to StringHeader but specifically for binary data
/// NOTE: Layout must match ArrayHeader (length at offset 0, capacity at offset 4)
/// because the codegen treats Uint8Array like arrays with hardcoded offsets.
#[repr(C)]
pub struct BufferHeader {
    /// Length in bytes
    pub length: u32,
    /// Capacity (allocated space)
    pub capacity: u32,
}

/// Calculate the layout for a buffer with given capacity
fn buffer_layout(capacity: usize) -> Layout {
    let total_size = std::mem::size_of::<BufferHeader>() + capacity;
    Layout::from_size_align(total_size, 8).unwrap()
}

/// Thread-local registry of buffer pointers for instanceof checks.
/// Since BufferHeader has the same layout as ArrayHeader (no type_id field),
/// we track buffer pointers separately to distinguish them from arrays.
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static BUFFER_REGISTRY: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    /// Buffers that were specifically created via `new Uint8Array(...)` —
    /// formatted as `Uint8Array(N) [ a, b, c ]` instead of `<Buffer aa bb cc>`.
    static UINT8ARRAY_FROM_CTOR: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Register a buffer pointer in the thread-local registry
pub fn register_buffer(ptr: *const BufferHeader) {
    BUFFER_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

// ----- Small-buffer slab allocator ----------------------------------------
//
// GC interaction:
//   Buffers carry no GcHeader and are not tracked in MALLOC_STATE (the existing
//   malloc path also never calls `dealloc` on individual buffers — they live for
//   the lifetime of the thread). Slab blocks are malloc'd once and retained for
//   the same duration. No GC behaviour changes.
//
// Registry:
//   Large buffers (capacity >= SMALL_BUF_THRESHOLD) still go through
//   `register_buffer` and appear in BUFFER_REGISTRY (HashSet).
//   Small buffers skip the HashSet insert; `is_registered_buffer` instead
//   performs a range-check against the (tiny) list of slab blocks — O(n_slabs),
//   typically ≤ 5 entries for a 100k-iteration allocation loop.
//   No false positives: slab blocks exclusively contain BufferHeader allocations
//   and all callers of `is_registered_buffer` pass the header pointer (the
//   NaN-boxed POINTER_TAG value always points to the header start, never to
//   interior data bytes).

/// Capacities strictly below this threshold use the slab fast path.
pub const SMALL_BUF_THRESHOLD: u32 = 256;

/// One slab block covers this many bytes of BufferHeader+data storage.
/// 256 KB → ≥ 1 000 allocations of the max small size (255 bytes), or up to
/// 32 768 allocations of the minimum (0 bytes / header only).
const SLAB_CAPACITY: usize = 256 * 1024;

/// Per-thread bump-pointer slab for small buffers.
/// Raw pointers stored as `usize` to keep the type `Send + Sync`.
struct SmallBufSlab {
    /// Byte offset of the next free slot within the current slab block.
    current: usize,
    /// One-past-the-end offset (absolute address as usize) of the current block.
    end: usize,
    /// (start, end) address pair for every slab block allocated so far.
    /// Used by `is_registered_buffer` to confirm an address is a small buffer.
    ranges: Vec<(usize, usize)>,
}

thread_local! {
    static SMALL_BUF_SLAB: RefCell<SmallBufSlab> = RefCell::new(SmallBufSlab {
        current: 0,
        end: 0,
        ranges: Vec::new(),
    });
}

fn buffer_alloc_small(capacity: u32) -> *mut BufferHeader {
    let needed = std::mem::size_of::<BufferHeader>() + capacity as usize;
    // Round up to 8-byte boundary so every header is naturally aligned.
    let aligned = (needed + 7) & !7;

    SMALL_BUF_SLAB.with(|slab_ref| {
        let mut slab = slab_ref.borrow_mut();

        if slab.current + aligned > slab.end {
            // Current block exhausted (or first call): allocate a fresh slab.
            let layout = Layout::from_size_align(SLAB_CAPACITY, 8).unwrap();
            let block = unsafe { alloc(layout) };
            if block.is_null() {
                panic!("buffer: failed to allocate small-buffer slab ({} bytes)", SLAB_CAPACITY);
            }
            let block_start = block as usize;
            let block_end = block_start + SLAB_CAPACITY;
            slab.ranges.push((block_start, block_end));
            slab.current = block_start;
            slab.end = block_end;
        }

        let ptr = slab.current as *mut BufferHeader;
        slab.current += aligned;

        unsafe {
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
        }

        ptr
    })
}

/// Check if a pointer is a registered buffer (for instanceof Uint8Array)
pub fn is_registered_buffer(addr: usize) -> bool {
    // Fast path: address falls within a small-buffer slab block.  All bytes in
    // a slab block belong exclusively to BufferHeader allocations, so any match
    // is definitively a buffer pointer.
    let in_slab = SMALL_BUF_SLAB.with(|slab_ref| {
        let slab = slab_ref.borrow();
        slab.ranges.iter().any(|&(start, end)| addr >= start && addr < end)
    });
    if in_slab {
        return true;
    }
    // Slow path: large buffers tracked in the HashSet registry.
    BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Mark this buffer as one that came from `new Uint8Array(...)` so it
/// formats as `Uint8Array(N) [ ... ]` rather than `<Buffer ...>`.
pub fn mark_as_uint8array(addr: usize) {
    UINT8ARRAY_FROM_CTOR.with(|r| { r.borrow_mut().insert(addr); });
}

pub fn is_uint8array_buffer(addr: usize) -> bool {
    UINT8ARRAY_FROM_CTOR.with(|r| r.borrow().contains(&addr))
}

/// Allocate a buffer with the given capacity
pub fn buffer_alloc(capacity: u32) -> *mut BufferHeader {
    // Fast path: small buffers come from a per-thread bump slab (no malloc,
    // no HashSet insert).  Large buffers fall through to the existing malloc path.
    if capacity < SMALL_BUF_THRESHOLD {
        return buffer_alloc_small(capacity);
    }
    let layout = buffer_layout(capacity as usize);
    unsafe {
        let ptr = alloc(layout) as *mut BufferHeader;
        if ptr.is_null() {
            panic!("Failed to allocate buffer");
        }
        (*ptr).length = 0;
        (*ptr).capacity = capacity;
        register_buffer(ptr);
        ptr
    }
}

/// Get the data pointer for a buffer
fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    unsafe {
        (buf as *const u8).add(std::mem::size_of::<BufferHeader>())
    }
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    unsafe {
        (buf as *mut u8).add(std::mem::size_of::<BufferHeader>())
    }
}

/// Create a Buffer from a string
/// encoding: 0 = utf8 (default), 1 = hex, 2 = base64
#[no_mangle]
pub extern "C" fn js_buffer_from_string(str_ptr: *const StringHeader, encoding: i32) -> *mut BufferHeader {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(data_ptr, len);

        match encoding {
            1 => {
                // Hex encoding
                let decoded = decode_hex(str_bytes);
                let buf = buffer_alloc(decoded.len() as u32);
                (*buf).length = decoded.len() as u32;
                ptr::copy_nonoverlapping(decoded.as_ptr(), buffer_data_mut(buf), decoded.len());
                buf
            }
            2 => {
                // Base64 encoding
                let decoded = decode_base64(str_bytes);
                let buf = buffer_alloc(decoded.len() as u32);
                (*buf).length = decoded.len() as u32;
                ptr::copy_nonoverlapping(decoded.as_ptr(), buffer_data_mut(buf), decoded.len());
                buf
            }
            _ => {
                // UTF-8 (default)
                let buf = buffer_alloc(len as u32);
                (*buf).length = len as u32;
                ptr::copy_nonoverlapping(str_bytes.as_ptr(), buffer_data_mut(buf), len);
                buf
            }
        }
    }
}

/// Map a JS string value (NaN-boxed, pointer-tagged, or raw `*const StringHeader`
/// bitcast to f64) to the integer encoding tag expected by `js_buffer_from_string`
/// and `js_buffer_to_string`:
/// - 0 = utf8 / utf-8 / ascii / latin1 / binary (fallback default)
/// - 1 = hex
/// - 2 = base64 / base64url
///
/// Used by codegen for non-literal encoding arguments to `Buffer.from(str, enc)`
/// and `buf.toString(enc)` where the encoding expression cannot be statically
/// resolved to a string literal.
#[no_mangle]
pub extern "C" fn js_encoding_tag_from_value(value: f64) -> i32 {
    let str_ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        // Cap at a reasonable upper bound to avoid pathological reads on garbage inputs.
        if len == 0 || len > 32 {
            return 0;
        }
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        // Case-insensitive compare against known encoding names.
        // Avoid heap allocation: compare byte-by-byte with ASCII lowercase fold.
        fn eq_ascii_lower(a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() { return false; }
            a.iter().zip(b.iter()).all(|(x, y)| x.to_ascii_lowercase() == *y)
        }
        if eq_ascii_lower(bytes, b"hex") {
            1
        } else if eq_ascii_lower(bytes, b"base64") || eq_ascii_lower(bytes, b"base64url") {
            2
        } else {
            // utf8, utf-8, ascii, latin1, binary, and unknown all fall through to UTF-8.
            // Matches the runtime's `_ =>` arm in js_buffer_from_string/js_buffer_to_string.
            0
        }
    }
}

/// Create a Buffer from a value (auto-detects string vs array vs buffer)
/// This is used by Buffer.from() which accepts multiple input types.
#[no_mangle]
pub extern "C" fn js_buffer_from_value(value: i64, encoding: i32) -> *mut BufferHeader {
    let bits = value as u64;
    let jsval = crate::JSValue::from_bits(bits);

    // Check if it's a NaN-boxed string
    if jsval.is_string() {
        let str_ptr = jsval.as_string_ptr();
        return js_buffer_from_string(str_ptr as *const crate::string::StringHeader, encoding);
    }

    // Extract the raw pointer
    let ptr = if bits >> 48 >= 0x7FF8 {
        // NaN-boxed pointer
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };

    if ptr < 0x1000 {
        return buffer_alloc(0);
    }

    // Check if it's a buffer (copy it)
    if is_registered_buffer(ptr) {
        let src = ptr as *const BufferHeader;
        unsafe {
            let len = (*src).length;
            let buf = buffer_alloc(len);
            (*buf).length = len;
            std::ptr::copy_nonoverlapping(buffer_data(src), buffer_data_mut(buf), len as usize);
            buf
        }
    } else {
        // Assume it's an array of numbers
        js_buffer_from_array(ptr as *const ArrayHeader)
    }
}

/// Create a Buffer from an array of numbers
#[no_mangle]
pub extern "C" fn js_buffer_from_array(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    // Strip NaN-boxing tags: if upper 16 bits are nonzero, this is a NaN-boxed value.
    // Valid heap pointers on macOS ARM64 have upper 16 bits = 0.
    let arr_ptr = if (arr_ptr as u64) >> 48 != 0 {
        ((arr_ptr as u64) & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
    } else {
        arr_ptr
    };
    if arr_ptr.is_null() || (arr_ptr as usize) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*arr_ptr).length as usize;
        let buf = buffer_alloc(len as u32);
        (*buf).length = len as u32;

        let arr_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let buf_data = buffer_data_mut(buf);

        for i in 0..len {
            let val = *arr_data.add(i);
            // Array elements may be NaN-boxed INT32, raw f64 numbers, or
            // NaN-boxed pointers/strings (rare for byte literals). Decode
            // numeric kinds; non-numeric values become 0.
            let bits = val.to_bits();
            let top16 = bits >> 48;
            let byte = if top16 == 0x7FFE {
                // INT32_TAG: lower 32 bits are an i32
                ((bits as u32) & 0xFF) as u8
            } else if top16 < 0x7FF8 || (top16 == 0x7FF8 && bits == 0x7FF8_0000_0000_0000) {
                // Raw double — convert via i64 to handle negatives correctly
                ((val as i64) & 0xFF) as u8
            } else {
                0
            };
            *buf_data.add(i) = byte;
        }

        buf
    }
}

/// `new Uint8Array(arr)` — same as `js_buffer_from_array` but additionally
/// marks the resulting buffer so it formats as `Uint8Array(N) [ ... ]`.
#[no_mangle]
pub extern "C" fn js_uint8array_from_array(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    let buf = js_buffer_from_array(arr_ptr);
    mark_as_uint8array(buf as usize);
    buf
}

/// `new Uint8Array(length)` — zero-filled buffer marked as Uint8Array.
#[no_mangle]
pub extern "C" fn js_uint8array_alloc(length: i32) -> *mut BufferHeader {
    let length = length.max(0) as u32;
    let buf = buffer_alloc(length);
    unsafe { (*buf).length = length; }
    mark_as_uint8array(buf as usize);
    buf
}

/// `new Uint8Array(x)` runtime dispatch.
///
/// The codegen can't always statically distinguish `new Uint8Array(n)` (numeric
/// length) from `new Uint8Array(arr)` (source array) when `n` is not a literal,
/// so this entry point inspects the NaN-box tag on the incoming value and
/// routes accordingly. Before this helper the catch-all codegen arm always
/// called `js_uint8array_from_array`, which treated numeric lengths as
/// `ArrayHeader*` and silently produced a zero-length buffer (closes #38).
#[no_mangle]
pub extern "C" fn js_uint8array_new(val: f64) -> *mut BufferHeader {
    let bits = val.to_bits();
    let top16 = (bits >> 48) as u16;
    // POINTER_TAG (0x7FFD) — an object/array pointer. Treat as source array.
    if top16 == 0x7FFD {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
        return js_uint8array_from_array(ptr);
    }
    // Plain IEEE double (upper16 < 0x7FFC or > 0x7FFF) — numeric length.
    if top16 < 0x7FFC || top16 > 0x7FFF {
        let len = if val.is_finite() && val >= 0.0 {
            val as i32
        } else {
            0
        };
        return js_uint8array_alloc(len);
    }
    // Any other tag (undefined/null/bool/string/bigint) → empty buffer,
    // matching the JS semantics of `new Uint8Array(undefined)` et al.
    js_uint8array_alloc(0)
}

/// Allocate a zero-filled buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc(size: i32, fill: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
        let data = buffer_data_mut(buf);
        ptr::write_bytes(data, fill as u8, size as usize);
    }
    buf
}

/// Fill an existing buffer with a byte value. Returns the same buffer pointer.
/// Implements Uint8Array.prototype.fill(value)
#[no_mangle]
pub extern "C" fn js_buffer_fill(buf: *mut BufferHeader, value: i32) -> *mut BufferHeader {
    if buf.is_null() || (buf as u64) < 0x1000 {
        return buf;
    }
    // Strip NaN-boxing tags if present
    let buf = {
        let bits = buf as u64;
        let top16 = (bits >> 48) as u16;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut BufferHeader
        } else {
            buf
        }
    };
    unsafe {
        let len = (*buf).length as usize;
        let data = buffer_data_mut(buf);
        ptr::write_bytes(data, value as u8, len);
    }
    buf
}

/// Allocate an uninitialized buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc_unsafe(size: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
    }
    buf
}

/// Concatenate multiple buffers
#[no_mangle]
pub extern "C" fn js_buffer_concat(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    // Strip NaN-boxing tags if present
    let arr_ptr = {
        let bits = arr_ptr as u64;
        let top16 = (bits >> 48) as u16;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr_ptr
        }
    };
    if arr_ptr.is_null() || (arr_ptr as u64) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*arr_ptr).length as usize;
        let arr_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Helper to strip NaN-boxing tags from buffer element pointers
        let strip_nanbox = |bits: u64| -> u64 {
            let top16 = (bits >> 48) as u16;
            if top16 >= 0x7FF8 {
                bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                bits
            }
        };

        // Calculate total size
        let mut total_size: usize = 0;
        for i in 0..len {
            let raw_bits = strip_nanbox((*arr_data.add(i)).to_bits());
            let buf_ptr = raw_bits as *const BufferHeader;
            if !buf_ptr.is_null() && raw_bits >= 0x1000 {
                total_size += (*buf_ptr).length as usize;
            }
        }

        // Allocate result buffer
        let result = buffer_alloc(total_size as u32);
        (*result).length = total_size as u32;

        // Copy data
        let mut offset: usize = 0;
        for i in 0..len {
            let raw_bits = strip_nanbox((*arr_data.add(i)).to_bits());
            let buf_ptr = raw_bits as *const BufferHeader;
            if !buf_ptr.is_null() && raw_bits >= 0x1000 {
                let buf_len = (*buf_ptr).length as usize;
                let src_data = buffer_data(buf_ptr);
                let dst_data = buffer_data_mut(result).add(offset);
                ptr::copy_nonoverlapping(src_data, dst_data, buf_len);
                offset += buf_len;
            }
        }

        result
    }
}

/// Check if an object is a Buffer (using the buffer registry)
#[no_mangle]
pub extern "C" fn js_buffer_is_buffer(ptr: i64) -> i32 {
    if ptr == 0 || (ptr as u64) < 0x1000 {
        return 0;
    }
    // Strip NaN-boxing tags if present
    let addr = if ((ptr as u64) >> 48) != 0 {
        (ptr as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        ptr as u64
    };
    if is_registered_buffer(addr as usize) { 1 } else { 0 }
}

/// Get the byte length of a string (when encoded to UTF-8)
#[no_mangle]
pub extern "C" fn js_buffer_byte_length(str_ptr: *const StringHeader) -> i32 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe {
        (*str_ptr).byte_len as i32
    }
}

/// Convert a buffer slice to a string. Honors the optional `start`/`end`
/// range (Node semantics: `start` clamped to `[0, len]`, `end` clamped to
/// `[start, len]`; defaults are `start=0, end=len`).
///
/// `encoding`: 0 = utf8 (default), 1 = hex, 2 = base64.
#[no_mangle]
pub extern "C" fn js_buffer_to_string_range(
    buf_ptr: *const BufferHeader,
    encoding: i32,
    start: i32,
    end: i32,
) -> *mut StringHeader {
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(ptr::null(), 0);
    }

    unsafe {
        let len = (*buf_ptr).length as i32;
        let s = start.max(0).min(len);
        let e = end.max(s).min(len);
        let slice_len = (e - s) as usize;
        let data = buffer_data(buf_ptr).add(s as usize);
        let bytes = std::slice::from_raw_parts(data, slice_len);

        match encoding {
            1 => {
                let hex = encode_hex(bytes);
                js_string_from_bytes(hex.as_ptr(), hex.len() as u32)
            }
            2 => {
                let b64 = encode_base64(bytes);
                js_string_from_bytes(b64.as_ptr(), b64.len() as u32)
            }
            _ => js_string_from_bytes(data, slice_len as u32),
        }
    }
}

/// Convert a buffer to a string
/// encoding: 0 = utf8 (default), 1 = hex, 2 = base64
#[no_mangle]
pub extern "C" fn js_buffer_to_string(buf_ptr: *const BufferHeader, encoding: i32) -> *mut StringHeader {
    // Strip NaN-boxing tags if present so callers can pass an i64 that came
    // from `bitcast double → i64` without unboxing first. The LLVM backend
    // NaN-boxes Buffer pointers with POINTER_TAG (0x7FFD), and the dispatch
    // path in `js_value_to_string_with_encoding` below passes the raw bits
    // straight through.
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(ptr::null(), 0);
    }

    unsafe {
        let len = (*buf_ptr).length as usize;
        let data = buffer_data(buf_ptr);
        let bytes = std::slice::from_raw_parts(data, len);

        match encoding {
            1 => {
                // Hex encoding
                let hex = encode_hex(bytes);
                js_string_from_bytes(hex.as_ptr(), hex.len() as u32)
            }
            2 => {
                // Base64 encoding
                let b64 = encode_base64(bytes);
                js_string_from_bytes(b64.as_ptr(), b64.len() as u32)
            }
            _ => {
                // UTF-8 (default)
                js_string_from_bytes(data, len as u32)
            }
        }
    }
}

/// Universal `.toString(encoding?)` dispatch used by the LLVM backend's
/// `lower_call.rs` for chained `.toString(arg)` calls where the receiver
/// type is not statically known.
///
/// - If the receiver is a registered Buffer (POINTER_TAG-boxed or raw),
///   route to `js_buffer_to_string` with the encoding tag.
/// - Otherwise fall through to `js_jsvalue_to_string` (encoding ignored,
///   matches Node behavior for non-Buffer values like numbers/objects).
///
/// `enc_tag` is the i32 produced by `js_encoding_tag_from_value` (or a
/// compile-time-folded literal): 0 = utf8, 1 = hex, 2 = base64.
#[no_mangle]
pub extern "C" fn js_value_to_string_with_encoding(value: f64, enc_tag: i32) -> *mut StringHeader {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    // Extract the underlying pointer regardless of NaN-box presence:
    //   - POINTER_TAG (0x7FFD) → strip top 16 bits
    //   - raw pointer bitcast to f64 → use bits directly (top16 == 0)
    let ptr_addr = if top16 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits >= 0x1000 {
        bits as usize
    } else {
        0
    };
    if ptr_addr != 0 && is_registered_buffer(ptr_addr) {
        return js_buffer_to_string(ptr_addr as *const BufferHeader, enc_tag);
    }
    crate::value::js_jsvalue_to_string(value)
}

/// Print a buffer in Node.js `<Buffer xx xx ...>` format to stdout
#[no_mangle]
pub extern "C" fn js_buffer_print(buf_ptr: *const BufferHeader) {
    if buf_ptr.is_null() {
        println!("<Buffer >");
        return;
    }
    unsafe {
        let len = (*buf_ptr).length as usize;
        let data = buffer_data(buf_ptr);
        let bytes = std::slice::from_raw_parts(data, len);
        let mut out = String::with_capacity(9 + len * 3);
        out.push_str("<Buffer");
        for (i, b) in bytes.iter().enumerate() {
            if i == 0 {
                out.push(' ');
            } else {
                out.push(' ');
            }
            out.push_str(&format!("{:02x}", b));
        }
        out.push('>');
        println!("{}", out);
    }
}

/// Get the length of a buffer
#[no_mangle]
pub extern "C" fn js_buffer_length(buf_ptr: *const BufferHeader) -> i32 {
    // Strip NaN-boxing tags if present (POINTER_TAG-boxed buffer pointers).
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe { (*buf_ptr).length as i32 }
}

/// Get a byte at the specified index
#[no_mangle]
pub extern "C" fn js_buffer_get(buf_ptr: *const BufferHeader, index: i32) -> i32 {
    if buf_ptr.is_null() || index < 0 {
        return 0;
    }
    unsafe {
        if index as u32 >= (*buf_ptr).length {
            return 0;
        }
        let data = buffer_data(buf_ptr);
        *data.add(index as usize) as i32
    }
}

/// Set a byte at the specified index
#[no_mangle]
pub extern "C" fn js_buffer_set(buf_ptr: *mut BufferHeader, index: i32, value: i32) {
    if buf_ptr.is_null() || index < 0 {
        return;
    }
    unsafe {
        if index as u32 >= (*buf_ptr).length {
            return;
        }
        let data = buffer_data_mut(buf_ptr);
        *data.add(index as usize) = (value & 0xFF) as u8;
    }
}

/// Copy bytes from source buffer into target buffer at given offset.
/// Implements Uint8Array.prototype.set(source, offset)
#[no_mangle]
pub extern "C" fn js_buffer_set_from(target: *mut BufferHeader, source: *const BufferHeader, offset: i32) {
    if target.is_null() || source.is_null() || offset < 0 {
        return;
    }
    // Strip NaN-boxing tags
    let target = {
        let bits = target as u64;
        if (bits >> 48) >= 0x7FF8 { (bits & 0x0000_FFFF_FFFF_FFFF) as *mut BufferHeader } else { target }
    };
    let source = {
        let bits = source as u64;
        if (bits >> 48) >= 0x7FF8 { (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader } else { source }
    };
    if target.is_null() || source.is_null() { return; }
    unsafe {
        let target_len = (*target).length as usize;
        let source_len = (*source).length as usize;
        let off = offset as usize;
        if off + source_len > target_len { return; } // Would overflow
        let target_data = buffer_data_mut(target);
        let source_data = buffer_data(source);
        ptr::copy_nonoverlapping(source_data, target_data.add(off), source_len);
    }
}

/// Create a slice of a buffer (returns a new buffer)
#[no_mangle]
pub extern "C" fn js_buffer_slice(buf_ptr: *const BufferHeader, start: i32, end: i32) -> *mut BufferHeader {
    if buf_ptr.is_null() {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*buf_ptr).length as i32;

        // Handle negative indices
        let start = if start < 0 { (len + start).max(0) } else { start.min(len) };
        let end = if end < 0 { (len + end).max(0) } else { end.min(len) };

        if start >= end {
            return buffer_alloc(0);
        }

        let slice_len = (end - start) as u32;
        let result = buffer_alloc(slice_len);
        (*result).length = slice_len;

        let src_data = buffer_data(buf_ptr).add(start as usize);
        let dst_data = buffer_data_mut(result);
        ptr::copy_nonoverlapping(src_data, dst_data, slice_len as usize);

        result
    }
}

/// Copy data from source buffer to target buffer
/// Returns the number of bytes copied
#[no_mangle]
pub extern "C" fn js_buffer_copy(
    src_ptr: *const BufferHeader,
    dst_ptr: *mut BufferHeader,
    target_start: i32,
    source_start: i32,
    source_end: i32,
) -> i32 {
    if src_ptr.is_null() || dst_ptr.is_null() {
        return 0;
    }

    unsafe {
        let src_len = (*src_ptr).length as i32;
        let dst_len = (*dst_ptr).length as i32;

        let target_start = target_start.max(0).min(dst_len);
        let source_start = source_start.max(0).min(src_len);
        let source_end = if source_end < 0 { src_len } else { source_end.min(src_len) };

        if source_start >= source_end {
            return 0;
        }

        let copy_len = (source_end - source_start).min(dst_len - target_start);
        if copy_len <= 0 {
            return 0;
        }

        let src_data = buffer_data(src_ptr).add(source_start as usize);
        let dst_data = buffer_data_mut(dst_ptr).add(target_start as usize);
        ptr::copy_nonoverlapping(src_data, dst_data, copy_len as usize);

        copy_len
    }
}

/// Write a string to a buffer
/// Returns the number of bytes written
#[no_mangle]
pub extern "C" fn js_buffer_write(
    buf_ptr: *mut BufferHeader,
    str_ptr: *const StringHeader,
    offset: i32,
    encoding: i32,
) -> i32 {
    if buf_ptr.is_null() || str_ptr.is_null() {
        return 0;
    }

    unsafe {
        let buf_len = (*buf_ptr).length as i32;
        let offset = offset.max(0).min(buf_len);

        let str_len = (*str_ptr).byte_len as usize;
        let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(str_data, str_len);

        let bytes_to_write = match encoding {
            1 => decode_hex(str_bytes),
            2 => decode_base64(str_bytes),
            _ => str_bytes.to_vec(),
        };

        let available = (buf_len - offset) as usize;
        let write_len = bytes_to_write.len().min(available);

        let dst_data = buffer_data_mut(buf_ptr).add(offset as usize);
        ptr::copy_nonoverlapping(bytes_to_write.as_ptr(), dst_data, write_len);

        write_len as i32
    }
}

/// Strip POINTER_TAG NaN-box bits from a buffer-pointer-like u64. Returns
/// the raw heap address as usize. Returns 0 if the input is below the heap.
fn unbox_buffer_ptr(bits: u64) -> usize {
    let top16 = bits >> 48;
    let raw = if top16 >= 0x7FF8 {
        bits & 0x0000_FFFF_FFFF_FFFF
    } else {
        bits
    };
    if raw < 0x1000 { 0 } else { raw as usize }
}

/// Compare two buffers for equality
#[no_mangle]
pub extern "C" fn js_buffer_equals(buf1_ptr: *const BufferHeader, buf2_ptr: *const BufferHeader) -> i32 {
    let p1 = unbox_buffer_ptr(buf1_ptr as u64) as *const BufferHeader;
    let p2 = unbox_buffer_ptr(buf2_ptr as u64) as *const BufferHeader;
    if p1.is_null() && p2.is_null() {
        return 1;
    }
    if p1.is_null() || p2.is_null() {
        return 0;
    }

    unsafe {
        let len1 = (*p1).length;
        let len2 = (*p2).length;

        if len1 != len2 {
            return 0;
        }

        let data1 = buffer_data(p1);
        let data2 = buffer_data(p2);

        for i in 0..len1 as usize {
            if *data1.add(i) != *data2.add(i) {
                return 0;
            }
        }

        1
    }
}

/// Lexicographic compare of two buffers (Buffer.compare semantics).
/// Returns -1, 0, or 1 (i32).
#[no_mangle]
pub extern "C" fn js_buffer_compare(a: *const BufferHeader, b: *const BufferHeader) -> i32 {
    let pa = unbox_buffer_ptr(a as u64) as *const BufferHeader;
    let pb = unbox_buffer_ptr(b as u64) as *const BufferHeader;
    if pa.is_null() && pb.is_null() { return 0; }
    if pa.is_null() { return -1; }
    if pb.is_null() { return 1; }
    unsafe {
        let la = (*pa).length as usize;
        let lb = (*pb).length as usize;
        let da = std::slice::from_raw_parts(buffer_data(pa), la);
        let db = std::slice::from_raw_parts(buffer_data(pb), lb);
        match da.cmp(db) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Search for a byte sequence in a buffer.
fn buffer_index_of_bytes(buf: *const BufferHeader, needle: &[u8], start: i32) -> i32 {
    if buf.is_null() { return -1; }
    unsafe {
        let len = (*buf).length as usize;
        let data = std::slice::from_raw_parts(buffer_data(buf), len);
        let from = if start < 0 {
            ((len as i32) + start).max(0) as usize
        } else {
            (start as usize).min(len)
        };
        if needle.is_empty() {
            return from as i32;
        }
        if needle.len() > len.saturating_sub(from) {
            return -1;
        }
        for i in from..=(len - needle.len()) {
            if &data[i..i + needle.len()] == needle {
                return i as i32;
            }
        }
        -1
    }
}

/// `buf.indexOf(needle, start?)` where `needle` is a string, buffer,
/// or numeric byte value (NaN-boxed value).
#[no_mangle]
pub extern "C" fn js_buffer_index_of(buf_ptr: f64, needle: f64, start: i32) -> i32 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    if buf.is_null() { return -1; }
    let needle_bits = needle.to_bits();
    let top16 = needle_bits >> 48;

    // Buffer needle (POINTER_TAG-boxed or raw)
    let raw_ptr = if top16 >= 0x7FF8 {
        (needle_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && needle_bits >= 0x1000 {
        needle_bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        let other = raw_ptr as *const BufferHeader;
        let needle_slice = unsafe {
            std::slice::from_raw_parts(buffer_data(other), (*other).length as usize)
        };
        return buffer_index_of_bytes(buf, needle_slice, start);
    }
    // String needle (STRING_TAG-boxed)
    if top16 == 0x7FFF {
        let str_ptr = (needle_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !str_ptr.is_null() {
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                return buffer_index_of_bytes(buf, bytes, start);
            }
        }
    }
    // Numeric byte needle — INT32_TAG or plain double
    let byte_val = if top16 == 0x7FFE {
        // INT32_TAG: lower 32 bits are an i32
        (needle_bits as u32) & 0xFF
    } else if top16 < 0x7FF8 || (top16 == 0x7FF8 && needle_bits == 0x7FF8_0000_0000_0000) {
        // Raw double — convert to byte
        ((needle as i64) & 0xFF) as u32
    } else {
        return -1;
    };
    let byte = [byte_val as u8];
    buffer_index_of_bytes(buf, &byte, start)
}

/// `buf.includes(needle, start?)` — boolean i32.
#[no_mangle]
pub extern "C" fn js_buffer_includes(buf_ptr: f64, needle: f64, start: i32) -> i32 {
    if js_buffer_index_of(buf_ptr, needle, start) >= 0 { 1 } else { 0 }
}

/// `crypto.getRandomValues(buf)` — fill an existing buffer with random
/// bytes in-place. Returns the same buffer pointer.
#[no_mangle]
pub extern "C" fn js_buffer_fill_random(buf_ptr: f64) -> f64 {
    use rand::RngCore;
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() { return buf_ptr; }
    unsafe {
        let len = (*buf).length as usize;
        let data = buffer_data_mut(buf);
        let mut bytes = std::slice::from_raw_parts_mut(data, len);
        rand::thread_rng().fill_bytes(&mut bytes);
    }
    buf_ptr
}

/// `buf.swap16()` — pairs of bytes are swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap16(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() { return; }
    unsafe {
        let len = (*buf).length as usize;
        if len % 2 != 0 { return; }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(2) {
            let a = *data.add(i);
            *data.add(i) = *data.add(i + 1);
            *data.add(i + 1) = a;
        }
    }
}

/// `buf.swap32()` — groups of 4 bytes byte-swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap32(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() { return; }
    unsafe {
        let len = (*buf).length as usize;
        if len % 4 != 0 { return; }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(4) {
            let b0 = *data.add(i);
            let b1 = *data.add(i + 1);
            let b2 = *data.add(i + 2);
            let b3 = *data.add(i + 3);
            *data.add(i) = b3;
            *data.add(i + 1) = b2;
            *data.add(i + 2) = b1;
            *data.add(i + 3) = b0;
        }
    }
}

/// `buf.swap64()` — groups of 8 bytes byte-swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap64(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() { return; }
    unsafe {
        let len = (*buf).length as usize;
        if len % 8 != 0 { return; }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(8) {
            for j in 0..4 {
                let a = *data.add(i + j);
                *data.add(i + j) = *data.add(i + 7 - j);
                *data.add(i + 7 - j) = a;
            }
        }
    }
}

// ---------------------------------------------------------------------
// Numeric read/write helpers
// ---------------------------------------------------------------------

#[inline]
fn buffer_slice_at<'a>(buf: *const BufferHeader, offset: i32, n: usize) -> Option<&'a [u8]> {
    if buf.is_null() || offset < 0 { return None; }
    unsafe {
        let len = (*buf).length as usize;
        let off = offset as usize;
        if off.checked_add(n)? > len { return None; }
        Some(std::slice::from_raw_parts(buffer_data(buf).add(off), n))
    }
}

#[inline]
fn buffer_slice_at_mut<'a>(buf: *mut BufferHeader, offset: i32, n: usize) -> Option<&'a mut [u8]> {
    if buf.is_null() || offset < 0 { return None; }
    unsafe {
        let len = (*buf).length as usize;
        let off = offset as usize;
        if off.checked_add(n)? > len { return None; }
        Some(std::slice::from_raw_parts_mut(buffer_data_mut(buf).add(off), n))
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint8(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 1) { Some(s) => s[0] as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int8(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 1) { Some(s) => (s[0] as i8) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint16_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) { Some(s) => u16::from_be_bytes([s[0], s[1]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint16_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) { Some(s) => u16::from_le_bytes([s[0], s[1]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int16_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) { Some(s) => i16::from_be_bytes([s[0], s[1]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int16_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) { Some(s) => i16::from_le_bytes([s[0], s[1]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint32_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint32_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int32_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => i32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int32_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_float_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => f32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_float_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) { Some(s) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64, None => 0.0 }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_double_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 8) {
        Some(s) => f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_double_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 8) {
        Some(s) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint8(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 1) { s[0] = (value as i64 & 0xFF) as u8; }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int8(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint8(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint16_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 2) {
        let bytes = (value as i64 as u16).to_be_bytes();
        s[0] = bytes[0]; s[1] = bytes[1];
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint16_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 2) {
        let bytes = (value as i64 as u16).to_le_bytes();
        s[0] = bytes[0]; s[1] = bytes[1];
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int16_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint16_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int16_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint16_le(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint32_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as i64 as u32).to_be_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint32_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as i64 as u32).to_le_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int32_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint32_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int32_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint32_le(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_float_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as f32).to_be_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_float_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as f32).to_le_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_double_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&value.to_be_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_double_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&value.to_le_bytes());
    }
}

// ---- BigInt 64-bit read/write ----

#[no_mangle]
pub extern "C" fn js_buffer_read_bigint64_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => i64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_bigint64_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => i64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_biguint64_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as i64,
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_biguint64_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as i64,
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_write_bigint64_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let val = bigint_value_to_i64(value);
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&val.to_be_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_bigint64_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let val = bigint_value_to_i64(value);
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&val.to_le_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_biguint64_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_bigint64_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_biguint64_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_bigint64_le(buf_ptr, value, offset);
}

fn bigint_value_to_i64(value: f64) -> i64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    // BigInt pointers can carry either BIGINT_TAG (0x7FFA) or — when the
    // codegen folds them through the generic `nanbox_pointer_inline` path
    // (Expr::BigInt) — POINTER_TAG (0x7FFD). Both encode the lower 48 bits
    // as the heap address. Detect either and use `clean_bigint_ptr` to
    // strip and validate the address before reading the limb.
    if top16 >= 0x7FF8 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
        let cleaned = crate::bigint::clean_bigint_ptr(ptr);
        if cleaned.is_null() { return 0; }
        unsafe { (*cleaned).limbs[0] as i64 }
    } else if value.is_finite() {
        value as i64
    } else {
        0
    }
}

// Helper functions for encoding/decoding

fn decode_hex(input: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len() / 2);
    let mut i = 0;
    while i + 1 < input.len() {
        let high = hex_char_to_value(input[i]);
        let low = hex_char_to_value(input[i + 1]);
        if high < 16 && low < 16 {
            result.push((high << 4) | low);
        }
        i += 2;
    }
    result
}

fn hex_char_to_value(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 16, // Invalid
    }
}

fn encode_hex(input: &[u8]) -> Vec<u8> {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut result = Vec::with_capacity(input.len() * 2);
    for &byte in input {
        result.push(HEX_CHARS[(byte >> 4) as usize]);
        result.push(HEX_CHARS[(byte & 0xF) as usize]);
    }
    result
}

fn decode_base64(input: &[u8]) -> Vec<u8> {
    // Simple base64 decoder
    const DECODE_TABLE: [u8; 256] = {
        let mut table = [64u8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i;
            table[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = i + 52;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut buf_bits: u32 = 0;

    for &byte in input {
        if byte == b'=' {
            break;
        }
        let val = DECODE_TABLE[byte as usize];
        if val == 64 {
            continue; // Skip invalid characters
        }
        buf = (buf << 6) | val as u32;
        buf_bits += 6;

        if buf_bits >= 8 {
            buf_bits -= 8;
            result.push((buf >> buf_bits) as u8);
            buf &= (1 << buf_bits) - 1;
        }
    }

    result
}

fn encode_base64(input: &[u8]) -> Vec<u8> {
    const ENCODE_TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = Vec::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;

    while i + 2 < input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        result.push(ENCODE_TABLE[(n >> 18) as usize]);
        result.push(ENCODE_TABLE[((n >> 12) & 0x3F) as usize]);
        result.push(ENCODE_TABLE[((n >> 6) & 0x3F) as usize]);
        result.push(ENCODE_TABLE[(n & 0x3F) as usize]);
        i += 3;
    }

    if i + 1 < input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        result.push(ENCODE_TABLE[(n >> 18) as usize]);
        result.push(ENCODE_TABLE[((n >> 12) & 0x3F) as usize]);
        result.push(ENCODE_TABLE[((n >> 6) & 0x3F) as usize]);
        result.push(b'=');
    } else if i < input.len() {
        let n = (input[i] as u32) << 16;
        result.push(ENCODE_TABLE[(n >> 18) as usize]);
        result.push(ENCODE_TABLE[((n >> 12) & 0x3F) as usize]);
        result.push(b'=');
        result.push(b'=');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_buffer_slab_unique_addresses() {
        // Every allocation must occupy a distinct address (no overlap).
        let n = 1000usize;
        let mut ptrs: Vec<*mut BufferHeader> = Vec::new();
        for i in 0..n {
            let cap = (i % (SMALL_BUF_THRESHOLD as usize)) as u32;
            let buf = buffer_alloc(cap);
            assert!(!buf.is_null(), "slab alloc returned null at i={}", i);
            ptrs.push(buf);
        }
        let addrs: HashSet<usize> = ptrs.iter().map(|&p| p as usize).collect();
        assert_eq!(addrs.len(), n, "slab allocations must have unique addresses");
    }

    #[test]
    fn test_small_buffer_slab_is_registered() {
        // All slab-allocated buffers must be recognised as buffers.
        for cap in [0u32, 1, 15, 16, 127, 255] {
            let buf = buffer_alloc(cap);
            assert!(
                is_registered_buffer(buf as usize),
                "cap={cap}: slab buffer not recognised by is_registered_buffer"
            );
            assert_eq!(
                unsafe { (*buf).capacity }, cap,
                "cap={cap}: wrong capacity stored in header"
            );
        }
    }

    #[test]
    fn test_large_buffer_still_registered() {
        // Buffers at or above the threshold still go through the HashSet path.
        let buf = buffer_alloc(SMALL_BUF_THRESHOLD);
        assert!(!buf.is_null());
        assert!(
            is_registered_buffer(buf as usize),
            "large buffer not in BUFFER_REGISTRY"
        );
        assert_eq!(unsafe { (*buf).capacity }, SMALL_BUF_THRESHOLD);
    }

    #[test]
    fn test_buffer_alloc() {
        let buf = js_buffer_alloc(10, 0);
        assert_eq!(js_buffer_length(buf), 10);
        for i in 0..10 {
            assert_eq!(js_buffer_get(buf, i), 0);
        }
    }

    #[test]
    fn test_buffer_alloc_with_fill() {
        let buf = js_buffer_alloc(5, 0x42);
        assert_eq!(js_buffer_length(buf), 5);
        for i in 0..5 {
            assert_eq!(js_buffer_get(buf, i), 0x42);
        }
    }

    #[test]
    fn test_buffer_get_set() {
        let buf = js_buffer_alloc(5, 0);
        js_buffer_set(buf, 2, 0x42);
        assert_eq!(js_buffer_get(buf, 2), 0x42);
    }

    #[test]
    fn test_hex_encode_decode() {
        let original = b"Hello";
        let encoded = encode_hex(original);
        let decoded = decode_hex(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_encode_decode() {
        let original = b"Hello, World!";
        let encoded = encode_base64(original);
        let decoded = decode_base64(&encoded);
        assert_eq!(decoded, original);
    }
}
