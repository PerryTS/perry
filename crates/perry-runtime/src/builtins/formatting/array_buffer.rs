pub(super) unsafe fn format_array_buffer_value(
    buf_ptr: *const crate::buffer::BufferHeader,
    label: &str,
) -> String {
    if buf_ptr.is_null() {
        return format!("{label} {{ [Uint8Contents]: <>, [byteLength]: 0 }}");
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    let display_len = len.min(50);
    let mut contents = String::new();
    for (i, b) in bytes[..display_len].iter().enumerate() {
        if i > 0 {
            contents.push(' ');
        }
        contents.push_str(&format!("{:02x}", b));
    }
    if len > display_len {
        contents.push_str(&format!(" ... {} more bytes", len - display_len));
    }
    format!("{label} {{ [Uint8Contents]: <{contents}>, [byteLength]: {len} }}")
}

pub(super) unsafe fn format_data_view_value(buf_ptr: *const crate::buffer::BufferHeader) -> String {
    if buf_ptr.is_null() {
        return "DataView {\n  [byteLength]: 0,\n  [byteOffset]: 0,\n  [buffer]: ArrayBuffer { [Uint8Contents]: <>, [byteLength]: 0 }\n}".to_string();
    }
    let len = (*buf_ptr).length as usize;
    let buffer = format_array_buffer_value(buf_ptr, "ArrayBuffer");
    format!("DataView {{\n  [byteLength]: {len},\n  [byteOffset]: 0,\n  [buffer]: {buffer}\n}}")
}
