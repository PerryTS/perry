use super::*;
use crate::buffer::BufferHeader;
use crate::typedarray::TypedArrayHeader;
use crate::value::JSValue;
use std::ffi::c_void;
use std::ptr;

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiTypedarrayType {
    Int8Array = 0,
    Uint8Array = 1,
    Int16Array = 2,
    Uint16Array = 3,
    Int32Array = 4,
    Uint32Array = 5,
    Float32Array = 6,
    Float64Array = 7,
    Uint8ClampedArray = 8,
    Bigint64Array = 9,
    Biguint64Array = 10,
}

fn checked_length(env: NapiEnv, length: usize, what: &'static str) -> Result<u32, NapiStatus> {
    u32::try_from(length).map_err(|_| set_status(env, NapiStatus::InvalidArg, what))
}

fn pointer_owner(env: NapiEnv, value: NapiValue) -> Result<usize, NapiStatus> {
    let bits = value_bits(env, value)?;
    super::metadata::owner_from_bits(bits).ok_or(NapiStatus::InvalidArg)
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
    let buffer = crate::buffer::buffer_alloc(length);
    (*buffer).length = length;
    if !data.is_null() {
        *data = crate::buffer::buffer_data_mut(buffer).cast();
    }
    write_pointer_handle(env, buffer.cast(), result)
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
    let buffer = crate::buffer::buffer_alloc_foreign(data.cast(), length);
    super::metadata::attach_owner_finalizer(buffer as usize, data, finalize_cb, finalize_hint);
    write_pointer_handle(env, buffer.cast(), result)
}

fn is_node_buffer(owner: usize) -> bool {
    crate::buffer::is_registered_buffer(owner)
        && !crate::buffer::is_any_array_buffer(owner)
        && !crate::buffer::is_data_view(owner)
        && !crate::buffer::is_uint8array_buffer(owner)
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
    *result = pointer_owner(env, value).is_ok_and(is_node_buffer);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_buffer_info(
    env: NapiEnv,
    value: NapiValue,
    data: *mut *mut c_void,
    length: *mut usize,
) -> NapiStatus {
    let owner = match pointer_owner(env, value) {
        Ok(owner) if is_node_buffer(owner) => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be a Buffer"),
    };
    let buffer = owner as *const BufferHeader;
    if !data.is_null() {
        *data = crate::buffer::resolve_span_data_ptr(buffer) as *mut c_void;
    }
    if !length.is_null() {
        *length = (*buffer).length as usize;
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
    let buffer = crate::buffer::js_array_buffer_new(byte_length);
    if !data.is_null() {
        *data = crate::buffer::buffer_data_mut(buffer).cast();
    }
    write_pointer_handle(env, buffer.cast(), result)
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
    let buffer = crate::buffer::buffer_alloc_foreign(external_data.cast(), byte_length);
    crate::buffer::mark_as_array_buffer(buffer as usize);
    super::metadata::attach_owner_finalizer(
        buffer as usize,
        external_data,
        finalize_cb,
        finalize_hint,
    );
    write_pointer_handle(env, buffer.cast(), result)
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
    let buffer = owner as *const BufferHeader;
    if !data.is_null() {
        *data = crate::buffer::buffer_data(buffer) as *mut c_void;
    }
    if !byte_length.is_null() {
        *byte_length = (*buffer).length as usize;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_typedarray(
    env: NapiEnv,
    kind: NapiTypedarrayType,
    length: usize,
    arraybuffer: NapiValue,
    byte_offset: usize,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
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
        let typed_array = crate::typedarray_view::js_typed_array_view(
            kind as i32,
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
        .is_ok_and(|owner| crate::typedarray::lookup_typed_array_kind(owner).is_some());
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
    let owner = match pointer_owner(env, typedarray) {
        Ok(owner) if crate::typedarray::lookup_typed_array_kind(owner).is_some() => owner,
        _ => return set_status(env, NapiStatus::InvalidArg, "value must be a TypedArray"),
    };
    let typed_array = owner as *mut TypedArrayHeader;
    let backing = crate::typedarray_view::js_typed_array_backing_buffer(typed_array);
    let refreshed_owner = pointer_owner(env, typedarray).unwrap_or(owner);
    let refreshed = refreshed_owner as *mut TypedArrayHeader;
    if !kind.is_null() {
        *kind = std::mem::transmute::<i32, NapiTypedarrayType>((*refreshed).kind as i32);
    }
    if !length.is_null() {
        *length = (*refreshed).length as usize;
    }
    if !data.is_null() {
        *data = crate::typedarray::data_ptr_mut(refreshed).cast();
    }
    if !byte_offset.is_null() {
        *byte_offset = crate::typedarray_view::js_typed_array_byte_offset(refreshed) as usize;
    }
    if !arraybuffer.is_null() {
        *arraybuffer =
            add_handle(env, JSValue::pointer(backing.cast()).bits()).unwrap_or(ptr::null_mut());
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
        *byte_length = (*view).length as usize;
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
