use super::*;

#[derive(Clone, Copy)]
enum BufferSetSource {
    Buffer(*const BufferHeader),
    TypedArray(*const crate::typedarray::TypedArrayHeader),
    Array(*const ArrayHeader),
    Object(*const crate::object::ObjectHeader),
    Empty,
}

#[inline]
fn js_value_to_number(value: f64) -> f64 {
    crate::value::JSValue::from_bits(value.to_bits()).to_number()
}

#[inline]
fn to_integer_or_zero(value: f64) -> i64 {
    let number = js_value_to_number(value);
    if number.is_nan() {
        0
    } else if number == f64::INFINITY {
        i64::MAX
    } else if number == f64::NEG_INFINITY {
        i64::MIN
    } else {
        number.trunc() as i64
    }
}

#[inline]
fn to_uint8(value: f64) -> u8 {
    let number = js_value_to_number(value);
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    (number.trunc() as i64).rem_euclid(256) as u8
}

#[inline]
fn pointer_addr_from_value(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        return Some((bits & crate::value::POINTER_MASK) as usize);
    }

    // A few runtime paths pass heap pointers as raw f64 bit patterns. Only
    // accept those when a dedicated registry can prove the address is binary
    // data; object/array fallback requires the normal POINTER_TAG form.
    if top16 == 0 {
        let addr = bits as usize;
        if crate::buffer::is_registered_buffer(addr)
            || crate::typedarray::lookup_typed_array_kind(addr).is_some()
        {
            return Some(addr);
        }
    }

    None
}

#[inline]
fn gc_type_at(addr: usize) -> Option<u8> {
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    if !crate::object::is_valid_obj_ptr(addr as *const u8) {
        return None;
    }
    unsafe {
        let header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        Some((*header).obj_type)
    }
}

fn decode_buffer_set_source(value: f64) -> BufferSetSource {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if js_value.is_null() || js_value.is_undefined() {
        crate::node_submodules::diagnostics::throw_type_error_no_code(
            b"Cannot convert undefined or null to object",
        );
    }

    let Some(addr) = pointer_addr_from_value(value) else {
        return BufferSetSource::Empty;
    };

    if crate::buffer::is_registered_buffer(addr) {
        return BufferSetSource::Buffer(addr as *const BufferHeader);
    }

    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return BufferSetSource::TypedArray(addr as *const crate::typedarray::TypedArrayHeader);
    }

    match gc_type_at(addr) {
        Some(crate::gc::GC_TYPE_ARRAY) => BufferSetSource::Array(addr as *const ArrayHeader),
        Some(crate::gc::GC_TYPE_OBJECT) => {
            BufferSetSource::Object(addr as *const crate::object::ObjectHeader)
        }
        _ => BufferSetSource::Empty,
    }
}

unsafe fn array_like_object_length(obj: *const crate::object::ObjectHeader) -> usize {
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let value = crate::object::js_object_get_field_by_name(obj, key);
    let number = value.to_number();
    if !number.is_finite() || number <= 0.0 {
        0
    } else {
        number.floor() as usize
    }
}

unsafe fn buffer_set_source_len(source: BufferSetSource) -> usize {
    match source {
        BufferSetSource::Buffer(ptr) => {
            if ptr.is_null() {
                0
            } else {
                (*ptr).length as usize
            }
        }
        BufferSetSource::TypedArray(ptr) => {
            crate::typedarray::js_typed_array_length(ptr).max(0) as usize
        }
        BufferSetSource::Array(ptr) => crate::array::js_array_length(ptr) as usize,
        BufferSetSource::Object(ptr) => array_like_object_length(ptr),
        BufferSetSource::Empty => 0,
    }
}

unsafe fn collect_buffer_set_bytes(source: BufferSetSource, source_len: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(source_len);
    match source {
        BufferSetSource::Buffer(ptr) => {
            for i in 0..source_len {
                bytes.push(js_buffer_get(ptr, i as i32) as u8);
            }
        }
        BufferSetSource::TypedArray(ptr) => {
            if let Some(kind) = crate::typedarray::lookup_typed_array_kind(ptr as usize) {
                if crate::typedarray::bigint::is_bigint_kind(kind) {
                    crate::typedarray::bigint::throw_bigint_number_mix();
                }
            }
            for i in 0..source_len {
                bytes.push(to_uint8(crate::typedarray::js_typed_array_get(
                    ptr, i as i32,
                )));
            }
        }
        BufferSetSource::Array(ptr) => {
            for i in 0..source_len {
                bytes.push(to_uint8(crate::array::js_array_get_f64(ptr, i as u32)));
            }
        }
        BufferSetSource::Object(ptr) => {
            for i in 0..source_len {
                let key = i.to_string();
                let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let value = crate::object::js_object_get_field_by_name(ptr, key_ptr);
                bytes.push(to_uint8(f64::from_bits(value.bits())));
            }
        }
        BufferSetSource::Empty => {}
    }
    bytes
}

/// Read the byte at `index`, resolving a registered view to its ultimate
/// backing buffer. Returns `None` for a null receiver or an out-of-range
/// index (`index < 0` or `index >= length`). Shared by the native i32
/// accessor (`js_buffer_get`) and the JS-value accessor
/// (`js_buffer_index_get_value`) so their read semantics never drift.
#[inline]
unsafe fn read_buffer_byte(buf_ptr: *const BufferHeader, index: i32) -> Option<u8> {
    if buf_ptr.is_null() || index < 0 || index as u32 >= (*buf_ptr).length {
        return None;
    }
    // Issue #1205: if the receiver is a registered view, read from
    // the ultimate backing buffer — otherwise the view's local
    // snapshot can lag any direct-fast-path write made to the
    // backing through codegen.
    let buf_addr = buf_ptr as usize;
    if let Some(info) = super::view::lookup(buf_addr) {
        let back_off = info.offset + index as u32;
        let backing_ptr = info.backing as *const BufferHeader;
        if !backing_ptr.is_null() && back_off < (*backing_ptr).length {
            let back_data = buffer_data(backing_ptr);
            return Some(*back_data.add(back_off as usize));
        }
    }
    let data = buffer_data(buf_ptr);
    Some(*data.add(index as usize))
}

/// Get a byte at the specified index. Native i32 accessor: an out-of-range
/// index yields the `0` sentinel because every caller here has proven the
/// index in bounds or consumes the byte in a native integer context. A
/// JS-value `buf[i]` read must instead yield `undefined` for out-of-range —
/// use `js_buffer_index_get_value` for that (#6088).
#[no_mangle]
pub extern "C" fn js_buffer_get(buf_ptr: *const BufferHeader, index: i32) -> i32 {
    unsafe { read_buffer_byte(buf_ptr, index).map_or(0, |byte| byte as i32) }
}

/// `buf[i]` / `uint8array[i]` read as a JS value. Perry's `Uint8Array` is
/// Buffer-backed, so both share this accessor. An out-of-range canonical
/// integer index reads `undefined` — the ECMAScript IntegerIndexedExotic
/// `[[Get]]` semantics — NOT the `0` byte-sentinel of the native
/// `js_buffer_get` (#6088). Negative and fractional keys never reach here
/// (codegen routes them to the dynamic-key helper); an in-range read returns
/// the byte as a plain (non-NaN) f64, which is its own NaN-boxed JS number.
#[no_mangle]
pub extern "C" fn js_buffer_index_get_value(buf_ptr: *const BufferHeader, index: i32) -> f64 {
    match unsafe { read_buffer_byte(buf_ptr, index) } {
        Some(byte) => byte as f64,
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

// #6088: force-keep the JS-value buffer index getter under LTO /
// auto-optimize. It has zero internal Rust callers — codegen emits the only
// call (in `perry-codegen/src/expr/index_get.rs`), so a whole-program bitcode
// link is otherwise free to internalize and dead-strip it. The `#[used]`
// anchor pins it (mirrors `KEEP_JS_TYPED_ARRAY_INDEX_GET_DYNAMIC`).
#[used]
static KEEP_JS_BUFFER_INDEX_GET_VALUE: extern "C" fn(*const BufferHeader, i32) -> f64 =
    js_buffer_index_get_value;

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
        let byte = (value & 0xFF) as u8;
        // Write the byte to the receiver's own data area first so a
        // direct codegen fast-path read of this buffer still sees the
        // update.
        let data = buffer_data_mut(buf_ptr);
        *data.add(index as usize) = byte;
        // Issue #1205: propagate through the view registry.  If the
        // receiver is itself a slice, mirror the write into the
        // ultimate backing buffer; in either direction, sister views
        // covering the same backing byte must observe the new value.
        let buf_addr = buf_ptr as usize;
        if let Some(info) = super::view::lookup(buf_addr) {
            let back_off = info.offset + index as u32;
            let backing_ptr = info.backing as *mut BufferHeader;
            if !backing_ptr.is_null() && back_off < (*backing_ptr).length {
                let back_data = buffer_data_mut(backing_ptr);
                *back_data.add(back_off as usize) = byte;
                super::view::propagate_byte_to_views(info.backing, back_off, byte, buf_addr);
            }
        } else {
            super::view::propagate_byte_to_views(buf_addr, index as u32, byte, buf_addr);
        }
    }
}

/// Copy bytes from source buffer into target buffer at given offset.
/// Implements Uint8Array.prototype.set(source, offset)
#[no_mangle]
pub extern "C" fn js_buffer_set_from(
    target: *mut BufferHeader,
    source: *const BufferHeader,
    offset: i32,
) {
    if target.is_null() || source.is_null() || offset < 0 {
        return;
    }
    // Strip NaN-boxing tags
    let target = {
        let bits = target as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut BufferHeader
        } else {
            target
        }
    };
    let source = {
        let bits = source as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            source
        }
    };
    if target.is_null() || source.is_null() {
        return;
    }
    let source_value = f64::from_bits(crate::value::JSValue::pointer(source as *const u8).bits());
    js_buffer_set_from_value(target, source_value, offset as f64);
}

/// Copy array-like or typed-array bytes into a Buffer/Uint8Array receiver.
/// Implements the Uint8Array.prototype.set(source, offset) behavior used by
/// Buffer instances and BufferHeader-backed Uint8Array values.
#[no_mangle]
pub extern "C" fn js_buffer_set_from_value(
    target: *mut BufferHeader,
    source_value: f64,
    offset_value: f64,
) -> f64 {
    if target.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let target = {
        let bits = target as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & crate::value::POINTER_MASK) as *mut BufferHeader
        } else {
            target
        }
    };
    if target.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let offset = to_integer_or_zero(offset_value);
    let source = decode_buffer_set_source(source_value);

    unsafe {
        let target_len = (*target).length as usize;
        let source_len = buffer_set_source_len(source);
        if offset < 0 {
            super::numeric::throw_out_of_range();
        }
        let offset = offset as usize;
        if offset
            .checked_add(source_len)
            .is_none_or(|end| end > target_len)
        {
            super::numeric::throw_out_of_range();
        }

        let bytes = collect_buffer_set_bytes(source, source_len);
        if !bytes.is_empty() {
            let target_data = buffer_data_mut(target).add(offset);
            ptr::copy_nonoverlapping(bytes.as_ptr(), target_data, bytes.len());
            super::view::propagate_written_range_from_receiver(
                target as usize,
                offset as u32,
                target_data,
                bytes.len() as u32,
            );
        }
    }

    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Create a slice of a buffer.  Issue #1205: the returned buffer is
/// a *view* over the source — registered in the view registry so that
/// subsequent reads/writes via the runtime helpers propagate between
/// the slice and the original.
#[no_mangle]
pub extern "C" fn js_buffer_slice(
    buf_ptr: *const BufferHeader,
    start: i32,
    end: i32,
) -> *mut BufferHeader {
    if buf_ptr.is_null() {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*buf_ptr).length as i32;

        // Handle negative indices
        let start = if start < 0 {
            (len + start).max(0)
        } else {
            start.min(len)
        };
        let end = if end < 0 {
            (len + end).max(0)
        } else {
            end.min(len)
        };

        if start >= end {
            return buffer_alloc(0);
        }

        let slice_len = (end - start) as u32;
        let result = buffer_alloc(slice_len);
        (*result).length = slice_len;

        let src_data = buffer_data(buf_ptr).add(start as usize);
        let dst_data = buffer_data_mut(result);
        ptr::copy_nonoverlapping(src_data, dst_data, slice_len as usize);

        // Register the alias.  `register` flattens slices-of-slices
        // so the recorded backing is always the original allocation.
        super::view::register(result as usize, buf_ptr as usize, start as u32, slice_len);

        result
    }
}
