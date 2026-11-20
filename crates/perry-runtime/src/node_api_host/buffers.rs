use super::*;
use crate::buffer::BufferHeader;
use crate::typedarray_hook_slots::TypedArrayHeader;
use crate::value::JSValue;
use std::ffi::c_void;
use std::ptr;

/// `napi_typedarray_type`, numbered as js_native_api_types.h numbers it
/// (not as Perry numbers its element kinds).
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiTypedarrayType {
    Int8Array = 0,
    Uint8Array = 1,
    Uint8ClampedArray = 2,
    Int16Array = 3,
    Uint16Array = 4,
    Int32Array = 5,
    Uint32Array = 6,
    Float32Array = 7,
    Float64Array = 8,
    Bigint64Array = 9,
    Biguint64Array = 10,
    Float16Array = 11,
}

impl NapiTypedarrayType {
    fn from_element_kind(kind: u8) -> Self {
        use crate::typedarray::*;
        match kind {
            KIND_INT8 => Self::Int8Array,
            KIND_UINT8 => Self::Uint8Array,
            KIND_UINT8_CLAMPED => Self::Uint8ClampedArray,
            KIND_INT16 => Self::Int16Array,
            KIND_UINT16 => Self::Uint16Array,
            KIND_INT32 => Self::Int32Array,
            KIND_UINT32 => Self::Uint32Array,
            KIND_FLOAT32 => Self::Float32Array,
            KIND_FLOAT64 => Self::Float64Array,
            KIND_BIGINT64 => Self::Bigint64Array,
            KIND_BIGUINT64 => Self::Biguint64Array,
            KIND_FLOAT16 => Self::Float16Array,
            other => unreachable!("typed-array element kind {other}"),
        }
    }

    /// The Perry element kind for a raw `napi_typedarray_type` from C, or
    /// `None` for a number the header does not define.
    fn element_kind(raw: i32) -> Option<u8> {
        use crate::typedarray::*;
        Some(match raw {
            0 => KIND_INT8,
            1 => KIND_UINT8,
            2 => KIND_UINT8_CLAMPED,
            3 => KIND_INT16,
            4 => KIND_UINT16,
            5 => KIND_INT32,
            6 => KIND_UINT32,
            7 => KIND_FLOAT32,
            8 => KIND_FLOAT64,
            9 => KIND_BIGINT64,
            10 => KIND_BIGUINT64,
            11 => KIND_FLOAT16,
            _ => return None,
        })
    }
}

fn checked_length(env: NapiEnv, length: usize, what: &'static str) -> Result<u32, NapiStatus> {
    // RULE 3 (`object/shape_rule3.rs`): the length becomes a `BufferHeader`'s
    // `capacity` at payload `+4`, the word the emitted property-read path
    // compares against a cached ShapeId. This entry point cannot throw (it
    // must answer with a `NapiStatus`), so it is the one that has to refuse —
    // `buffer_alloc_foreign` downstream can only clamp.
    u32::try_from(length)
        .ok()
        .filter(|&n| n <= crate::object::shape_rule3::MAX_PLUS_FOUR_WORD)
        .ok_or_else(|| set_status(env, NapiStatus::InvalidArg, what))
}

fn pointer_owner(env: NapiEnv, value: NapiValue) -> Result<usize, NapiStatus> {
    let bits = value_bits(env, value)?;
    super::metadata::owner_from_bits(bits).ok_or(NapiStatus::InvalidArg)
}

#[derive(Clone, Copy)]
enum NapiTypedArrayOwner {
    /// A typed array and its Perry element kind.
    Registered(usize, u8),
    Uint8Buffer(usize),
}

fn classify_typed_array_owner(owner: usize) -> Option<NapiTypedArrayOwner> {
    if let Some(kind) = crate::typedarray_hook_slots::lookup_typed_array_kind(owner) {
        Some(NapiTypedArrayOwner::Registered(owner, kind))
    } else if crate::object::typed_array_proto_thunks::is_typed_array_buffer(owner) {
        Some(NapiTypedArrayOwner::Uint8Buffer(owner))
    } else {
        None
    }
}

/// Whether `owner` is an ArrayBufferView (a typed array, a Buffer or a
/// DataView): the values Node's `napi_is_buffer` / `napi_get_buffer_info`
/// accept (`node::Buffer::HasInstance` is `IsArrayBufferView`).
fn is_array_buffer_view(owner: usize) -> bool {
    classify_typed_array_owner(owner).is_some() || crate::buffer::is_data_view(owner)
}

/// The data pointer of an ArrayBuffer or a view at `byte_offset`: its
/// backing store's data plus the offset, NULL once the store is detached
/// (V8's `Data()` of a detached ArrayBuffer).
unsafe fn store_data(addr: usize, byte_offset: usize) -> *mut c_void {
    let store_owner = crate::buffer::store::owner(addr);
    if (*crate::buffer::store::header(store_owner))._reserved & crate::codegen_abi::BYTES_DETACHED
        != 0
    {
        return ptr::null_mut();
    }
    crate::buffer::store::owner_data(store_owner)
        .add(byte_offset)
        .cast::<c_void>()
}

fn write_pointer_handle(env: NapiEnv, pointer: *const u8, result: *mut NapiValue) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    match add_handle(env, JSValue::pointer(pointer).bits()) {
        Ok(handle) => unsafe {
            *result = handle;
            ok(env)
        },
        Err(status) => status,
    }
}

unsafe fn initialize_buffer(
    env: NapiEnv,
    length: usize,
    data: *mut *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let length = match checked_length(env, length, "buffer length exceeds Perry's u32 limit") {
        Ok(length) => length,
        Err(status) => return status,
    };
    let (value, pin) = crate::buffer::bytes::new_bytes(
        crate::buffer::bytes::Brand::Buffer,
        length as usize,
        crate::buffer::bytes::Init::Uninit,
    );
    if !data.is_null() {
        *data = pin.as_mut_ptr().cast();
    }
    let buffer = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
    write_pointer_handle(env, buffer.cast_mut().cast(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_buffer(
    env: NapiEnv,
    length: usize,
    data: *mut *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    initialize_buffer(env, length, data, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_buffer_copy(
    env: NapiEnv,
    length: usize,
    data: *const c_void,
    result_data: *mut *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if length != 0 && data.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "source data must not be null");
    }
    let mut destination = ptr::null_mut();
    let status = initialize_buffer(env, length, &mut destination, result);
    if status != NapiStatus::Ok {
        return status;
    }
    if length != 0 {
        ptr::copy_nonoverlapping(data.cast::<u8>(), destination.cast::<u8>(), length);
    }
    if !result_data.is_null() {
        *result_data = destination;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_external_buffer(
    env: NapiEnv,
    length: usize,
    data: *mut c_void,
    finalize_cb: NapiFinalize,
    finalize_hint: *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || (length != 0 && data.is_null()) {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and external data must be valid",
        );
    }
    let length = match checked_length(env, length, "buffer length exceeds Perry's u32 limit") {
        Ok(length) => length,
        Err(status) => return status,
    };
    // Node: `node::Buffer::New` over the external bytes (a Buffer, not an
    // ArrayBuffer).
    let buffer = crate::buffer::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER,
        length,
        crate::buffer::store::Init::Foreign(data.cast()),
    );
    crate::buffer::set_foreign_finalizer(
        buffer,
        super::metadata::finalizer(finalize_cb, data, finalize_hint),
    );
    write_pointer_handle(env, buffer.cast(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_buffer(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = pointer_owner(env, value).is_ok_and(is_array_buffer_view);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_buffer_info(
    env: NapiEnv,
    value: NapiValue,
    data: *mut *mut c_void,
    length: *mut usize,
) -> NapiStatus {
    // Any ArrayBufferView, as Node: data and byte length of the view
    // (`Buffer::Data` / `ByteLength`); detached -> NULL and 0.
    let owner = match pointer_owner(env, value) {
        Ok(owner) if is_array_buffer_view(owner) => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be a Buffer"),
    };
    if !data.is_null() {
        *data = store_data(owner, crate::buffer::buffer_byte_offset(owner) as usize);
    }
    if !length.is_null() {
        *length = crate::buffer::store::length(owner) * crate::buffer::store::element_size(owner);
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_arraybuffer(
    env: NapiEnv,
    byte_length: usize,
    data: *mut *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let byte_length = match i32::try_from(byte_length) {
        Ok(length) => length,
        Err(_) => {
            return set_status(
                env,
                NapiStatus::InvalidArg,
                "ArrayBuffer length exceeds Perry's i32 limit",
            )
        }
    };
    let (value, pin) = crate::buffer::bytes::new_bytes(
        crate::buffer::bytes::Brand::ArrayBuffer,
        byte_length as usize,
        crate::buffer::bytes::Init::Zero,
    );
    if !data.is_null() {
        *data = pin.as_mut_ptr().cast();
    }
    let buffer = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
    write_pointer_handle(env, buffer, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_external_arraybuffer(
    env: NapiEnv,
    external_data: *mut c_void,
    byte_length: usize,
    finalize_cb: NapiFinalize,
    finalize_hint: *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || (byte_length != 0 && external_data.is_null()) {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and external data must be valid",
        );
    }
    let byte_length = match checked_length(
        env,
        byte_length,
        "ArrayBuffer length exceeds Perry's u32 limit",
    ) {
        Ok(length) => length,
        Err(status) => return status,
    };
    let buffer = crate::buffer::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER,
        byte_length,
        crate::buffer::store::Init::Foreign(external_data.cast()),
    );

    crate::buffer::set_foreign_finalizer(
        buffer,
        super::metadata::finalizer(finalize_cb, external_data, finalize_hint),
    );
    write_pointer_handle(env, buffer.cast(), result)
}

/// A Buffer view over `byte_length` bytes of an ArrayBuffer, sharing its
/// storage (Node-API 10). Status and exception behavior follow Node 26: a
/// non-ArrayBuffer is `napi_invalid_arg`, and an out-of-range window throws
/// `ERR_OUT_OF_RANGE` and returns that throw's status.
#[no_mangle]
pub unsafe extern "C" fn node_api_create_buffer_from_arraybuffer(
    env: NapiEnv,
    arraybuffer: NapiValue,
    byte_offset: usize,
    byte_length: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    if pending_exception(env).is_some() {
        return set_status(env, NapiStatus::PendingException, "an exception is pending");
    }
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let owner = match pointer_owner(env, arraybuffer) {
        Ok(owner) if crate::buffer::is_array_buffer(owner) => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be an ArrayBuffer"),
    };
    let available = crate::buffer::store::length(owner);
    let window = byte_offset
        .checked_add(byte_length)
        .filter(|end| *end <= available)
        .and_then(|_| {
            Some((
                i32::try_from(byte_offset).ok()?,
                i32::try_from(byte_length).ok()?,
            ))
        });
    let Some((offset, length)) = window else {
        return super::values::napi_throw_range_error(
            env,
            c"ERR_OUT_OF_RANGE".as_ptr(),
            c"The byte offset + length is out of range".as_ptr(),
        );
    };
    // Nothing has allocated since the owner was read from its live handle.
    let view = crate::buffer::js_buffer_from_arraybuffer_slice(
        JSValue::pointer(owner as *const u8).bits() as i64,
        offset,
        length,
    );
    write_pointer_handle(env, view.cast(), result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_arraybuffer(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = pointer_owner(env, value).is_ok_and(crate::buffer::is_array_buffer);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_arraybuffer_info(
    env: NapiEnv,
    arraybuffer: NapiValue,
    data: *mut *mut c_void,
    byte_length: *mut usize,
) -> NapiStatus {
    let owner = match pointer_owner(env, arraybuffer) {
        Ok(owner) if crate::buffer::is_array_buffer(owner) => owner,
        _ => {
            return set_status(
                env,
                NapiStatus::ArraybufferExpected,
                "value must be an ArrayBuffer",
            )
        }
    };
    if !data.is_null() {
        *data = store_data(owner, 0);
    }
    if !byte_length.is_null() {
        *byte_length = crate::buffer::store::length(owner);
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_typedarray(
    env: NapiEnv,
    kind: i32,
    length: usize,
    arraybuffer: NapiValue,
    byte_offset: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Some(element_kind) = NapiTypedarrayType::element_kind(kind) else {
        return set_status(env, NapiStatus::InvalidArg, "unknown typed array type");
    };
    let arraybuffer_bits = match value_bits(env, arraybuffer) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "ArrayBuffer is not a live handle"),
    };
    let Some(owner) = super::metadata::owner_from_bits(arraybuffer_bits) else {
        return set_status(
            env,
            NapiStatus::ArraybufferExpected,
            "value must be an ArrayBuffer",
        );
    };
    if !crate::buffer::is_array_buffer(owner) {
        return set_status(
            env,
            NapiStatus::ArraybufferExpected,
            "value must be an ArrayBuffer",
        );
    }
    let length = match checked_length(env, length, "typed array length exceeds u32") {
        Ok(length) => length,
        Err(status) => return status,
    };
    let byte_offset = match u32::try_from(byte_offset) {
        Ok(offset) => offset,
        Err(_) => return set_status(env, NapiStatus::InvalidArg, "byte offset exceeds u32"),
    };
    match catch_value_call(env, || {
        let typed_array = crate::typedarray_hook_slots::js_typed_array_view(
            element_kind as i32,
            f64::from_bits(arraybuffer_bits),
            byte_offset as f64,
            length as f64,
        );
        f64::from_bits(JSValue::pointer(typed_array.cast()).bits())
    }) {
        Ok(value) => write_pointer_handle(
            env,
            JSValue::from_bits(value.to_bits()).as_pointer::<u8>(),
            result,
        ),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_typedarray(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = pointer_owner(env, value)
        .ok()
        .and_then(classify_typed_array_owner)
        .is_some();
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_typedarray_info(
    env: NapiEnv,
    typedarray: NapiValue,
    kind: *mut NapiTypedarrayType,
    length: *mut usize,
    data: *mut *mut c_void,
    arraybuffer: *mut NapiValue,
    byte_offset: *mut usize,
) -> NapiStatus {
    let owner = match pointer_owner(env, typedarray)
        .ok()
        .and_then(classify_typed_array_owner)
    {
        Some(owner) => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be a TypedArray"),
    };
    // Every answer is a field of the view's own header; the ArrayBuffer
    // object is resolved only for a caller that asks for it (Node calls
    // Buffer() only then: materializing it is the one costly part).
    let (addr, reported_kind) = match owner {
        NapiTypedArrayOwner::Registered(addr, kind) => {
            (addr, NapiTypedarrayType::from_element_kind(kind))
        }
        NapiTypedArrayOwner::Uint8Buffer(addr) => (addr, NapiTypedarrayType::Uint8Array),
    };
    if !arraybuffer.is_null() {
        let backing = match owner {
            NapiTypedArrayOwner::Registered(..) => {
                crate::typedarray_view::js_typed_array_backing_buffer(addr as *mut TypedArrayHeader)
                    as usize
            }
            NapiTypedArrayOwner::Uint8Buffer(_) => crate::buffer::buffer_backing_array_buffer(addr),
        };
        *arraybuffer = add_handle(env, JSValue::pointer(backing as *const u8).bits())
            .unwrap_or(ptr::null_mut());
    }
    if !kind.is_null() {
        *kind = reported_kind;
    }
    // Detached or out of bounds: length 0 and byte offset 0 (V8's Length()
    // and ByteOffset()); data is the backing store's data plus that offset,
    // NULL once the store is detached.
    let reported_offset = crate::buffer::buffer_byte_offset(addr) as usize;
    if !length.is_null() {
        *length = crate::buffer::store::length(addr);
    }
    if !byte_offset.is_null() {
        *byte_offset = reported_offset;
    }
    if !data.is_null() {
        *data = store_data(addr, reported_offset);
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_dataview(
    env: NapiEnv,
    length: usize,
    arraybuffer: NapiValue,
    byte_offset: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let bits = match value_bits(env, arraybuffer) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "ArrayBuffer is not a live handle"),
    };
    let length = match u32::try_from(length) {
        Ok(length) => length,
        Err(_) => return set_status(env, NapiStatus::InvalidArg, "DataView length exceeds u32"),
    };
    let offset = match u32::try_from(byte_offset) {
        Ok(offset) => offset,
        Err(_) => return set_status(env, NapiStatus::InvalidArg, "DataView offset exceeds u32"),
    };
    match catch_value_call(env, || {
        crate::buffer::js_data_view_new(f64::from_bits(bits), offset as f64, length as f64)
    }) {
        Ok(value) => write_pointer_handle(
            env,
            JSValue::from_bits(value.to_bits()).as_pointer::<u8>(),
            result,
        ),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_dataview(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = pointer_owner(env, value).is_ok_and(crate::buffer::is_data_view);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_dataview_info(
    env: NapiEnv,
    dataview: NapiValue,
    byte_length: *mut usize,
    data: *mut *mut c_void,
    arraybuffer: *mut NapiValue,
    byte_offset: *mut usize,
) -> NapiStatus {
    let owner = match pointer_owner(env, dataview) {
        Ok(owner) if crate::buffer::is_data_view(owner) => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be a DataView"),
    };
    let view = owner as *const BufferHeader;
    let info = crate::buffer::view::lookup(owner);
    if !byte_length.is_null() {
        *byte_length = crate::buffer::store::length(view as usize);
    }
    if !data.is_null() {
        *data = crate::buffer::resolve_span_data_ptr(view) as *mut c_void;
    }
    if !byte_offset.is_null() {
        *byte_offset = info.map_or(0, |info| info.offset as usize);
    }
    if !arraybuffer.is_null() {
        let backing = info.map_or(owner, |info| info.backing);
        *arraybuffer =
            add_handle(env, JSValue::pointer(backing as *mut u8).bits()).unwrap_or(ptr::null_mut());
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_detach_arraybuffer(
    env: NapiEnv,
    arraybuffer: NapiValue,
) -> NapiStatus {
    let owner = match pointer_owner(env, arraybuffer) {
        Ok(owner) if crate::buffer::is_array_buffer(owner) => owner,
        _ => {
            return set_status(
                env,
                NapiStatus::DetachableArraybufferExpected,
                "value must be a detachable ArrayBuffer",
            )
        }
    };
    crate::buffer::detach_array_buffer(owner);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_detached_arraybuffer(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let owner = match pointer_owner(env, value) {
        Ok(owner) if crate::buffer::is_array_buffer(owner) => owner,
        _ => {
            return set_status(
                env,
                NapiStatus::ArraybufferExpected,
                "value must be an ArrayBuffer",
            )
        }
    };
    *result = crate::buffer::is_detached_buffer(owner);
    ok(env)
}
