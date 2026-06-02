fn throw_invalid_str_arg(value: f64) -> ! {
    let message = format!(
        "The \"str\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn is_utf8_c1(bytes: &[u8], i: usize, code: u8) -> bool {
    i + 1 < bytes.len() && bytes[i] == 0xc2 && bytes[i + 1] == code
}

fn skip_csi(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if (0x40..=0x7e).contains(&b) {
            break;
        }
    }
    i
}

fn skip_string_control(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if bytes[i] == 0x07 {
            return i + 1;
        }
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
            return i + 2;
        }
        if is_utf8_c1(bytes, i, 0x9c) {
            return i + 2;
        }
        i += 1;
    }
    i
}

fn strip_vt_control_sequences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 >= bytes.len() {
                out.push('\x1b');
                i += 1;
                continue;
            }

            match bytes[i + 1] {
                b'[' => {
                    i = skip_csi(bytes, i + 2);
                    continue;
                }
                b']' | b'P' | b'^' | b'_' => {
                    i = skip_string_control(bytes, i + 2);
                    continue;
                }
                b'(' | b')' | b'*' | b'+' | b'-' | b'.' | b'/' | b'#' | b'%' => {
                    i = (i + 3).min(bytes.len());
                    continue;
                }
                0x30..=0x7e => {
                    i += 2;
                    continue;
                }
                _ => {
                    out.push('\x1b');
                    i += 1;
                    continue;
                }
            }
        }

        if is_utf8_c1(bytes, i, 0x9b) {
            i = skip_csi(bytes, i + 2);
            continue;
        }
        if is_utf8_c1(bytes, i, 0x9d)
            || is_utf8_c1(bytes, i, 0x90)
            || is_utf8_c1(bytes, i, 0x9e)
            || is_utf8_c1(bytes, i, 0x9f)
        {
            i = skip_string_control(bytes, i + 2);
            continue;
        }

        let ch = input[i..].chars().next().unwrap_or_default();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[no_mangle]
pub extern "C" fn js_util_strip_vt_control_characters(value: f64) -> f64 {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_any_string() {
        throw_invalid_str_arg(value);
    }

    let input = crate::url::get_string_content(value);
    let out = strip_vt_control_sequences(&input);
    let ptr = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}
