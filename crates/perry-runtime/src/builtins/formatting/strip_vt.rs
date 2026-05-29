#[no_mangle]
pub extern "C" fn js_util_strip_vt_control_characters(value: f64) -> f64 {
    unsafe {
        let s_ptr = crate::value::js_jsvalue_to_string(value);
        let input = if s_ptr.is_null() {
            String::new()
        } else {
            let len = (*s_ptr).byte_len as usize;
            let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        };
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                let start = i;
                i += 1;
                if i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                    while i < bytes.len() {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                    continue;
                } else if i < bytes.len() && bytes[i] == b']' {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                out.push_str(&input[start..i]);
            } else {
                // Preserve multi-byte UTF-8 sequences by advancing through
                // the whole code point instead of casting one byte to char.
                let lead = bytes[i];
                let width = if lead < 0x80 {
                    1
                } else if lead < 0xc0 {
                    1
                } else if lead < 0xe0 {
                    2
                } else if lead < 0xf0 {
                    3
                } else {
                    4
                };
                let end = (i + width).min(bytes.len());
                out.push_str(std::str::from_utf8(&bytes[i..end]).unwrap_or(""));
                i = end;
            }
        }
        let ptr = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}
