//! Runtime helpers for classic `node:readline` module-level functions.

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::{
    get_valid_func_ptr, js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    ClosureHeader,
};
use crate::object::{js_object_alloc_with_shape, js_object_set_field};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{js_jsvalue_to_string, JSValue, TAG_FALSE, TAG_UNDEFINED};

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn boxed_str(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn raw_ptr_from_value(value: f64) -> Option<i64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_pointer() {
        let raw = js.as_pointer::<u8>() as i64;
        if raw >= 0x10000 {
            return Some(raw);
        }
    }
    None
}

fn is_callable(value: f64) -> bool {
    raw_ptr_from_value(value)
        .map(|raw| !get_valid_func_ptr(raw as *const ClosureHeader).is_null())
        .unwrap_or(false)
}

fn arg(args: *const crate::array::ArrayHeader, index: u32) -> f64 {
    if args.is_null() || index >= js_array_length(args) {
        undefined()
    } else {
        js_array_get_f64(args, index)
    }
}

fn number_arg(value: f64, default: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() && value.is_finite() {
        value
    } else {
        default
    }
}

fn optional_callback(args: *const crate::array::ArrayHeader, start: u32) -> f64 {
    if args.is_null() {
        return undefined();
    }
    let len = js_array_length(args);
    for i in start..len {
        let value = js_array_get_f64(args, i);
        if is_callable(value) {
            return value;
        }
    }
    undefined()
}

fn write_escape(stream: f64, bytes: &[u8], callback: f64) -> f64 {
    let Some(raw) = raw_ptr_from_value(stream) else {
        return f64::from_bits(TAG_FALSE);
    };
    let chunk = boxed_str(bytes);
    crate::node_stream::js_node_stream_method_write(raw, chunk, undefined(), callback)
}

fn ansi_clear_line(dir: f64) -> &'static [u8] {
    match number_arg(dir, 0.0) as i32 {
        -1 => b"\x1b[1K",
        1 => b"\x1b[0K",
        _ => b"\x1b[2K",
    }
}

#[no_mangle]
pub extern "C" fn js_readline_clear_line_args(args: *const crate::array::ArrayHeader) -> f64 {
    write_escape(
        arg(args, 0),
        ansi_clear_line(arg(args, 1)),
        optional_callback(args, 2),
    )
}

#[no_mangle]
pub extern "C" fn js_readline_clear_screen_down_args(
    args: *const crate::array::ArrayHeader,
) -> f64 {
    write_escape(arg(args, 0), b"\x1b[0J", optional_callback(args, 1))
}

#[no_mangle]
pub extern "C" fn js_readline_cursor_to_args(args: *const crate::array::ArrayHeader) -> f64 {
    let x = number_arg(arg(args, 1), 0.0).max(0.0) as i32 + 1;
    let y_arg = arg(args, 2);
    let y_js = JSValue::from_bits(y_arg.to_bits());
    let seq = if y_js.is_undefined() || is_callable(y_arg) {
        format!("\x1b[{x}G")
    } else {
        let y = number_arg(y_arg, 0.0).max(0.0) as i32 + 1;
        format!("\x1b[{y};{x}H")
    };
    write_escape(
        arg(args, 0),
        seq.as_bytes(),
        optional_callback(args, if y_js.is_undefined() { 2 } else { 3 }),
    )
}

#[no_mangle]
pub extern "C" fn js_readline_move_cursor_args(args: *const crate::array::ArrayHeader) -> f64 {
    let dx = number_arg(arg(args, 1), 0.0) as i32;
    let dy = number_arg(arg(args, 2), 0.0) as i32;
    let mut seq = String::new();
    if dx < 0 {
        seq.push_str(&format!("\x1b[{}D", -dx));
    } else if dx > 0 {
        seq.push_str(&format!("\x1b[{dx}C"));
    }
    if dy < 0 {
        seq.push_str(&format!("\x1b[{}A", -dy));
    } else if dy > 0 {
        seq.push_str(&format!("\x1b[{dy}B"));
    }
    write_escape(arg(args, 0), seq.as_bytes(), optional_callback(args, 3))
}

fn string_bytes(value: f64) -> Vec<u8> {
    let ptr = js_jsvalue_to_string(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return Vec::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn build_keypress_object(name: &str, ctrl: bool, shift: bool, meta: bool, seq: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let packed = b"name\0ctrl\0shift\0meta\0sequence\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF48, 5, packed.as_ptr(), packed.len() as u32);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let name_str = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let obj = obj_handle.get_raw_mut_ptr();
    js_object_set_field(obj, 0, JSValue::string_ptr(name_str));
    js_object_set_field(obj, 1, JSValue::bool(ctrl));
    js_object_set_field(obj, 2, JSValue::bool(shift));
    js_object_set_field(obj, 3, JSValue::bool(meta));
    let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
    let obj = obj_handle.get_raw_mut_ptr();
    js_object_set_field(obj, 4, JSValue::string_ptr(seq_str));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn parse_keypress(chunk: &[u8]) -> Option<(Option<String>, String, bool, bool, bool, String)> {
    if chunk.is_empty() {
        return None;
    }
    let seq = String::from_utf8_lossy(chunk).into_owned();
    if chunk.len() == 3 && chunk[0] == 0x1b && chunk[1] == b'[' {
        let name = match chunk[2] {
            b'A' => "up",
            b'B' => "down",
            b'C' => "right",
            b'D' => "left",
            b'H' => "home",
            b'F' => "end",
            _ => "undefined",
        };
        return Some((None, name.to_string(), false, false, false, seq));
    }
    if chunk.len() == 1 {
        let b = chunk[0];
        let (name, ctrl) = match b {
            b'\r' | b'\n' => ("return".to_string(), false),
            b'\t' => ("tab".to_string(), false),
            0x7f | 0x08 => ("backspace".to_string(), false),
            0x1b => ("escape".to_string(), false),
            b' ' => ("space".to_string(), false),
            0x01..=0x1a => (((b + b'a' - 1) as char).to_string(), true),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => ((b as char).to_string(), false),
            _ => (seq.clone(), false),
        };
        let shift = b.is_ascii_uppercase();
        return Some((Some(seq.clone()), name, ctrl, shift, false, seq));
    }
    Some((Some(seq.clone()), seq.clone(), false, false, false, seq))
}

/// Split a cooked-mode stream chunk into the logical key units that Node's
/// `emitKeypressEvents` parser emits. Raw-mode input already arrives one byte
/// at a time, while a pipe commonly arrives as `"abc\n"` in one read; treating
/// that whole read as one key was the cooked-input half of #9692.
pub fn split_keypress_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b
            && index + 2 < bytes.len()
            && matches!(bytes[index + 1], b'[' | b'O')
        {
            let mut end = index + 2;
            while end < bytes.len() && end - index < 16 {
                if (0x40..=0x7e).contains(&bytes[end]) {
                    end += 1;
                    break;
                }
                end += 1;
            }
            chunks.push(bytes[index..end].to_vec());
            index = end;
            continue;
        }

        let width = match bytes[index] {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        let end = (index + width).min(bytes.len());
        if std::str::from_utf8(&bytes[index..end]).is_ok() {
            chunks.push(bytes[index..end].to_vec());
            index = end;
        } else {
            chunks.push(vec![bytes[index]]);
            index += 1;
        }
    }
    chunks
}

extern "C" fn emit_keypress_data(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let stream_scope = crate::gc::RuntimeHandleScope::new();
    let stream = stream_scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let chunk = stream_scope.root_nanbox_f64(chunk);
    let bytes = string_bytes(chunk.get_nanbox_f64());
    for key_chunk in split_keypress_chunks(&bytes) {
        let Some((str_arg, name, ctrl, shift, meta, seq)) = parse_keypress(&key_chunk) else {
            continue;
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let event = boxed_str(b"keypress");
        let event = scope.root_nanbox_f64(event);
        let first = str_arg
            .as_ref()
            .map(|s| boxed_str(s.as_bytes()))
            .unwrap_or_else(undefined);
        let first = scope.root_nanbox_f64(first);
        let key = build_keypress_object(&name, ctrl, shift, meta, &seq);
        let key = scope.root_nanbox_f64(key);
        let args = scope.root_raw_mut_ptr(js_array_alloc(0));
        let pushed = js_array_push_f64(args.get_raw_mut_ptr(), first.get_nanbox_f64());
        args.set_raw_mut_ptr(pushed);
        let pushed = js_array_push_f64(args.get_raw_mut_ptr(), key.get_nanbox_f64());
        args.set_raw_mut_ptr(pushed);
        let Some(raw) = raw_ptr_from_value(stream.get_nanbox_f64()) else {
            return undefined();
        };
        crate::node_stream::js_node_stream_method_emit_args(
            raw,
            event.get_nanbox_f64(),
            args.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
        );
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_emit_keypress_events_args(
    args: *const crate::array::ArrayHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(arg(args, 0));
    if raw_ptr_from_value(stream.get_nanbox_f64()).is_none() {
        return undefined();
    }
    let listener = js_closure_alloc(emit_keypress_data as *const u8, 1);
    js_closure_set_capture_f64(listener, 0, stream.get_nanbox_f64());
    let listener = scope.root_raw_const_ptr(listener as *const ClosureHeader);
    let Some(raw) = raw_ptr_from_value(stream.get_nanbox_f64()) else {
        return undefined();
    };
    if crate::os::is_process_stdin_handle(raw) {
        crate::os::enable_process_stdin_keypress_events(
            listener.get_raw_const_ptr::<ClosureHeader>() as i64,
        );
    } else {
        let event = scope.root_nanbox_f64(boxed_str(b"data"));
        let listener_value = f64::from_bits(
            JSValue::pointer(listener.get_raw_const_ptr::<ClosureHeader>() as *const u8).bits(),
        );
        crate::node_stream::js_node_stream_method_on(raw, event.get_nanbox_f64(), listener_value);
    }
    undefined()
}
