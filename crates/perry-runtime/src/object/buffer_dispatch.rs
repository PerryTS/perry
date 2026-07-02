//! Buffer / Uint8Array runtime method dispatch (issue #639 followup).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;
use base64::Engine as _;

fn throw_buffer_type_error_with_code(message: &'static str, code: &'static str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn is_buffer_dispatch_number(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_number() || jsval.is_int32()
}

fn is_buffer_dispatch_string(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_string() || jsval.is_short_string()
}

fn buffer_dispatch_i32(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        jsval.as_int32()
    } else {
        value as i32
    }
}

fn buffer_write_encoding_tag_or_throw(value: f64) -> i32 {
    if !is_buffer_dispatch_string(value) {
        throw_buffer_type_error_with_code("Invalid Buffer encoding", "ERR_INVALID_ARG_TYPE");
    }
    if crate::buffer::js_buffer_is_encoding(value) == 0 {
        throw_unknown_encoding(value);
    }
    crate::buffer::js_encoding_tag_from_value(value)
}

/// Throw `TypeError [ERR_UNKNOWN_ENCODING]: Unknown encoding: <name>` with
/// the offending encoding name spelled out, matching node's message. `value`
/// is the rejected encoding argument (a string).
fn throw_unknown_encoding(value: f64) -> ! {
    let name = crate::fs::validate::read_js_string_pub(value);
    let message = format!("Unknown encoding: {name}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_UNKNOWN_ENCODING");
}

/// True when `value` is a `Buffer` or `Uint8Array` (the acceptance set of
/// node's `Buffer.prototype.copy/compare/equals` target argument).
fn is_buffer_or_uint8array(value: f64) -> bool {
    let bits = value.to_bits() as i64;
    if crate::buffer::js_buffer_is_buffer(bits) == 1 {
        return true;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    raw >= 0x1000 && crate::typedarray::lookup_typed_array_kind(raw).is_some()
}

/// `Buffer.prototype.copy/compare/equals` reject a non-`Buffer`/`Uint8Array`
/// target with `TypeError [ERR_INVALID_ARG_TYPE]` — and without this guard a
/// number/boolean argument's bit pattern is masked into a bogus pointer and
/// dereferenced (segfault). `name` is node's parameter name (`target` /
/// `otherBuffer`).
fn validate_buffer_target(value: f64, name: &str) {
    if is_buffer_or_uint8array(value) {
        return;
    }
    crate::validators::throw_invalid_arg_type(name, "an instance of Buffer or Uint8Array", value);
}

/// Dispatch a Buffer / Uint8Array instance method call. Receiver address
/// is the raw heap pointer (already stripped of NaN-box tags). Routes
/// the Node-style numeric read/write/search/swap method family through
/// `crate::buffer` helpers; unknown methods return undefined.
/// Issue #639 followup: list of method names recognized by `dispatch_buffer_method`.
/// Used by `js_object_get_field_by_name`'s Buffer arm to decide whether a
/// non-length property read should synthesize a bound-method closure (so
/// duck-type tests like `typeof v.readUInt8 === "function"` pass and a
/// subsequent call dispatches through `js_native_call_method`).
///
/// Keep this list aligned with the `match method_name` arms below — every
/// arm there should be reachable from a method-as-value read.
pub fn is_buffer_method_name(name: &str) -> bool {
    matches!(
        name,
        "toString"
            | "inspect"
            | "slice"
            | "subarray"
            | "set"
            | "copy"
            | "write"
            | "toJSON"
            | "export"
            | "toCryptoKey"
            | "fill"
            | "equals"
            | "compare"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "at"
            | "swap16"
            | "swap32"
            | "swap64"
            // Issue #1206: explicit iterator-protocol surface.
            | "values"
            | "keys"
            | "entries"
            // Object.prototype methods exposed on Buffer instances so
            // safer-buffer's `if (buffer.hasOwnProperty(...))` probe (and
            // similar duck-type tests in express / body-parser dependents)
            // resolve to a callable, not undefined. Without these,
            // `typeof buf.hasOwnProperty` is `"undefined"` and the
            // subsequent invocation throws "buffer.hasOwnProperty is not
            // a function" at express startup.
            | "hasOwnProperty"
            | "propertyIsEnumerable"
            | "valueOf"
            | "isPrototypeOf"
            | "toLocaleString"
            | "readUInt8"
            | "readUint8"
            | "readInt8"
            | "readUInt16BE"
            | "readUint16BE"
            | "readUInt16LE"
            | "readUint16LE"
            | "readInt16BE"
            | "readInt16LE"
            | "readUInt32BE"
            | "readUint32BE"
            | "readUInt32LE"
            | "readUint32LE"
            | "readInt32BE"
            | "readInt32LE"
            | "readFloatBE"
            | "readFloatLE"
            | "readDoubleBE"
            | "readDoubleLE"
            | "readBigInt64BE"
            | "readBigInt64LE"
            | "readBigUInt64BE"
            | "readBigUint64BE"
            | "readBigUInt64LE"
            | "readBigUint64LE"
            | "readUIntBE"
            | "readUintBE"
            | "readUIntLE"
            | "readUintLE"
            | "readIntBE"
            | "readIntLE"
            | "writeUInt8"
            | "writeUint8"
            | "writeInt8"
            | "writeUInt16BE"
            | "writeUint16BE"
            | "writeUInt16LE"
            | "writeUint16LE"
            | "writeInt16BE"
            | "writeInt16LE"
            | "writeUInt32BE"
            | "writeUint32BE"
            | "writeUInt32LE"
            | "writeUint32LE"
            | "writeInt32BE"
            | "writeInt32LE"
            | "writeFloatBE"
            | "writeFloatLE"
            | "writeDoubleBE"
            | "writeDoubleLE"
            | "writeBigInt64BE"
            | "writeBigInt64LE"
            | "writeBigUInt64BE"
            | "writeBigUint64BE"
            | "writeBigUInt64LE"
            | "writeBigUint64LE"
            | "writeUIntBE"
            | "writeUintBE"
            | "writeUIntLE"
            | "writeUintLE"
            | "writeIntBE"
            | "writeIntLE"
            // #2901: TC39 Uint8Array base64/hex instance conversion APIs.
            | "toBase64"
            | "toHex"
            | "setFromBase64"
            | "setFromHex"
            // #2879: typed-array mutators that reach buffer dispatch for the
            // Uint8Array/Buffer shape.
            | "copyWithin"
            // #2878: DataView numeric accessors. These resolve as bound-method
            // values on a DataView-marked buffer (so `typeof dv.getUint8 ===
            // "function"`); the call routes through `dispatch_buffer_method`.
            | "getInt8"
            | "getUint8"
            | "getInt16"
            | "getUint16"
            | "getInt32"
            | "getUint32"
            | "getFloat32"
            | "getFloat64"
            | "setInt8"
            | "setUint8"
            | "setInt16"
            | "setUint16"
            | "setInt32"
            | "setUint32"
            | "setFloat32"
            | "setFloat64"
            // #4365: DataView BigInt64/BigUint64 accessors (8-byte BigInt
            // read/write). Route through `dispatch_buffer_method` like the
            // other DataView numeric methods.
            | "getBigInt64"
            | "getBigUint64"
            | "setBigInt64"
            | "setBigUint64"
    )
}

unsafe fn buffer_secret_export_format(bits: f64) -> Option<String> {
    let raw = bits.to_bits();
    if (raw >> 48) as u16 == 0x7FFC {
        return None;
    }
    let obj = (raw & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    if (obj as usize) < 0x1000 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"format".as_ptr(), 6);
    let val = js_object_get_field_by_name(obj, key);
    let vbits = val.bits();
    if (vbits >> 48) as u16 != 0x7FFF {
        return None;
    }
    let ptr = (vbits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(
        (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
        (*ptr).byte_len as usize,
    );
    Some(String::from_utf8_lossy(bytes).to_ascii_lowercase())
}

unsafe fn secret_key_jwk_object(buf_ptr: *mut crate::buffer::BufferHeader) -> f64 {
    let bytes = std::slice::from_raw_parts(
        (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>()),
        (*buf_ptr).length as usize,
    );
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let obj = js_object_alloc(0, 2);
    let kty_key = crate::string::js_string_from_bytes(b"kty".as_ptr(), 3);
    let kty_val = crate::string::js_string_from_bytes(b"oct".as_ptr(), 3);
    js_object_set_field_by_name(
        obj,
        kty_key,
        f64::from_bits(JSValue::string_ptr(kty_val).bits()),
    );
    let k_key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
    let k_val = crate::string::js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    js_object_set_field_by_name(
        obj,
        k_key,
        f64::from_bits(JSValue::string_ptr(k_val).bits()),
    );
    f64::from_bits(JSValue::pointer(obj as *mut u8).bits())
}

unsafe fn js_string_from_value(bits: f64) -> Option<String> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if top16 != 0x7FFF {
        return None;
    }
    let ptr = (raw & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(
        (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
        (*ptr).byte_len as usize,
    );
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

unsafe fn object_field_string_value(obj_bits: f64, name: &[u8]) -> Option<String> {
    let raw = obj_bits.to_bits();
    if (raw >> 48) as u16 != 0x7FFD {
        return None;
    }
    let obj = (raw & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    if (obj as usize) < 0x1000 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = js_object_get_field_by_name(obj, key);
    js_string_from_value(f64::from_bits(val.bits()))
}

unsafe fn secret_to_crypto_key(addr: usize, algorithm_bits: f64) -> f64 {
    let name = js_string_from_value(algorithm_bits)
        .or_else(|| object_field_string_value(algorithm_bits, b"name"))
        .unwrap_or_default();
    let upper = name.to_ascii_uppercase();
    let algo_id = match upper.as_str() {
        "HMAC" => 1,
        "AES-GCM" => 2,
        "AES-KW" => 3,
        "AES-CBC" => 4,
        "AES-CTR" => 5,
        "HKDF" => 6,
        "PBKDF2" => 7,
        "CHACHA20-POLY1305" => 15,
        "KMAC128" => 23,
        "KMAC256" => 24,
        "AES-OCB" => 25,
        _ => return f64::from_bits(JSValue::undefined().bits()),
    };
    let hash_name = object_field_string_value(algorithm_bits, b"hash")
        .or_else(|| {
            let raw = algorithm_bits.to_bits();
            if (raw >> 48) as u16 != 0x7FFD {
                return None;
            }
            let obj = (raw & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            let key = crate::string::js_string_from_bytes(b"hash".as_ptr(), 4);
            let hash_val = js_object_get_field_by_name(obj, key);
            object_field_string_value(f64::from_bits(hash_val.bits()), b"name")
        })
        .unwrap_or_else(|| "SHA-256".to_string());
    let hash_id = match hash_name.to_ascii_uppercase().replace('-', "").as_str() {
        "SHA1" => 1,
        "SHA256" => 2,
        "SHA384" => 3,
        "SHA512" => 4,
        _ => 2,
    };
    crate::buffer::mark_as_crypto_key(addr, algo_id, hash_id, 1);
    f64::from_bits(JSValue::pointer(addr as *mut u8).bits())
}

pub unsafe fn dispatch_buffer_method(
    addr: usize,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let buf_f64 = f64::from_bits(JSValue::pointer(addr as *mut u8).bits());
    let buf_ptr = addr as *mut crate::buffer::BufferHeader;
    let args = if !args_ptr.is_null() && args_len > 0 {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let arg_i32 = |i: usize| -> i32 {
        if i < args.len() {
            args[i] as i32
        } else {
            0
        }
    };
    let arg_or_zero = |i: usize| -> f64 {
        if i < args.len() {
            args[i]
        } else {
            0.0
        }
    };
    let i32_bool = |b: i32| f64::from_bits(JSValue::bool(b != 0).bits());
    let i32_num = |n: i32| n as f64;
    // ToIntegerOrInfinity for an ArrayBuffer/SharedArrayBuffer slice index:
    // full ToNumber (runs `valueOf`/`toString`, throws on a Symbol) then
    // truncate toward zero, saturating into i32 so `js_buffer_slice` can apply
    // the spec's relative-index clamp to [0, len].
    let ab_slice_index = |v: f64| -> i32 {
        let n = crate::builtins::js_number_coerce(v);
        if n.is_nan() {
            return 0;
        }
        let t = n.trunc();
        if t >= i32::MAX as f64 {
            i32::MAX
        } else if t <= i32::MIN as f64 {
            i32::MIN
        } else {
            t as i32
        }
    };

    // DataView numeric accessors (#2878): getInt8/getUint16/setFloat64/… The
    // receiver is a BufferHeader marked as a DataView. Endianness defaults to
    // big-endian; the optional trailing `littleEndian` arg flips it. Routed
    // here (before the Buffer method match) so DataView setters use ToIntN/
    // ToUintN value-wrap semantics rather than the Buffer value-range throw.
    if crate::buffer::is_data_view(addr) {
        let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
        if let Some(suffix) = method_name.strip_prefix("get") {
            if let Some(kind) = crate::buffer::DataViewKind::from_method_suffix(suffix) {
                let little = args.len() >= 2 && truthy(args[1]);
                return crate::buffer::js_data_view_get(buf_f64, arg_or_zero(0), kind, little);
            }
        } else if let Some(suffix) = method_name.strip_prefix("set") {
            if let Some(kind) = crate::buffer::DataViewKind::from_method_suffix(suffix) {
                let little = args.len() >= 3 && truthy(args[2]);
                return crate::buffer::js_data_view_set(
                    buf_f64,
                    arg_or_zero(0),
                    arg_or_zero(1),
                    kind,
                    little,
                );
            }
        }
    }

    match method_name {
        "length" => crate::buffer::js_buffer_length(buf_ptr) as f64,
        "toString" if crate::buffer::is_secret_key(addr) => {
            let s = crate::string::js_string_from_bytes(b"[object KeyObject]".as_ptr(), 18);
            f64::from_bits(JSValue::string_ptr(s).bits())
        }
        "toString" => {
            let enc = if !args.is_empty() {
                crate::buffer::js_encoding_tag_from_value(args[0])
            } else {
                0
            };
            let str_ptr = if args.len() >= 2 {
                let len = (*buf_ptr).length as i32;
                let start = arg_i32(1);
                let end = if args.len() >= 3 { arg_i32(2) } else { len };
                crate::buffer::js_buffer_to_string_range(buf_ptr, enc, start, end)
            } else {
                crate::buffer::js_buffer_to_string(buf_ptr, enc)
            };
            f64::from_bits(JSValue::string_ptr(str_ptr).bits())
        }
        // TC39 Uint8Array base64/hex conversion APIs (#2901). Perry aliases
        // Uint8Array → Buffer, so these instance methods reach buffer dispatch.
        "toBase64" => {
            let opts = arg_or_zero(0);
            let s = crate::buffer::js_u8_to_base64(addr as i64, opts);
            f64::from_bits(JSValue::string_ptr(s).bits())
        }
        "toHex" => {
            let s = crate::buffer::js_u8_to_hex(addr as i64);
            f64::from_bits(JSValue::string_ptr(s).bits())
        }
        "setFromBase64" => {
            let str_handle = arg_or_zero(0).to_bits() as i64;
            let opts = arg_or_zero(1);
            crate::buffer::js_u8_set_from_base64(addr as i64, str_handle, opts)
        }
        "setFromHex" => {
            let str_handle = arg_or_zero(0).to_bits() as i64;
            crate::buffer::js_u8_set_from_hex(addr as i64, str_handle)
        }
        "inspect" => {
            crate::builtins::js_util_inspect(buf_f64, f64::from_bits(crate::value::TAG_UNDEFINED))
        }
        "slice" | "subarray" => {
            let len = (*buf_ptr).length as i32;
            let (start, end) = if crate::buffer::is_array_buffer(addr)
                || crate::buffer::is_shared_array_buffer(addr)
            {
                // ArrayBuffer / SharedArrayBuffer.prototype.slice: ToIntegerOrInfinity
                // on `start` (always) and `end` (defaults to len when undefined),
                // running an object arg's `valueOf` in left-to-right order. Plain
                // `arg_i32` would `as i32`-cast a boxed object to garbage and skip
                // the coercion entirely (test262 slice/number-conversion,
                // end-default-if-undefined). `js_buffer_slice` then clamps.
                let s = ab_slice_index(arg_or_zero(0));
                let e = if args.len() < 2 || JSValue::from_bits(args[1].to_bits()).is_undefined() {
                    len
                } else {
                    ab_slice_index(args[1])
                };
                (s, e)
            } else {
                let s = arg_i32(0);
                let e = if args.len() >= 2 { arg_i32(1) } else { len };
                (s, e)
            };
            let result = crate::buffer::js_buffer_slice(buf_ptr, start, end);
            // #2877: `ArrayBuffer.prototype.slice` returns a NEW ArrayBuffer
            // (a copy), so mark the result so `ArrayBuffer.isView(slice)` is
            // false and a subsequent `new Uint8Array(slice)` aliases it.
            if crate::buffer::is_array_buffer(addr) {
                crate::buffer::mark_as_array_buffer(result as usize);
            } else if crate::buffer::is_shared_array_buffer(addr) {
                crate::buffer::mark_as_shared_array_buffer(result as usize);
            }
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "set" => {
            let source = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            crate::buffer::js_buffer_set_from_value(buf_ptr, source, arg_or_zero(1))
        }
        // #2879: `Uint8Array.prototype.copyWithin` — Buffer/Uint8Array elements
        // are single bytes, so copy at byte granularity. Returns the receiver.
        "copyWithin" => {
            let len = (*buf_ptr).length as i64;
            let rel = |v: f64| -> i64 {
                let n = crate::value::JSValue::from_bits(v.to_bits()).to_number();
                if n.is_nan() {
                    return 0;
                }
                if !n.is_finite() {
                    return if n > 0.0 { len } else { 0 };
                }
                let idx = n.trunc() as i64;
                if idx < 0 {
                    (len + idx).max(0)
                } else {
                    idx.min(len)
                }
            };
            let to = rel(arg_or_zero(0));
            let from = rel(arg_or_zero(1));
            let final_ = if args.len() >= 3 { rel(args[2]) } else { len };
            let count = (final_ - from).min(len - to);
            if count > 0 {
                let data = crate::buffer::buffer_data_mut(buf_ptr);
                let block: Vec<u8> = (0..count as usize)
                    .map(|i| *data.add(from as usize + i))
                    .collect();
                for (i, b) in block.into_iter().enumerate() {
                    *data.add(to as usize + i) = b;
                }
            }
            buf_f64
        }
        // Issue #1206: explicit iterator-protocol surface. Each helper
        // returns a Buffer-iterator object whose `.next()` is dispatched
        // through `dispatch_buffer_iterator_method` in `iter.rs`.
        "values" => crate::buffer::js_buffer_values(buf_f64),
        "keys" => crate::buffer::js_buffer_keys(buf_f64),
        "entries" => crate::buffer::js_buffer_entries(buf_f64),
        // `src.copy(dst, targetStart?, sourceStart?, sourceEnd?)` — mirrors
        // Node's Buffer.prototype.copy. Returns the number of bytes copied.
        "copy" => {
            if args.is_empty() {
                validate_buffer_target(f64::from_bits(crate::value::TAG_UNDEFINED), "target");
            }
            validate_buffer_target(args[0], "target");
            let dst_bits = args[0].to_bits();
            let dst_addr = if (dst_bits >> 48) >= 0x7FF8 {
                dst_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                dst_bits
            };
            let dst_ptr = dst_addr as *mut crate::buffer::BufferHeader;
            let target_start = if args.len() >= 2 { arg_i32(1) } else { 0 };
            let source_start = if args.len() >= 3 { arg_i32(2) } else { 0 };
            let source_end = if args.len() >= 4 {
                arg_i32(3)
            } else {
                (*buf_ptr).length as i32
            };
            crate::buffer::js_buffer_copy(buf_ptr, dst_ptr, target_start, source_start, source_end)
                as f64
        }
        "toJSON" => crate::buffer::js_buffer_to_json(buf_f64),
        // `buf.write(string, offset?, length?, encoding?)` — writes the
        // utf8/hex/base64 encoding of `string` into `buf` at `offset`.
        // Returns the number of bytes written.
        "write" => {
            if args.is_empty() || !is_buffer_dispatch_string(args[0]) {
                throw_buffer_type_error_with_code(
                    "argument must be a string",
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            let str_bits = args[0].to_bits();
            let str_addr = if (str_bits >> 48) >= 0x7FF8 {
                str_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                str_bits
            };
            let str_ptr = str_addr as *const crate::string::StringHeader;
            let mut offset = 0;
            let mut arg_index = 1;
            let mut enc = 0;
            if args.len() >= 2 {
                if args.len() == 2 && is_buffer_dispatch_string(args[1]) {
                    enc = buffer_write_encoding_tag_or_throw(args[1]);
                    arg_index = 2;
                } else if is_buffer_dispatch_number(args[1]) {
                    offset = buffer_dispatch_i32(args[1]);
                    arg_index = 2;
                } else {
                    throw_buffer_type_error_with_code(
                        "Invalid Buffer offset",
                        "ERR_INVALID_ARG_TYPE",
                    );
                }
            }
            // Detect trailing encoding arg (string) vs length arg (number).
            // Common forms: write(str), write(str, offset), write(str, offset, enc),
            // write(str, offset, length, enc).
            let max_len = if arg_index < args.len() {
                if is_buffer_dispatch_string(args[arg_index]) {
                    enc = buffer_write_encoding_tag_or_throw(args[arg_index]);
                    (*buf_ptr).length as i32 - offset
                } else if is_buffer_dispatch_number(args[arg_index]) {
                    let len = buffer_dispatch_i32(args[arg_index]);
                    arg_index += 1;
                    if arg_index < args.len() {
                        enc = buffer_write_encoding_tag_or_throw(args[arg_index]);
                    }
                    len
                } else {
                    throw_buffer_type_error_with_code(
                        "Invalid Buffer length",
                        "ERR_INVALID_ARG_TYPE",
                    );
                }
            } else {
                (*buf_ptr).length as i32 - offset
            };
            crate::buffer::js_buffer_write_len(buf_ptr, str_ptr, offset, max_len, enc) as f64
        }
        "export" if crate::buffer::is_secret_key(addr) => {
            let format = args.first().and_then(|v| buffer_secret_export_format(*v));
            if matches!(format.as_deref(), Some("jwk")) {
                return secret_key_jwk_object(buf_ptr);
            }
            let bytes = std::slice::from_raw_parts(
                (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>()),
                (*buf_ptr).length as usize,
            );
            let out = crate::buffer::buffer_alloc(bytes.len() as u32);
            if !out.is_null() {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    crate::buffer::buffer_data_mut(out),
                    bytes.len(),
                );
                (*out).length = bytes.len() as u32;
                // Node returns a `Buffer` (Uint8Array subclass) here so
                // `instanceof Uint8Array` must hold on the result.
                crate::buffer::mark_as_uint8array(out as usize);
            }
            f64::from_bits(JSValue::pointer(out as *mut u8).bits())
        }
        "toCryptoKey" if crate::buffer::is_secret_key(addr) && !args.is_empty() => {
            secret_to_crypto_key(addr, args[0])
        }
        "fill" => {
            let len = (*buf_ptr).length as i32;
            let start = if args.len() >= 2 { arg_i32(1) } else { 0 };
            let end = if args.len() >= 3 { arg_i32(2) } else { len };
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let enc = if args.len() >= 4 {
                crate::buffer::js_encoding_tag_from_value(args[3])
            } else {
                0
            };
            let result = crate::buffer::js_buffer_fill_value_range(buf_ptr, value, start, end, enc);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "equals" => {
            validate_buffer_target(
                args.first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
                "otherBuffer",
            );
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                other_bits
            };
            let other = other_addr as *const crate::buffer::BufferHeader;
            i32_bool(crate::buffer::js_buffer_equals(buf_ptr, other))
        }
        "compare" => {
            validate_buffer_target(
                args.first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
                "target",
            );
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                other_bits
            };
            let other = other_addr as *const crate::buffer::BufferHeader;
            if args.len() >= 2 {
                let target_len = if other.is_null() {
                    0
                } else {
                    (*other).length as i32
                };
                let source_len = (*buf_ptr).length as i32;
                let arg_i32_or = |i: usize, default: i32| -> i32 {
                    if i < args.len() {
                        let value = JSValue::from_bits(args[i].to_bits());
                        if value.is_undefined() {
                            default
                        } else {
                            args[i] as i32
                        }
                    } else {
                        default
                    }
                };
                i32_num(crate::buffer::js_buffer_compare_range(
                    buf_ptr,
                    other,
                    arg_i32_or(1, 0),
                    arg_i32_or(2, target_len),
                    arg_i32_or(3, 0),
                    arg_i32_or(4, source_len),
                ))
            } else {
                i32_num(crate::buffer::js_buffer_compare(buf_ptr, other))
            }
        }
        "indexOf" => {
            let enc = if args.len() >= 3 {
                crate::buffer::js_encoding_tag_from_value(args[2])
            } else {
                0
            };
            i32_num(crate::buffer::js_buffer_index_of_enc(
                buf_f64,
                arg_or_zero(0),
                arg_i32(1),
                enc,
            ))
        }
        "lastIndexOf" => {
            let len = (*buf_ptr).length as i32;
            let start = if args.len() >= 2 { arg_i32(1) } else { len - 1 };
            let enc = if args.len() >= 3 {
                crate::buffer::js_encoding_tag_from_value(args[2])
            } else {
                0
            };
            i32_num(crate::buffer::js_buffer_last_index_of_enc(
                buf_f64,
                arg_or_zero(0),
                start,
                enc,
            ))
        }
        "includes" => {
            let enc = if args.len() >= 3 {
                crate::buffer::js_encoding_tag_from_value(args[2])
            } else {
                0
            };
            i32_bool(crate::buffer::js_buffer_includes_enc(
                buf_f64,
                arg_or_zero(0),
                arg_i32(1),
                enc,
            ))
        }
        // `buf.at(i)` — supports negative indices like Array.prototype.at.
        "at" => {
            let len = (*buf_ptr).length as i32;
            let mut idx = arg_i32(0);
            if idx < 0 {
                idx += len;
            }
            if idx < 0 || idx >= len {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            crate::buffer::js_buffer_get(buf_ptr, idx) as f64
        }
        "swap16" => {
            crate::buffer::js_buffer_swap16(buf_f64);
            buf_f64
        }
        "swap32" => {
            crate::buffer::js_buffer_swap32(buf_f64);
            buf_f64
        }
        "swap64" => {
            crate::buffer::js_buffer_swap64(buf_f64);
            buf_f64
        }
        // Synthetic method emitted by lower.rs for `crypto.getRandomValues(buf)`.
        "$$cryptoFillRandom" => crate::buffer::js_buffer_fill_random(buf_f64),
        "readUInt8" | "readUint8" => crate::buffer::js_buffer_read_uint8(buf_f64, arg_i32(0)),
        "readInt8" => crate::buffer::js_buffer_read_int8(buf_f64, arg_i32(0)),
        "readUInt16BE" | "readUint16BE" => {
            crate::buffer::js_buffer_read_uint16_be(buf_f64, arg_i32(0))
        }
        "readUInt16LE" | "readUint16LE" => {
            crate::buffer::js_buffer_read_uint16_le(buf_f64, arg_i32(0))
        }
        "readInt16BE" => crate::buffer::js_buffer_read_int16_be(buf_f64, arg_i32(0)),
        "readInt16LE" => crate::buffer::js_buffer_read_int16_le(buf_f64, arg_i32(0)),
        "readUInt32BE" | "readUint32BE" => {
            crate::buffer::js_buffer_read_uint32_be(buf_f64, arg_i32(0))
        }
        "readUInt32LE" | "readUint32LE" => {
            crate::buffer::js_buffer_read_uint32_le(buf_f64, arg_i32(0))
        }
        "readInt32BE" => crate::buffer::js_buffer_read_int32_be(buf_f64, arg_i32(0)),
        "readInt32LE" => crate::buffer::js_buffer_read_int32_le(buf_f64, arg_i32(0)),
        "readFloatBE" => crate::buffer::js_buffer_read_float_be(buf_f64, arg_i32(0)),
        "readFloatLE" => crate::buffer::js_buffer_read_float_le(buf_f64, arg_i32(0)),
        "readDoubleBE" => crate::buffer::js_buffer_read_double_be(buf_f64, arg_i32(0)),
        "readDoubleLE" => crate::buffer::js_buffer_read_double_le(buf_f64, arg_i32(0)),
        "readBigInt64BE" => crate::buffer::js_buffer_read_bigint64_be(buf_f64, arg_i32(0)),
        "readBigInt64LE" => crate::buffer::js_buffer_read_bigint64_le(buf_f64, arg_i32(0)),
        "readBigUInt64BE" | "readBigUint64BE" => {
            crate::buffer::js_buffer_read_biguint64_be(buf_f64, arg_i32(0))
        }
        "readBigUInt64LE" | "readBigUint64LE" => {
            crate::buffer::js_buffer_read_biguint64_le(buf_f64, arg_i32(0))
        }
        "writeUInt8" | "writeUint8" => {
            crate::buffer::js_buffer_write_uint8(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 1) as f64
        }
        "writeInt8" => {
            crate::buffer::js_buffer_write_int8(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 1) as f64
        }
        "writeUInt16BE" | "writeUint16BE" => {
            crate::buffer::js_buffer_write_uint16_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeUInt16LE" | "writeUint16LE" => {
            crate::buffer::js_buffer_write_uint16_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeInt16BE" => {
            crate::buffer::js_buffer_write_int16_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeInt16LE" => {
            crate::buffer::js_buffer_write_int16_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeUInt32BE" | "writeUint32BE" => {
            crate::buffer::js_buffer_write_uint32_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeUInt32LE" | "writeUint32LE" => {
            crate::buffer::js_buffer_write_uint32_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeInt32BE" => {
            crate::buffer::js_buffer_write_int32_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeInt32LE" => {
            crate::buffer::js_buffer_write_int32_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeFloatBE" => {
            crate::buffer::js_buffer_write_float_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeFloatLE" => {
            crate::buffer::js_buffer_write_float_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeDoubleBE" => {
            crate::buffer::js_buffer_write_double_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeDoubleLE" => {
            crate::buffer::js_buffer_write_double_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigInt64BE" => {
            crate::buffer::js_buffer_write_bigint64_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigInt64LE" => {
            crate::buffer::js_buffer_write_bigint64_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigUInt64BE" | "writeBigUint64BE" => {
            crate::buffer::js_buffer_write_biguint64_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigUInt64LE" | "writeBigUint64LE" => {
            crate::buffer::js_buffer_write_biguint64_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        // Variable byteLength forms (Node-spec: byteLength 1..=6).
        // ObjectId / BSON drivers rely on these for the 3-byte counter.
        "readUIntBE" | "readUintBE" => {
            crate::buffer::js_buffer_read_uint_be(buf_f64, arg_i32(0), arg_i32(1))
        }
        "readUIntLE" | "readUintLE" => {
            crate::buffer::js_buffer_read_uint_le(buf_f64, arg_i32(0), arg_i32(1))
        }
        "readIntBE" => crate::buffer::js_buffer_read_int_be(buf_f64, arg_i32(0), arg_i32(1)),
        "readIntLE" => crate::buffer::js_buffer_read_int_le(buf_f64, arg_i32(0), arg_i32(1)),
        "writeUIntBE" | "writeUintBE" => {
            crate::buffer::js_buffer_write_uint_be(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeUIntLE" | "writeUintLE" => {
            crate::buffer::js_buffer_write_uint_le(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeIntBE" => {
            crate::buffer::js_buffer_write_int_be(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeIntLE" => {
            crate::buffer::js_buffer_write_int_le(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        // ── Object.prototype fallbacks on Buffer instances ──
        // safer-buffer (loaded by express) probes Buffer instances with
        // `if (buffer.hasOwnProperty(...))`. Pre-fix every non-buffer-specific
        // method read returned undefined, so the call threw
        // "buffer.hasOwnProperty is not a function". Mirror the generic
        // ObjectHeader behaviour wired up in PR #978: hasOwnProperty checks
        // numeric indices against the buffer length (Node spec — indexed
        // bytes are own properties, `length` is on the prototype), and
        // the remaining Object.prototype methods get spec-shaped stubs.
        "hasOwnProperty" => {
            let key_is_own = if args.is_empty() {
                false
            } else {
                let key_bits = args[0].to_bits();
                if (key_bits >> 48) == 0x7FFF {
                    // string key
                    let sptr =
                        (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader;
                    if sptr.is_null() {
                        false
                    } else {
                        let slen = (*sptr).byte_len as usize;
                        let sdata =
                            (sptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let bytes = std::slice::from_raw_parts(sdata, slen);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            // Only numeric-string indices that are in bounds
                            // count as own properties for Buffer/Uint8Array.
                            if let Ok(idx) = s.parse::<u32>() {
                                let buf_len = (*buf_ptr).length;
                                idx < buf_len
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                } else if (key_bits >> 48) == 0x7FFE {
                    // int32 key
                    let idx = (key_bits & 0xFFFF_FFFF) as i32;
                    let buf_len = (*buf_ptr).length as i32;
                    idx >= 0 && idx < buf_len
                } else if !(0x7FF8..=0x7FFF).contains(&(key_bits >> 48)) {
                    // raw f64 numeric key (NaN-boxing tags occupy 0x7FF8..=0x7FFF)
                    let n = args[0];
                    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 {
                        let idx = n as u32;
                        let buf_len = (*buf_ptr).length;
                        idx < buf_len
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            i32_bool(key_is_own as i32)
        }
        "propertyIsEnumerable" => {
            // Same key→own check as hasOwnProperty; indexed bytes on a
            // Buffer are enumerable own data properties.
            let key_is_own = if args.is_empty() {
                false
            } else {
                let key_bits = args[0].to_bits();
                if (key_bits >> 48) == 0x7FFF {
                    let sptr =
                        (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader;
                    if sptr.is_null() {
                        false
                    } else {
                        let slen = (*sptr).byte_len as usize;
                        let sdata =
                            (sptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let bytes = std::slice::from_raw_parts(sdata, slen);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            if let Ok(idx) = s.parse::<u32>() {
                                let buf_len = (*buf_ptr).length;
                                idx < buf_len
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                } else if (key_bits >> 48) == 0x7FFE {
                    let idx = (key_bits & 0xFFFF_FFFF) as i32;
                    let buf_len = (*buf_ptr).length as i32;
                    idx >= 0 && idx < buf_len
                } else if !args[0].is_nan() {
                    let n = args[0];
                    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 {
                        let idx = n as u32;
                        let buf_len = (*buf_ptr).length;
                        idx < buf_len
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            i32_bool(key_is_own as i32)
        }
        // `buf.valueOf()` returns the Buffer itself in Node (Uint8Array
        // inherits the no-op valueOf from Object.prototype, but for the
        // duck-test usage in safer-buffer/express-graph the receiver
        // round-trip is what matters).
        "valueOf" => f64::from_bits(JSValue::pointer(addr as *mut u8).bits()),
        // `buf.toLocaleString()` — Node delegates to toString() with no
        // args, which yields the utf8 decode. Match that.
        "toLocaleString" => {
            let str_ptr = crate::buffer::js_buffer_to_string(buf_ptr, 0);
            f64::from_bits(JSValue::string_ptr(str_ptr).bits())
        }
        // `buf.isPrototypeOf(other)` — buffers aren't prototype objects in
        // user code, so this is always false (matches Node when `buf` is
        // a Buffer instance rather than `Buffer.prototype`).
        "isPrototypeOf" => i32_bool(0),
        // A `Uint8Array` / `Uint8ClampedArray` (and Node `Buffer`, a Uint8Array
        // subclass) reaches this buffer dispatcher via the dynamic
        // `(ta as any).method(...)` path — `handle_methods` routes a
        // registered-buffer receiver here before the `%TypedArray%.prototype`
        // tower. The arms above only implement the Node `Buffer` API surface, so
        // the inherited `%TypedArray%.prototype` iteration methods
        // (`every`/`some`/`map`/`forEach`/`find`/`reduce`/…) fell through to this
        // catch-all and silently returned `undefined` — never validating the
        // callback (#5591: `every`/`some` must throw a `TypeError` for a
        // non-callable callback) nor iterating. Delegate any method the Buffer
        // API doesn't claim to the shared uint8 typed-array dispatcher (it
        // validates callbacks and reads the typed store); it returns `None` for
        // names it doesn't implement, preserving the `undefined` fallback.
        _ => {
            if super::typed_array_proto_thunks::is_typed_array_buffer(addr) {
                if let Some(r) = super::typed_array_proto_thunks::dispatch_uint8_buffer_method(
                    addr,
                    method_name,
                    args,
                ) {
                    return r;
                }
            }
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}
