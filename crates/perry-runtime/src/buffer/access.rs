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
    // accept those when the allocator owns the word and its header says binary
    // data (#10694: the brand is the type byte); object/array fallback requires
    // the normal POINTER_TAG form.
    if top16 == 0 {
        let addr = bits as usize;
        if crate::buffer::header_is_owned(addr)
            && (crate::buffer::is_registered_buffer(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some())
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_const_ptr(obj);
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let value = obj.with_const_ptr(|obj| crate::object::js_object_get_field_by_name(obj, key));
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
                super::store::length(ptr as usize)
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
            let scope = crate::gc::RuntimeHandleScope::new();
            let source = scope.root_raw_const_ptr(ptr);
            for i in 0..source_len {
                let value =
                    source.with_const_ptr(|ptr| crate::array::js_array_get_f64(ptr, i as u32));
                bytes.push(to_uint8(value));
            }
        }
        BufferSetSource::Object(ptr) => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let source = scope.root_raw_const_ptr(ptr);
            for i in 0..source_len {
                let key = i.to_string();
                let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let value = source
                    .with_const_ptr(|ptr| crate::object::js_object_get_field_by_name(ptr, key_ptr));
                bytes.push(to_uint8(f64::from_bits(value.bits())));
            }
        }
        BufferSetSource::Empty => {}
    }
    bytes
}

/// Byte-width typed sources can be copied together within one NoGc scope.
fn bulk_source_is_bytes(source: BufferSetSource) -> bool {
    match source {
        BufferSetSource::Buffer(ptr) => !ptr.is_null(),
        BufferSetSource::TypedArray(ptr) => matches!(
            crate::typedarray::lookup_typed_array_kind(ptr as usize),
            Some(
                crate::typedarray::KIND_INT8
                    | crate::typedarray::KIND_UINT8
                    | crate::typedarray::KIND_UINT8_CLAMPED
            )
        ),
        _ => false,
    }
}

/// Read the byte at `index`, resolving a registered view to its ultimate
/// backing buffer. Returns `None` for a null receiver or an out-of-range
/// index (`index < 0` or `index >= length`). Shared by the native i32
/// accessor (`js_buffer_get`) and the JS-value accessor
/// (`js_buffer_index_get_value`) so their read semantics never drift.
#[inline]
unsafe fn read_buffer_byte(buf_ptr: *const BufferHeader, index: i32) -> Option<u8> {
    if buf_ptr.is_null() || index < 0 || index as usize >= super::store::length(buf_ptr as usize) {
        return None;
    }
    let data = byte_access_data(buf_ptr);
    if super::is_shared_array_buffer(super::store::owner(buf_ptr as usize)) {
        // A pointer-backed view may alias a SAB on another agent. Relaxed
        // atomic bytes preserve ordinary shared-memory reads without allowing
        // the optimizer to treat their contents as loop-invariant.
        return Some(
            (*(data.add(index as usize) as *const std::sync::atomic::AtomicU8))
                .load(std::sync::atomic::Ordering::Relaxed),
        );
    }
    Some(*data.add(index as usize))
}

/// Resolve the current owner store immediately before a leaf element access.
#[inline]
pub(crate) unsafe fn byte_access_data(buf_ptr: *const BufferHeader) -> *mut u8 {
    super::store::data(buf_ptr as usize)
}

#[inline(always)]
pub(crate) fn admitted_u8_read(addr: usize, index: i32) -> Option<u8> {
    let obj_type = super::header::byte_cell_type(addr)?;
    if !matches!(
        obj_type & !0x20,
        crate::gc::GC_TYPE_BUFFER | crate::gc::GC_TYPE_BUFFER_UINT8ARRAY
    ) {
        return None;
    }
    unsafe { read_buffer_byte(addr as *const BufferHeader, index) }
}

/// Could `addr` be a cell `admitted_u8_read` / `admitted_u8_write` serve (a
/// Node `Buffer` owner or view)? One header load and one compare, so a
/// dispatcher can keep every other receiver off the out-of-line byte arm. A
/// filter only: the arm itself proves the cell before reading it.
#[inline(always)]
pub(crate) fn is_admitted_u8_cell(addr: usize) -> bool {
    unsafe { crate::value::addr_class::try_read_gc_header(addr) }
        .is_some_and(|header| header.obj_type & !0x20 == crate::gc::GC_TYPE_BUFFER)
}

#[inline(always)]
pub(crate) fn admitted_u8_write(addr: usize, index: i32, byte: u8) -> bool {
    let Some(obj_type) = super::header::byte_cell_type(addr) else {
        return false;
    };
    if !matches!(
        obj_type & !0x20,
        crate::gc::GC_TYPE_BUFFER | crate::gc::GC_TYPE_BUFFER_UINT8ARRAY
    ) {
        return false;
    }
    if index < 0 || index as usize >= unsafe { super::store::length(addr) } {
        return false;
    }
    js_buffer_set(addr as *mut BufferHeader, index, byte as i32);
    true
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
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BUFFER_INDEX_GET_VALUE: extern "C" fn(*const BufferHeader, i32) -> f64 =
    js_buffer_index_get_value;

/// Set a byte at the specified index
#[no_mangle]
pub extern "C" fn js_buffer_set(buf_ptr: *mut BufferHeader, index: i32, value: i32) {
    if buf_ptr.is_null() || index < 0 {
        return;
    }
    unsafe {
        if index as usize >= super::store::length(buf_ptr as usize) {
            return;
        }
        let byte = (value & 0xFF) as u8;
        let data = byte_access_data(buf_ptr);
        if super::is_shared_array_buffer(super::store::owner(buf_ptr as usize)) {
            (*(data.add(index as usize) as *const std::sync::atomic::AtomicU8))
                .store(byte, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        *data.add(index as usize) = byte;
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

    let roots = crate::gc::RuntimeHandleScope::new();
    let target_root = roots.root_raw_mut_ptr(target);
    let source_root = roots.root_nanbox_u64(source_value.to_bits());
    let offset = to_integer_or_zero(offset_value);
    let source = decode_buffer_set_source(f64::from_bits(source_root.get_nanbox_u64()));

    unsafe {
        let target_len = (super::store::length(target as usize) as u32) as usize;
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

        if bulk_source_is_bytes(source) {
            let source_value = f64::from_bits(source_root.get_nanbox_u64());
            let copied = super::bytes::no_gc(|_| {
                let src = super::bytes::span(source_value, false)?;
                let dst = super::bytes::span(
                    crate::value::js_nanbox_pointer(
                        target_root.get_raw_mut_ptr::<BufferHeader>() as i64
                    ),
                    true,
                )?;
                if source_len > src.len
                    || offset
                        .checked_add(source_len)
                        .is_none_or(|end| end > dst.len)
                {
                    return Ok(false);
                }
                // Aliased JS views require memmove, without overlapping Rust borrows.
                ptr::copy(src.ptr, dst.ptr.add(offset), source_len);
                Ok::<_, super::bytes::NotBytes>(true)
            });
            match copied {
                Ok(true) => (),
                Ok(false) => super::numeric::throw_out_of_range(),
                Err(super::bytes::NotBytes::Frozen) => {
                    crate::typedarray::throw_type_error(b"Cannot modify a frozen typed array")
                }
                Err(_) => crate::typedarray::throw_type_error(
                    b"Cannot perform set on a detached ArrayBuffer",
                ),
            }
        } else {
            let copied = collect_buffer_set_bytes(
                decode_buffer_set_source(f64::from_bits(source_root.get_nanbox_u64())),
                source_len,
            );
            let written = super::bytes::no_gc(|scope| {
                super::bytes::bytes_mut(
                    crate::value::js_nanbox_pointer(
                        target_root.get_raw_mut_ptr::<BufferHeader>() as i64
                    ),
                    scope,
                )
                .map(|dst| {
                    if offset
                        .checked_add(copied.len())
                        .is_some_and(|end| end <= dst.len())
                    {
                        dst[offset..offset + copied.len()].copy_from_slice(&copied);
                        true
                    } else {
                        false
                    }
                })
            });
            if written != Ok(true) {
                super::numeric::throw_out_of_range();
            }
        }
    }

    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Create a shared Buffer slice / Uint8Array subarray in constant space.
#[no_mangle]
pub extern "C" fn js_buffer_slice(
    buf_ptr: *const BufferHeader,
    start: i32,
    end: i32,
) -> *mut BufferHeader {
    if buf_ptr.is_null() {
        return buffer_alloc(0);
    }
    let (start, length) = slice_bounds(buf_ptr, start, end);
    let brand = if is_uint8array_buffer(buf_ptr as usize) {
        crate::gc::GC_TYPE_BUFFER_UINT8ARRAY
    } else {
        crate::gc::GC_TYPE_BUFFER
    };
    let result = super::store::new_view(brand, buf_ptr as usize, start, length, false);
    result
}

fn slice_bounds(buf: *const BufferHeader, start: i32, end: i32) -> (u32, u32) {
    let len = unsafe { (super::store::length(buf as usize) as u32) as i64 };
    let bound = |index: i32| {
        if index < 0 {
            (len + index as i64).max(0)
        } else {
            (index as i64).min(len)
        }
    };
    let start = bound(start);
    (start as u32, (bound(end) - start).max(0) as u32)
}

/// ArrayBuffer/SharedArrayBuffer and Uint8Array `.slice()` copy their bytes.
/// Buffer `.slice()` alone has the shared semantics of `.subarray()`.
pub(crate) fn buffer_slice_copy(
    buf: *const BufferHeader,
    start: i32,
    end: i32,
) -> *mut BufferHeader {
    let (start, length) = slice_bounds(buf, start, end);
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_raw_const_ptr(buf);
    let brand = if is_array_buffer(buf as usize) {
        crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER
    } else if is_shared_array_buffer(buf as usize) {
        crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER
    } else if is_uint8array_buffer(buf as usize) {
        crate::gc::GC_TYPE_BUFFER_UINT8ARRAY
    } else {
        crate::gc::GC_TYPE_BUFFER
    };
    let result = super::store::store_alloc(brand, length, super::store::Init::Uninit);
    unsafe {
        super::store::set_length(result as usize, length);
        super::bytes::no_gc(|scope| {
            let src = super::bytes::bytes(
                crate::value::js_nanbox_pointer(source.get_raw_const_ptr::<BufferHeader>() as i64),
                scope,
            )
            .unwrap();
            let dst =
                super::bytes::bytes_mut(crate::value::js_nanbox_pointer(result as i64), scope)
                    .unwrap();
            dst.copy_from_slice(&src[start as usize..start as usize + length as usize]);
        });
    }
    result
}
