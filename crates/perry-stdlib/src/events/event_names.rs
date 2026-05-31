use perry_runtime::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_object_get_field_by_name_f64,
    js_object_set_field_by_name, js_string_from_bytes, JSValue, ObjectHeader, StringHeader,
};

#[derive(Clone)]
pub(super) struct EventName {
    pub(super) key: String,
    pub(super) value: f64,
    pub(super) display: String,
}

/// Helper to extract string from StringHeader pointer
pub(super) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    let sym_ptr = ptr as *const perry_runtime::symbol::SymbolHeader;
    if (*sym_ptr).magic == perry_runtime::symbol::SYMBOL_MAGIC {
        let sym_value = js_nanbox_pointer(ptr as i64);
        let rendered = perry_runtime::symbol::js_symbol_to_string(sym_value);
        return string_from_header(rendered as *const StringHeader);
    }

    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

pub(super) fn string_event_key(name: &str) -> String {
    format!("str:{name}")
}

pub(super) fn error_monitor_event_key() -> String {
    let name = js_string_from_bytes(b"events.errorMonitor".as_ptr(), 19);
    let sym = unsafe { perry_runtime::symbol::js_symbol_for(js_nanbox_string(name as i64)) };
    format!("sym:{}", js_nanbox_get_pointer(sym))
}

pub(super) fn string_from_value(value: f64) -> Option<String> {
    let ptr = perry_runtime::value::js_jsvalue_to_string(value);
    unsafe { string_from_header(ptr as *const StringHeader) }
}

pub(super) fn normalize_event_name(value: f64) -> Option<EventName> {
    if unsafe { perry_runtime::symbol::js_is_symbol(value) } != 0 {
        let display = string_from_value(value).unwrap_or_else(|| "Symbol()".to_string());
        return Some(EventName {
            key: format!("sym:{}", js_nanbox_get_pointer(value)),
            value,
            display,
        });
    }
    let str_ptr = perry_runtime::value::js_jsvalue_to_string(value);
    let display = unsafe { string_from_header(str_ptr as *const StringHeader) }?;
    Some(EventName {
        key: string_event_key(&display),
        value: js_nanbox_string(str_ptr as i64),
        display,
    })
}

pub(super) fn event_name_ptr_from_value(value: f64) -> Option<*const StringHeader> {
    let ptr = if JSValue::from_bits(value.to_bits()).is_any_string() {
        perry_runtime::value::js_get_string_pointer_unified(value) as *const StringHeader
    } else {
        perry_runtime::value::js_jsvalue_to_string(value) as *const StringHeader
    };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

pub(super) fn js_string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_nanbox_string(ptr as i64)
}

pub(super) fn set_object_prop(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

pub(super) unsafe fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = super::object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}
