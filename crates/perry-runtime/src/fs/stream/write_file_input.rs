//! `writeFile` / `writeFileSync` input-consumption helpers, split out of
//! `stream.rs` to keep that file under the 2k-line limit. These resolve the
//! many shapes Node accepts for the `data` argument (string, Buffer,
//! TypedArray/DataView, async iterables, streams) and the abort-`signal`
//! option, then drive the bytes to an fd or path. `use super::*` pulls in the
//! private `stream.rs` helpers (`bytes_from_buffer_value`,
//! `encoding_tag_from_options`, `object_value`, the `STREAM_REGISTRY`, …).

use super::*;

fn write_file_data_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn write_file_signal_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn write_file_signal_from_options(options_value: f64) -> Result<Option<*mut ObjectHeader>, f64> {
    let value = JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() || value.is_any_string() {
        return Ok(None);
    }
    unsafe {
        let Some(signal_value) = options_field_value(options_value, b"signal") else {
            return Ok(None);
        };
        let signal_f64 = f64::from_bits(signal_value.bits());
        let signal_js = JSValue::from_bits(signal_value.bits());
        if signal_js.is_undefined() {
            return Ok(None);
        }
        match crate::url::abort::abort_signal_ptr_from_value(signal_f64) {
            Some(signal) => Ok(Some(signal)),
            None => Err(write_file_signal_type_error(signal_f64)),
        }
    }
}

fn check_write_file_aborted(signal: Option<*mut ObjectHeader>) -> Result<(), f64> {
    let Some(signal) = signal else {
        return Ok(());
    };
    if crate::url::js_abort_signal_is_aborted(signal) != 0 {
        Err(crate::url::js_abort_error_value())
    } else {
        Ok(())
    }
}

fn write_file_chunk_bytes(value: f64, encoding_tag: i32) -> Result<Vec<u8>, f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_any_string() {
        return Ok(bytes_from_string_value(value, encoding_tag));
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return Ok(bytes_from_buffer_value(value));
    }
    let bits = value.to_bits();
    let addr = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        if let Some(bytes) = unsafe { crate::typedarray::typed_array_bytes(ta) } {
            return Ok(bytes.to_vec());
        }
        return Ok(Vec::new());
    }
    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        let buf = crate::buffer::js_buffer_from_value(value.to_bits() as i64, encoding_tag);
        if buf.is_null() {
            return Ok(Vec::new());
        }
        return Ok(unsafe {
            std::slice::from_raw_parts(crate::buffer::buffer_data(buf), (*buf).length as usize)
                .to_vec()
        });
    }
    Err(write_file_data_type_error(value))
}

fn write_file_raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let js = JSValue::from_bits(bits);
    if js.is_pointer() || js.is_string() || js.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn write_file_gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn write_file_object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let raw = write_file_raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if write_file_gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *const ObjectHeader)
}

fn is_callable_write_value(value: f64) -> bool {
    let raw = write_file_raw_ptr_from_value(value);
    raw >= 0x10000
        && !crate::buffer::is_registered_buffer(raw)
        && crate::closure::is_closure_ptr(raw)
}

fn well_known_iterator_method(value: f64, name: &str) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol(name);
    if sym.is_null() {
        return None;
    }
    let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_value) };
    if !is_callable_write_value(method) {
        return None;
    }
    Some(method)
}

fn call_well_known_iterator(value: f64, name: &str) -> Option<f64> {
    let method = well_known_iterator_method(value, name)?;
    let prev_this = crate::object::js_implicit_this_set(value);
    let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

fn value_has_named_next(value: f64) -> bool {
    let Some(obj) = write_file_object_ptr_from_value(value) else {
        return false;
    };
    let key = js_string_from_bytes(b"next".as_ptr(), 4);
    let field = crate::object::js_object_get_field_by_name(obj, key);
    let field_value = f64::from_bits(field.bits());
    is_callable_write_value(field_value)
}

fn write_file_iterator_for_value(value: f64) -> Option<f64> {
    if write_file_raw_ptr_from_value(value) == 0 {
        return None;
    }
    if let Some(iter) = call_well_known_iterator(value, "asyncIterator") {
        return Some(iter);
    }
    if value_has_named_next(value) {
        return Some(value);
    }
    call_well_known_iterator(value, "iterator")
}

fn write_file_data_has_source(value: f64) -> bool {
    if is_direct_write_data(value) || fs_read_stream_id(value).is_some() {
        return true;
    }
    if write_file_raw_ptr_from_value(value) == 0 {
        return false;
    }
    well_known_iterator_method(value, "asyncIterator").is_some()
        || value_has_named_next(value)
        || well_known_iterator_method(value, "iterator").is_some()
}

fn validate_write_file_data_source(value: f64) -> Result<(), f64> {
    if write_file_data_has_source(value) {
        Ok(())
    } else {
        Err(write_file_data_type_error(value))
    }
}

fn settle_write_file_value(value: f64) -> Result<f64, f64> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    for _ in 0..10_000 {
        let current = value_handle.get_nanbox_f64();
        if crate::promise::js_value_is_promise(current) == 0 {
            return Ok(current);
        }
        let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
        if promise.is_null() {
            return Ok(current);
        }
        unsafe {
            match (*promise).state {
                crate::promise::PromiseState::Fulfilled => return Ok((*promise).value),
                crate::promise::PromiseState::Rejected => return Err((*promise).reason),
                crate::promise::PromiseState::Pending => {}
            }
        }
        crate::event_pump::perry_poll();
        let _ = crate::timer::js_timer_tick();
        let _ = crate::timer::js_callback_timer_tick();
        let _ = crate::timer::js_interval_timer_tick();
        if crate::event_pump::perry_has_work() == 0 {
            break;
        }
        crate::event_pump::js_wait_for_event();
    }
    let current = value_handle.get_nanbox_f64();
    let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
    if promise.is_null() {
        return Ok(current);
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Ok((*promise).value),
            crate::promise::PromiseState::Rejected => Err((*promise).reason),
            crate::promise::PromiseState::Pending => Ok(current),
        }
    }
}

fn iterator_result_value_done(result: f64) -> Option<(f64, bool)> {
    let obj = write_file_object_ptr_from_value(result)?;
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let done = crate::object::js_object_get_field_by_name(obj, done_key);
    let value = crate::object::js_object_get_field_by_name(obj, value_key);
    let done_f64 = f64::from_bits(done.bits());
    let value_f64 = f64::from_bits(value.bits());
    Some((value_f64, crate::value::js_is_truthy(done_f64) != 0))
}

fn consume_iterator_for_write_file<F>(
    iterator: f64,
    encoding_tag: i32,
    signal: Option<*mut ObjectHeader>,
    mut write_chunk: F,
) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    for _ in 0..100_000 {
        check_write_file_aborted(signal)?;
        let next_result = unsafe {
            crate::object::js_native_call_method(
                iterator,
                b"next".as_ptr() as *const i8,
                4,
                std::ptr::null(),
                0,
            )
        };
        let next_result = settle_write_file_value(next_result)?;
        let Some((chunk, done)) = iterator_result_value_done(next_result) else {
            return Ok(());
        };
        if done {
            return Ok(());
        }
        check_write_file_aborted(signal)?;
        let bytes = write_file_chunk_bytes(chunk, encoding_tag)?;
        write_chunk(&bytes)?;
    }
    Ok(())
}

fn fs_read_stream_id(value: f64) -> Option<usize> {
    STREAM_REGISTRY.with(|registry| {
        registry.borrow().iter().find_map(|(id, state)| {
            (state.kind == StreamKind::Read
                && state.object_value.to_bits() == value.to_bits()
                && !state.destroyed)
                .then_some(*id)
        })
    })
}

fn consume_fs_read_stream_for_write_file<F>(
    id: usize,
    signal: Option<*mut ObjectHeader>,
    mut write_chunk: F,
) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    loop {
        check_write_file_aborted(signal)?;
        match read_next_chunk(id) {
            Ok(Some((bytes, _encoding))) => {
                check_write_file_aborted(signal)?;
                write_chunk(&bytes)?;
            }
            Ok(None) => {
                finish_read_stream(id);
                return Ok(());
            }
            Err(message) => return Err(make_error_value(&message)),
        }
    }
}

fn consume_write_file_input<F>(data: f64, options: f64, mut write_chunk: F) -> Result<(), f64>
where
    F: FnMut(&[u8]) -> Result<(), f64>,
{
    let signal = write_file_signal_from_options(options)?;
    check_write_file_aborted(signal)?;
    let encoding_tag = encoding_tag_from_options(options);
    if is_direct_write_data(data) {
        let bytes = write_file_chunk_bytes(data, encoding_tag)?;
        return write_chunk(&bytes);
    }
    if let Some(id) = fs_read_stream_id(data) {
        return consume_fs_read_stream_for_write_file(id, signal, write_chunk);
    }
    if let Some(iterator) = write_file_iterator_for_value(data) {
        return consume_iterator_for_write_file(iterator, encoding_tag, signal, write_chunk);
    }
    Err(write_file_data_type_error(data))
}

pub(crate) fn write_fd_chunk_result(fd: i32, bytes: &[u8], force_append: bool) -> Result<(), f64> {
    let result = FD_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(file) = registry.get_mut(&fd) else {
            return Err(std::io::Error::from_raw_os_error(libc::EBADF));
        };
        if force_append
            || FD_APPEND_MODE.with(|flags| flags.borrow().get(&fd).copied().unwrap_or(false))
        {
            file.seek(SeekFrom::End(0))?;
        }
        file.write_all(bytes)
    });
    result.map_err(|err| unsafe { build_fs_error_value_no_path(&err, "write") })
}

pub(crate) unsafe fn write_file_to_fd_result(
    fd: i32,
    data: f64,
    options: f64,
    force_append: bool,
) -> Result<(), f64> {
    validate::validate_string_or_object_options("options", options);
    validate_write_file_data_source(data)?;
    consume_write_file_input(data, options, |bytes| {
        write_fd_chunk_result(fd, bytes, force_append)
    })
}

pub(crate) unsafe fn write_file_path_or_fd_result(
    path_value: f64,
    data: f64,
    options: f64,
) -> Result<(), f64> {
    validate::validate_path_or_fd("path", path_value, "write");
    validate::validate_string_or_object_options("options", options);
    validate_write_file_data_source(data)?;
    let signal = write_file_signal_from_options(options)?;
    check_write_file_aborted(signal)?;

    if let Some(fd) = numeric_fd_value(path_value) {
        return consume_write_file_input(data, options, |bytes| {
            write_fd_chunk_result(fd, bytes, false)
        });
    }

    let path = match decode_path_value(path_value) {
        Some(path) => path,
        None => validate::throw_invalid_path_arg("path", path_value),
    };
    let flag = file_options_flag(options, "w");
    let mut file = match open_file_for_write_flag(&path, &flag) {
        Ok(file) => file,
        Err(err) => return Err(build_fs_error_value(&err, "open", &path)),
    };
    consume_write_file_input(data, options, |bytes| {
        file.write_all(bytes)
            .map_err(|err| build_fs_error_value(&err, "write", &path))
    })
}
