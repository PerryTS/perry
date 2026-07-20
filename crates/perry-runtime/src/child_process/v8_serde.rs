//! V8 structured-clone wire-format codec for `child_process` advanced IPC (#2130).
//!
//! When `fork(modulePath, args, { serialization: 'advanced' })` is used, Node's
//! IPC channel switches from newline-delimited JSON to V8's `ValueSerializer`
//! format (the same bytes `v8.serialize` / `v8.deserialize` produce). Because a
//! Perry-forked child runs under the real `node` binary (see [`super::fork`]),
//! Perry's *parent* side must speak that exact format so the child's
//! `process.send` / `process.on('message')` interoperate.
//!
//! Wire shape (verified against Node v25 `v8.serialize`):
//!   * Header: `0xFF` then the format version as an unsigned LEB128 varint
//!     (version 15 today).
//!   * Each value is a one-byte tag optionally followed by tag-specific data.
//!   * Integers use zig-zag LEB128; doubles are little-endian `f64`.
//!   * **Buffers and TypedArrays are written as *host objects*** (tag `\`,
//!     `0x5C`) — `v8.DefaultSerializer` sets `treatArrayBufferViewsAsHostObjects`,
//!     so the payload is `varint(typeIndex) varint(byteLength) rawBytes` rather
//!     than the native `kArrayBufferView` framing. The type index matches Node's
//!     `arrayBufferViewTypes` table (Buffer = 10, see [`v8_index_for_kind`]).
//!
//! On the message-framing layer (handled in [`super::reactor`]) each serialized
//! payload is prefixed with a 4-byte big-endian length, matching Node's
//! `writeChannelMessage` / `parseChannelMessages`.
//!
//! GC is suppressed for the whole encode/decode (as `JSON.parse` does): the
//! walk holds raw `ObjectHeader*` / `ArrayHeader*` across allocations, so a
//! moving collection mid-codec would invalidate them. IPC messages are small
//! and bounded, so a brief suppression window is safe.
//!
//! Coverage: undefined, null, booleans, int32/double numbers, strings
//! (one-byte / two-byte), dense + sparse arrays, plain objects, `Date`, BigInt,
//! `Buffer`, and all TypedArray kinds. Functions / symbols and other
//! un-cloneable values serialize as `undefined` (Node would throw; we degrade
//! gracefully rather than abort the channel). The read side honors
//! `kObjectReference`, so cyclic / shared graphs sent by a node child round-trip.

use super::*;

const V8_LATEST_VERSION: u32 = 15;

// Serialization tags (subset of v8 `SerializationTag` we emit/handle).
const TAG_VERSION: u8 = 0xFF;
const TAG_PADDING: u8 = b'\0';
const TAG_UNDEFINED: u8 = b'_';
const TAG_THE_NULL: u8 = b'0';
const TAG_TRUE: u8 = b'T';
const TAG_FALSE: u8 = b'F';
const TAG_INT32: u8 = b'I';
const TAG_UINT32: u8 = b'U';
const TAG_DOUBLE: u8 = b'N';
const TAG_BIGINT: u8 = b'Z';
const TAG_UTF8_STRING: u8 = b'S';
const TAG_ONE_BYTE_STRING: u8 = b'"';
const TAG_TWO_BYTE_STRING: u8 = b'c';
const TAG_OBJECT_REFERENCE: u8 = b'^';
const TAG_BEGIN_JS_OBJECT: u8 = b'o';
const TAG_END_JS_OBJECT: u8 = b'{';
const TAG_BEGIN_SPARSE_ARRAY: u8 = b'a';
const TAG_END_SPARSE_ARRAY: u8 = b'@';
const TAG_BEGIN_DENSE_ARRAY: u8 = b'A';
const TAG_END_DENSE_ARRAY: u8 = b'$';
const TAG_DATE: u8 = b'D';
const TAG_ARRAY_BUFFER: u8 = b'B';
const TAG_HOST_OBJECT: u8 = b'\\';

const NODE_BUFFER_VIEW_INDEX: u64 = 10;

/// Node's child_process host-object discriminator for a plain `ArrayBufferView`
/// (vs. a transferred OS handle). Always `0` for Buffers / TypedArrays.
const HOST_VIEW_DISCRIMINATOR: u64 = 0;

/// RAII guard mirroring `JSON.parse`'s GC-suppression window: raw heap pointers
/// held across the codec must not be relocated by a moving collection.
struct GcSuppressGuard;
impl GcSuppressGuard {
    fn new() -> Self {
        crate::gc::gc_suppress();
        GcSuppressGuard
    }
}
impl Drop for GcSuppressGuard {
    fn drop(&mut self) {
        crate::gc::gc_unsuppress();
        crate::gc::gc_bump_malloc_trigger();
    }
}

/// Node `arrayBufferViewTypes` index for a Perry typed-array `KIND_*`. The
/// ordering is Node's, NOT Perry's: Buffer is inserted at index 10, pushing the
/// BigInt views to 11/12. Verified empirically against `v8.serialize`.
fn v8_index_for_kind(kind: u8) -> u64 {
    use crate::typedarray::*;
    match kind {
        KIND_INT8 => 0,
        KIND_UINT8 => 1,
        KIND_UINT8_CLAMPED => 2,
        KIND_INT16 => 3,
        KIND_UINT16 => 4,
        KIND_INT32 => 5,
        KIND_UINT32 => 6,
        KIND_FLOAT32 => 7,
        KIND_FLOAT64 => 8,
        // 9 = DataView, 10 = Buffer (handled separately)
        KIND_BIGINT64 => 11,
        KIND_BIGUINT64 => 12,
        _ => 1,
    }
}

/// Inverse of [`v8_index_for_kind`]; `None` for Buffer (10), DataView (9), or an
/// unknown index — the caller builds those specially.
fn kind_for_v8_index(idx: u64) -> Option<u8> {
    use crate::typedarray::*;
    Some(match idx {
        0 => KIND_INT8,
        1 => KIND_UINT8,
        2 => KIND_UINT8_CLAMPED,
        3 => KIND_INT16,
        4 => KIND_UINT16,
        5 => KIND_INT32,
        6 => KIND_UINT32,
        7 => KIND_FLOAT32,
        8 => KIND_FLOAT64,
        11 => KIND_BIGINT64,
        12 => KIND_BIGUINT64,
        _ => return None,
    })
}

// ============================================================
// Serializer
// ============================================================

struct Serializer {
    out: Vec<u8>,
    depth: u32,
}

const MAX_DEPTH: u32 = 512;

impl Serializer {
    fn new() -> Self {
        Serializer {
            out: Vec::with_capacity(64),
            depth: 0,
        }
    }

    fn write_header(&mut self) {
        self.out.push(TAG_VERSION);
        self.write_varint(V8_LATEST_VERSION as u64);
    }

    fn write_varint(&mut self, mut value: u64) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn write_zigzag(&mut self, value: i64) {
        let zz = ((value << 1) ^ (value >> 63)) as u64;
        self.write_varint(zz);
    }

    fn write_double(&mut self, value: f64) {
        self.out.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    fn write_value(&mut self, value: f64) {
        let bits = value.to_bits();
        let jsval = JSValue::from_bits(bits);

        if jsval.is_undefined() {
            self.out.push(TAG_UNDEFINED);
            return;
        }
        if jsval.is_null() {
            self.out.push(TAG_THE_NULL);
            return;
        }
        if jsval.is_bool() {
            self.out
                .push(if jsval.as_bool() { TAG_TRUE } else { TAG_FALSE });
            return;
        }
        if jsval.is_int32() {
            self.out.push(TAG_INT32);
            self.write_zigzag(jsval.as_int32() as i64);
            return;
        }
        if jsval.is_number() {
            self.write_number(value);
            return;
        }
        if jsval.is_any_string() {
            self.write_string(value);
            return;
        }
        if jsval.is_bigint() {
            self.write_bigint(value);
            return;
        }
        if jsval.is_pointer() {
            let raw = (bits & crate::value::POINTER_MASK) as usize;
            if raw >= 0x10000 {
                if crate::buffer::is_registered_buffer(raw) {
                    self.write_host_buffer(value);
                    return;
                }
                if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw) {
                    self.write_host_typed_array(value, kind);
                    return;
                }
                if crate::date::is_date_value(value) {
                    self.out.push(TAG_DATE);
                    self.write_double(crate::date::js_date_get_time(value));
                    return;
                }
                if let Some(arr) = cp_array_ptr(value) {
                    self.write_dense_array(arr);
                    return;
                }
                if let Some(obj) = cp_object_ptr(value) {
                    self.write_object(obj);
                    return;
                }
            }
        }
        // Functions, symbols, unknown — degrade to undefined.
        self.out.push(TAG_UNDEFINED);
    }

    fn write_number(&mut self, value: f64) {
        // Emit a compact int32 when the double is an exact, non-negative-zero
        // integer in i32 range — matching V8's Smi fast path.
        if value.fract() == 0.0
            && value >= i32::MIN as f64
            && value <= i32::MAX as f64
            && value.to_bits() != 0x8000_0000_0000_0000
        {
            self.out.push(TAG_INT32);
            self.write_zigzag(value as i64);
        } else {
            self.out.push(TAG_DOUBLE);
            self.write_double(value);
        }
    }

    fn write_string(&mut self, value: f64) {
        let bytes = string_bytes(value);
        if bytes.iter().all(|&b| b < 0x80) {
            // Pure ASCII: a one-byte string is byte-identical in latin1.
            self.out.push(TAG_ONE_BYTE_STRING);
            self.write_varint(bytes.len() as u64);
            self.out.extend_from_slice(&bytes);
        } else {
            // Non-ASCII: re-encode UTF-8 → UTF-16LE so a node child reconstructs
            // the exact code units (one-byte strings are latin1-only).
            let s = String::from_utf8_lossy(&bytes);
            let mut utf16 = Vec::with_capacity(bytes.len());
            for u in s.encode_utf16() {
                utf16.extend_from_slice(&u.to_le_bytes());
            }
            self.out.push(TAG_TWO_BYTE_STRING);
            self.write_varint(utf16.len() as u64);
            self.out.extend_from_slice(&utf16);
        }
    }

    fn write_bigint(&mut self, value: f64) {
        let ptr = JSValue::from_bits(value.to_bits()).as_bigint_ptr();
        let negative = unsafe { crate::bigint::js_bigint_is_negative(ptr) } != 0;
        // Read the magnitude as big-endian bytes (negate first if needed), then
        // reverse to the little-endian order V8's bigint digits use.
        let mag_ptr = if negative {
            unsafe { crate::bigint::js_bigint_neg(ptr) as *const crate::bigint::BigIntHeader }
        } else {
            ptr
        };
        let nbytes = crate::bigint::BIGINT_LIMBS * 8;
        let be_buf = unsafe { crate::bigint::js_bigint_to_buffer(mag_ptr, nbytes as i32) };
        let mut le: Vec<u8> = if be_buf.is_null() {
            Vec::new()
        } else {
            let data = crate::buffer::buffer_data(be_buf);
            let len = unsafe { (*be_buf).length } as usize;
            let mut v = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
            v.reverse();
            v
        };
        while le.last() == Some(&0) {
            le.pop();
        }
        // V8 stores bigint digits as 64-bit words.
        let word_bytes = le.len().div_ceil(8) * 8;
        le.resize(word_bytes, 0);
        let bitfield = ((word_bytes as u64) << 1) | (negative as u64);
        self.out.push(TAG_BIGINT);
        self.write_varint(bitfield);
        self.out.extend_from_slice(&le);
    }

    /// Write a Node `ArrayBufferView` host object. Node's child_process
    /// serializer (`ChildProcessSerializer._writeHostObject`) prefixes the view
    /// with a `0` discriminator (distinguishing it from a transferred OS
    /// handle), then `varint(typeIndex) varint(byteLength) rawBytes`. Verified
    /// against `internal/child_process/serialization.js`'s on-wire output.
    fn write_host_view(&mut self, type_index: u64, bytes: &[u8]) {
        self.out.push(TAG_HOST_OBJECT);
        self.write_varint(HOST_VIEW_DISCRIMINATOR);
        self.write_varint(type_index);
        self.write_varint(bytes.len() as u64);
        self.out.extend_from_slice(bytes);
    }

    fn write_host_buffer(&mut self, value: f64) {
        let data = crate::buffer::js_native_buffer_data_ptr(value);
        let len = crate::buffer::js_native_buffer_byte_len(value);
        let bytes: &[u8] = if data.is_null() || len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        self.write_host_view(NODE_BUFFER_VIEW_INDEX, bytes);
    }

    fn write_host_typed_array(&mut self, value: f64, kind: u8) {
        let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
        let ta = raw as *const crate::typedarray::TypedArrayHeader;
        let bytes = unsafe { crate::typedarray::typed_array_bytes(ta) }.unwrap_or(&[]);
        self.write_host_view(v8_index_for_kind(kind), bytes);
    }

    fn write_dense_array(&mut self, arr: *mut crate::array::ArrayHeader) {
        if self.depth >= MAX_DEPTH {
            self.out.push(TAG_UNDEFINED);
            return;
        }
        self.depth += 1;
        let len = unsafe { (*arr).length } as usize;
        self.out.push(TAG_BEGIN_DENSE_ARRAY);
        self.write_varint(len as u64);
        for i in 0..len {
            let elem = crate::array::js_array_get_f64(arr, i as u32);
            self.write_value(elem);
        }
        self.out.push(TAG_END_DENSE_ARRAY);
        self.write_varint(0); // no extra named properties
        self.write_varint(len as u64);
        self.depth -= 1;
    }

    fn write_object(&mut self, obj: *const ObjectHeader) {
        if self.depth >= MAX_DEPTH {
            self.out.push(TAG_UNDEFINED);
            return;
        }
        self.depth += 1;
        self.out.push(TAG_BEGIN_JS_OBJECT);
        let count = unsafe { self.write_object_fields(obj) };
        self.out.push(TAG_END_JS_OBJECT);
        self.write_varint(count);
        self.depth -= 1;
    }

    /// Walk an object's own enumerable fields allocation-free, mirroring the
    /// JSON stringifier: names live in `keys_array`, values are positional
    /// (inline slots up to `max(field_count, 8)`, the rest via overflow).
    unsafe fn write_object_fields(&mut self, obj: *const ObjectHeader) -> u64 {
        let keys_arr = (*obj).keys_array;
        if keys_arr.is_null() {
            return 0;
        }
        let keys_len = (*keys_arr).length;
        let num_fields = (*obj).field_count;
        let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
        let fields_ptr = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const f64;
        let mut count = 0u64;
        for f in 0..keys_len {
            let key = crate::array::js_array_get_f64(keys_arr, f);
            let val = if f < alloc_limit {
                *fields_ptr.add(f as usize)
            } else {
                f64::from_bits(crate::object::js_object_get_field(obj, f).bits())
            };
            self.write_value(key);
            self.write_value(val);
            count += 1;
        }
        count
    }
}

/// Serialize a JS value to a V8 structured-clone payload (with header).
pub(crate) fn v8_serialize(value: f64) -> Vec<u8> {
    let _gc = GcSuppressGuard::new();
    let mut ser = Serializer::new();
    ser.write_header();
    ser.write_value(value);
    ser.out
}

// ============================================================
// Deserializer
// ============================================================

struct Deserializer<'a> {
    buf: &'a [u8],
    pos: usize,
    /// Objects in V8 id-assignment order, for `kObjectReference`.
    id_table: Vec<f64>,
}

impl<'a> Deserializer<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Deserializer {
            buf,
            pos: 0,
            id_table: Vec::new(),
        }
    }

    fn read_byte(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn read_varint(&mut self) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_byte()?;
            result |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
        Some(result)
    }

    fn read_zigzag(&mut self) -> Option<i64> {
        let v = self.read_varint()?;
        Some(((v >> 1) as i64) ^ -((v & 1) as i64))
    }

    fn read_double(&mut self) -> Option<f64> {
        let b = self.read_raw(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(b);
        Some(f64::from_bits(u64::from_le_bytes(arr)))
    }

    fn read_raw(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.pos.checked_add(len)? > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Some(s)
    }

    fn read_header(&mut self) {
        if self.peek_byte() == Some(TAG_VERSION) {
            self.pos += 1;
            let _ = self.read_varint(); // version — accept any
        }
    }

    fn read_value(&mut self) -> Option<f64> {
        loop {
            let tag = self.read_byte()?;
            return Some(match tag {
                TAG_PADDING | TAG_VERSION => continue,
                TAG_UNDEFINED => cp_undefined(),
                TAG_THE_NULL => TAG_NULL_F64,
                TAG_TRUE => TAG_TRUE_F64,
                TAG_FALSE => TAG_FALSE_F64,
                TAG_INT32 => int_to_value(self.read_zigzag()?),
                TAG_UINT32 => int_to_value(self.read_varint()? as i64),
                TAG_DOUBLE => self.read_double()?,
                TAG_ONE_BYTE_STRING => self.read_one_byte_string()?,
                TAG_UTF8_STRING => self.read_utf8_string()?,
                TAG_TWO_BYTE_STRING => self.read_two_byte_string()?,
                TAG_DATE => {
                    let ms = self.read_double()?;
                    let d = crate::date::js_date_new_from_timestamp(ms);
                    self.id_table.push(d);
                    d
                }
                TAG_BIGINT => self.read_bigint()?,
                TAG_BEGIN_JS_OBJECT => self.read_object()?,
                TAG_BEGIN_DENSE_ARRAY => self.read_dense_array()?,
                TAG_BEGIN_SPARSE_ARRAY => self.read_sparse_array()?,
                TAG_ARRAY_BUFFER => self.read_array_buffer()?,
                TAG_HOST_OBJECT => self.read_host_object()?,
                TAG_OBJECT_REFERENCE => {
                    let id = self.read_varint()? as usize;
                    self.id_table.get(id).copied().unwrap_or_else(cp_undefined)
                }
                _ => cp_undefined(),
            });
        }
    }

    fn read_one_byte_string(&mut self) -> Option<f64> {
        let len = self.read_varint()? as usize;
        let bytes = self.read_raw(len)?;
        // latin1 → UTF-8.
        let s: String = bytes.iter().map(|&b| b as char).collect();
        Some(cp_box_string(&s))
    }

    fn read_utf8_string(&mut self) -> Option<f64> {
        let len = self.read_varint()? as usize;
        let bytes = self.read_raw(len)?;
        Some(cp_box_string_bytes(bytes))
    }

    fn read_two_byte_string(&mut self) -> Option<f64> {
        let len = self.read_varint()? as usize;
        let bytes = self.read_raw(len)?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&units);
        Some(cp_box_string(&s))
    }

    fn read_bigint(&mut self) -> Option<f64> {
        let bitfield = self.read_varint()?;
        let negative = (bitfield & 1) != 0;
        let byte_len = (bitfield >> 1) as usize;
        let bytes = self.read_raw(byte_len)?;
        // Exact for magnitudes up to 64 bits; wider values keep the low 64.
        let mut mag: u64 = 0;
        for (i, &b) in bytes.iter().take(8).enumerate() {
            mag |= (b as u64) << (i * 8);
        }
        let ptr = if negative {
            crate::bigint::js_bigint_from_i64((mag as i64).wrapping_neg())
        } else {
            crate::bigint::js_bigint_from_u64(mag)
        };
        Some(crate::value::js_nanbox_bigint(ptr as i64))
    }

    fn read_object(&mut self) -> Option<f64> {
        let obj = crate::object::js_object_alloc(0, 0);
        let boxed = cp_box_ptr(obj as *const u8);
        self.id_table.push(boxed);
        loop {
            if self.peek_byte() == Some(TAG_END_JS_OBJECT) {
                self.pos += 1;
                self.read_varint()?; // property count
                break;
            }
            let key = self.read_value()?;
            let val = self.read_value()?;
            let key_bytes = string_bytes(key);
            js_object_set_field_by_name(
                obj,
                js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32),
                val,
            );
        }
        Some(boxed)
    }

    fn read_dense_array(&mut self) -> Option<f64> {
        let len = self.read_varint()? as usize;
        let arr = crate::array::js_array_alloc(len as u32);
        let boxed = cp_box_ptr(arr as *const u8);
        self.id_table.push(boxed);
        for _ in 0..len {
            let elem = self.read_value()?;
            // Capacity was reserved up front; push does not relocate.
            crate::array::js_array_push_f64(arr, elem);
        }
        if self.peek_byte() == Some(TAG_END_DENSE_ARRAY) {
            self.pos += 1;
            self.read_varint()?; // extra property count
            self.read_varint()?; // length
        }
        Some(boxed)
    }

    fn read_sparse_array(&mut self) -> Option<f64> {
        let total = self.read_varint()? as usize;
        let arr = crate::array::js_array_alloc(total as u32);
        let boxed = cp_box_ptr(arr as *const u8);
        self.id_table.push(boxed);
        loop {
            if self.peek_byte() == Some(TAG_END_SPARSE_ARRAY) {
                self.pos += 1;
                self.read_varint()?; // property count
                self.read_varint()?; // length
                break;
            }
            let key = self.read_value()?;
            let val = self.read_value()?;
            // Only integer keys land as dense indices; ignore named props.
            if JSValue::from_bits(key.to_bits()).is_number() {
                crate::array::js_array_set_f64_extend(arr, key as u32, val);
            }
        }
        Some(boxed)
    }

    fn read_array_buffer(&mut self) -> Option<f64> {
        let len = self.read_varint()? as usize;
        let bytes = self.read_raw(len)?;
        let v = cp_make_buffer(bytes);
        self.id_table.push(v);
        Some(v)
    }

    fn read_host_object(&mut self) -> Option<f64> {
        // Node child_process host objects lead with a discriminator: 0 = a plain
        // ArrayBufferView (Buffer / TypedArray), which is all Perry handles.
        let discriminator = self.read_varint()?;
        if discriminator != HOST_VIEW_DISCRIMINATOR {
            // A transferred OS handle (socket/server) — Perry can't reconstruct
            // it; the remaining frame layout is handle-specific, so bail.
            return Some(cp_undefined());
        }
        let idx = self.read_varint()?;
        let len = self.read_varint()? as usize;
        let bytes = self.read_raw(len)?;
        let v = if idx == NODE_BUFFER_VIEW_INDEX {
            cp_make_buffer(bytes)
        } else if let Some(kind) = kind_for_v8_index(idx) {
            make_typed_array(kind, bytes)
        } else {
            // DataView (9) / unknown — fall back to a Buffer of the raw bytes.
            cp_make_buffer(bytes)
        };
        self.id_table.push(v);
        Some(v)
    }
}

/// Deserialize a V8 structured-clone payload (header optional) to a JS value.
pub(crate) fn v8_deserialize(buf: &[u8]) -> f64 {
    let _gc = GcSuppressGuard::new();
    let mut de = Deserializer::new(buf);
    de.read_header();
    de.read_value().unwrap_or_else(cp_undefined)
}

// ============================================================
// Class API: `new v8.Serializer()` / `new v8.Deserializer(buf)` (#3680)
// ============================================================
//
// Node's `v8.Serializer` / `v8.Deserializer` (and the `Default*` subclasses)
// expose a stateful builder around the same structured-clone codec used by
// `v8.serialize` / `v8.deserialize`. We keep a per-thread registry of live
// instances keyed by a monotonic id; the JS-visible value is a native-module
// namespace object that carries that id in field[1] (mirrors the
// `PerformanceObserver` instance pattern). GC suppression is *not* held across
// calls (each method is a discrete JS call); instead each `write_value` /
// `read_value` wraps its own suppression window so raw heap pointers held
// during a single walk are never relocated.

use std::cell::RefCell;

thread_local! {
    static SERIALIZERS: RefCell<Vec<Serializer>> = const { RefCell::new(Vec::new()) };
    static DESERIALIZERS: RefCell<Vec<OwnedDeserializer>> = const { RefCell::new(Vec::new()) };
}

/// A `Deserializer` that owns its buffer (the codec's `Deserializer` borrows
/// `&[u8]`; the class API must keep the bytes alive across method calls).
struct OwnedDeserializer {
    buf: Vec<u8>,
    pos: usize,
    id_table: Vec<f64>,
}

impl OwnedDeserializer {
    fn with<R>(&mut self, f: impl FnOnce(&mut Deserializer) -> R) -> R {
        let mut de = Deserializer {
            buf: &self.buf,
            pos: self.pos,
            id_table: std::mem::take(&mut self.id_table),
        };
        let r = f(&mut de);
        self.pos = de.pos;
        self.id_table = std::mem::take(&mut de.id_table);
        r
    }
}

/// Allocate a fresh `Serializer` and return its registry id.
pub(crate) fn v8_class_serializer_new() -> usize {
    SERIALIZERS.with(|s| {
        let mut s = s.borrow_mut();
        s.push(Serializer::new());
        s.len() - 1
    })
}

pub(crate) fn v8_class_serializer_write_header(id: usize) {
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.write_header();
        }
    });
}

pub(crate) fn v8_class_serializer_write_value(id: usize, value: f64) {
    let _gc = GcSuppressGuard::new();
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.write_value(value);
        }
    });
}

pub(crate) fn v8_class_serializer_write_uint32(id: usize, value: u32) {
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.write_varint(value as u64);
        }
    });
}

pub(crate) fn v8_class_serializer_write_uint64(id: usize, hi: u32, lo: u32) {
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.write_varint(((hi as u64) << 32) | (lo as u64));
        }
    });
}

pub(crate) fn v8_class_serializer_write_double(id: usize, value: f64) {
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.write_double(value);
        }
    });
}

pub(crate) fn v8_class_serializer_write_raw_bytes(id: usize, bytes: &[u8]) {
    SERIALIZERS.with(|s| {
        if let Some(ser) = s.borrow_mut().get_mut(id) {
            ser.out.extend_from_slice(bytes);
        }
    });
}

/// Take a snapshot of the serializer's accumulated bytes (Node's
/// `releaseBuffer()` returns the buffer; we clone so further writes still work,
/// matching Node closely enough for the public contract).
pub(crate) fn v8_class_serializer_release(id: usize) -> Vec<u8> {
    SERIALIZERS.with(|s| {
        s.borrow()
            .get(id)
            .map(|ser| ser.out.clone())
            .unwrap_or_default()
    })
}

/// Allocate a fresh `Deserializer` over a copy of `bytes`; returns its id.
pub(crate) fn v8_class_deserializer_new(bytes: Vec<u8>) -> usize {
    DESERIALIZERS.with(|d| {
        let mut d = d.borrow_mut();
        d.push(OwnedDeserializer {
            buf: bytes,
            pos: 0,
            id_table: Vec::new(),
        });
        d.len() - 1
    })
}

pub(crate) fn v8_class_deserializer_read_header(id: usize) {
    DESERIALIZERS.with(|d| {
        if let Some(de) = d.borrow_mut().get_mut(id) {
            de.with(|inner| inner.read_header());
        }
    });
}

pub(crate) fn v8_class_deserializer_read_value(id: usize) -> f64 {
    let _gc = GcSuppressGuard::new();
    DESERIALIZERS.with(|d| {
        d.borrow_mut()
            .get_mut(id)
            .map(|de| de.with(|inner| inner.read_value().unwrap_or_else(cp_undefined)))
            .unwrap_or_else(cp_undefined)
    })
}

pub(crate) fn v8_class_deserializer_read_uint32(id: usize) -> u32 {
    DESERIALIZERS.with(|d| {
        d.borrow_mut()
            .get_mut(id)
            .map(|de| de.with(|inner| inner.read_varint().unwrap_or(0) as u32))
            .unwrap_or(0)
    })
}

/// Read a 64-bit value as Node does: `[hi, lo]` u32 pair.
pub(crate) fn v8_class_deserializer_read_uint64(id: usize) -> (u32, u32) {
    DESERIALIZERS.with(|d| {
        d.borrow_mut()
            .get_mut(id)
            .map(|de| {
                let v = de.with(|inner| inner.read_varint().unwrap_or(0));
                ((v >> 32) as u32, (v & 0xFFFF_FFFF) as u32)
            })
            .unwrap_or((0, 0))
    })
}

pub(crate) fn v8_class_deserializer_read_double(id: usize) -> f64 {
    DESERIALIZERS.with(|d| {
        d.borrow_mut()
            .get_mut(id)
            .map(|de| de.with(|inner| inner.read_double().unwrap_or(0.0)))
            .unwrap_or(0.0)
    })
}

/// Read `len` raw bytes from the deserializer's cursor.
pub(crate) fn v8_class_deserializer_read_raw_bytes(id: usize, len: usize) -> Vec<u8> {
    DESERIALIZERS.with(|d| {
        d.borrow_mut()
            .get_mut(id)
            .map(|de| de.with(|inner| inner.read_raw(len).map(|s| s.to_vec()).unwrap_or_default()))
            .unwrap_or_default()
    })
}

// ============================================================
// Helpers
// ============================================================

/// Materialize a JS string value's UTF-8 bytes (SSO-safe).
fn string_bytes(value: f64) -> Vec<u8> {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return Vec::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// Build a JS number from a deserialized integer. Returned as a plain `f64`
/// double (JS's canonical number representation) rather than an INT32-tagged
/// value — the int32/uint32 wire ranges are exact in `f64`, and downstream
/// consumers (`JSON.stringify`, console formatting) handle plain doubles
/// uniformly.
fn int_to_value(v: i64) -> f64 {
    v as f64
}

/// Build a NaN-boxed typed array of `kind` from raw little-endian bytes.
fn make_typed_array(kind: u8, bytes: &[u8]) -> f64 {
    let elem_size = crate::typedarray::elem_size_for_kind(kind);
    let length = if elem_size == 0 {
        0
    } else {
        bytes.len() / elem_size
    };
    let ta = crate::typedarray::js_typed_array_new_empty(kind as i32, length as i32);
    if ta.is_null() {
        return cp_undefined();
    }
    if let Some(dst) = unsafe { crate::typedarray::typed_array_bytes_mut(ta) } {
        let n = dst.len().min(bytes.len());
        dst[..n].copy_from_slice(&bytes[..n]);
    }
    cp_box_ptr(ta as *const u8)
}
