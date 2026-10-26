//! String-pattern replacement without the RegExp engine. Matching and template
//! offsets use UTF-16 units, including the interior of an astral character.
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string};

pub(crate) fn text<'a>(
    scope: &'a RuntimeHandleScope,
    value: &RuntimeHandle<'_>,
) -> RuntimeHandle<'a> {
    let primitive = if crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        scope.root_nanbox_f64(to_primitive(value, true))
    } else {
        scope.root_nanbox_f64(value.get_nanbox_f64())
    };
    if unsafe { crate::symbol::js_is_symbol(primitive.get_nanbox_f64()) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
    }
    let string = crate::value::js_jsvalue_to_string_coerce(primitive.get_nanbox_f64());
    super::js_string_addref(string);
    scope.root_string_ptr(string)
}
fn get(value: &RuntimeHandle<'_>, name: &[u8]) -> f64 {
    let key = super::canonical_key(name);
    let receiver = value.get_nanbox_f64();
    crate::proxy::js_reflect_get(receiver, js_nanbox_string(key as i64), receiver)
}
fn to_primitive(value: &RuntimeHandle<'_>, string_hint: bool) -> f64 {
    let scope = RuntimeHandleScope::new();
    let key = crate::symbol::well_known_symbol("toPrimitive");
    let receiver = value.get_nanbox_f64();
    let method = scope.root_nanbox_f64(crate::proxy::js_reflect_get(
        receiver,
        js_nanbox_pointer(key as i64),
        receiver,
    ));
    if !matches!(
        method.get_nanbox_u64(),
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) {
        if !callable(&method) {
            crate::collection_iter::throw_type_error("Symbol.toPrimitive is not callable");
        }
        let hint = if string_hint { b"string" } else { b"number" };
        let hint = super::js_string_from_bytes(hint.as_ptr(), 6);
        let result = call(
            &method,
            value.get_nanbox_f64(),
            &[js_nanbox_string(hint as i64)],
        );
        if crate::proxy::reflect_value_is_object(result) {
            crate::collection_iter::throw_type_error("Cannot convert object to primitive value");
        }
        return result;
    }
    let order = if string_hint {
        [b"toString".as_slice(), b"valueOf".as_slice()]
    } else {
        [b"valueOf".as_slice(), b"toString".as_slice()]
    };
    for name in order {
        let local = RuntimeHandleScope::new();
        let method = local.root_nanbox_f64(get(value, name));
        if callable(&method) {
            let result = call(&method, value.get_nanbox_f64(), &[]);
            if !crate::proxy::reflect_value_is_object(result) {
                return result;
            }
        }
    }
    crate::collection_iter::throw_type_error("Cannot convert object to primitive value")
}
fn boxed(value: &RuntimeHandle<'_>) -> f64 {
    value.with_const_ptr::<StringHeader, _>(|p| js_nanbox_string(p as i64))
}
fn units(value: &RuntimeHandle<'_>) -> Vec<u16> {
    value.with_const_ptr::<StringHeader, _>(|p| {
        let bytes = crate::regex::string_as_bytes(p);
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let (step, length, cp) = super::wtf8_step(bytes, i);
            if length == 2 {
                let cp = cp - 0x10000;
                out.extend_from_slice(&[0xD800 + (cp >> 10) as u16, 0xDC00 + (cp & 0x3FF) as u16]);
            } else {
                out.push(cp as u16);
            }
            i += step;
        }
        out
    })
}
fn finish(units: &[u16]) -> *mut StringHeader {
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        let cp = if (0xD800..=0xDBFF).contains(&unit)
            && units
                .get(i + 1)
                .is_some_and(|lo| (0xDC00..=0xDFFF).contains(lo))
        {
            i += 1;
            0x10000 + (((unit as u32 - 0xD800) << 10) | (units[i] as u32 - 0xDC00))
        } else {
            unit as u32
        };
        if (0xD800..=0xDFFF).contains(&cp) {
            bytes.extend_from_slice(&[
                0xE0 | (cp >> 12) as u8,
                0x80 | ((cp >> 6) & 0x3F) as u8,
                0x80 | (cp & 0x3F) as u8,
            ]);
        } else {
            bytes.extend_from_slice(
                char::from_u32(cp)
                    .unwrap()
                    .encode_utf8(&mut [0; 4])
                    .as_bytes(),
            );
        }
        i += 1;
    }
    super::js_string_from_builder_bytes(&bytes)
}
/// Linear search, with one operation-local prefix-length buffer. Overlapping
/// occurrences are skipped after a match, as replaceAll requires.
fn positions(input: &[u16], needle: &[u16], all: bool) -> Vec<usize> {
    if needle.is_empty() {
        return (0..=if all { input.len() } else { 0 }).collect();
    }
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0;
    for i in 1..needle.len() {
        while matched > 0 && needle[i] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[i] == needle[matched] {
            matched += 1;
        }
        prefix[i] = matched;
    }
    let mut out = Vec::new();
    matched = 0;
    for (i, unit) in input.iter().enumerate() {
        while matched > 0 && *unit != needle[matched] {
            matched = prefix[matched - 1];
        }
        if *unit == needle[matched] {
            matched += 1;
        }
        if matched == needle.len() {
            out.push(i + 1 - matched);
            if !all {
                break;
            }
            matched = 0;
        }
    }
    out
}
fn substitute(out: &mut Vec<u16>, template: &[u16], input: &[u16], start: usize, length: usize) {
    let mut i = 0;
    while i < template.len() {
        if template[i] == b'$' as u16 && i + 1 < template.len() {
            let part = match template[i + 1] {
                36 => Some(&template[i..i + 1]),
                38 => Some(&input[start..start + length]),
                96 => Some(&input[..start]),
                39 => Some(&input[start + length..]),
                _ => None,
            };
            if let Some(part) = part {
                out.extend_from_slice(part);
                i += 2;
                continue;
            }
        }
        out.push(template[i]);
        i += 1;
    }
}
pub(crate) fn callable(value: &RuntimeHandle<'_>) -> bool {
    if crate::proxy::proxy_wraps_callable(value.get_nanbox_f64()) {
        return true;
    }
    if !crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        return false;
    }
    let prototype = crate::object::builtin_prototype_value("Function");
    prototype.to_bits() == value.get_nanbox_u64()
}
/// The ordinary call keeps its register arguments. A callable Proxy must
/// receive the same explicit `this` through its apply trap.
pub(crate) fn call(method: &RuntimeHandle<'_>, this: f64, arguments: &[f64]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let args: Vec<_> = arguments
        .iter()
        .map(|v| scope.root_nanbox_f64(*v))
        .collect();
    if crate::proxy::js_proxy_is_proxy(method.get_nanbox_f64()) == 1 {
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(args.len() as u32));
        for arg in &args {
            let grown =
                array.with_mut_ptr(|p| crate::array::js_array_push_f64(p, arg.get_nanbox_f64()));
            array.set_raw_mut_ptr(grown);
        }
        let array =
            array.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| js_nanbox_pointer(p as i64));
        crate::proxy::js_proxy_apply(method.get_nanbox_f64(), this.get_nanbox_f64(), array)
    } else {
        let args: Vec<_> = args.iter().map(|v| v.get_nanbox_f64()).collect();
        unsafe {
            crate::closure::native_call_value_this(
                method.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(this.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            )
        }
    }
}
pub(crate) fn replace(all: bool, receiver: f64, search: f64, replacement: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let search = scope.root_nanbox_f64(search);
    let replacement = scope.root_nanbox_f64(replacement);
    let input = text(&scope, &receiver);
    let needle = text(&scope, &search);
    let functional = callable(&replacement);
    let template = if functional {
        None
    } else {
        Some(units(&text(&scope, &replacement)))
    };
    let input_units = units(&input);
    let needle_units = units(&needle);
    let matches = positions(&input_units, &needle_units, all);
    if matches.is_empty() {
        input.with_mut_ptr::<StringHeader, _>(|p| super::js_string_addref(p));
        return boxed(&input);
    }
    let mut out = Vec::new();
    let mut end = 0;
    for position in matches {
        out.extend_from_slice(&input_units[end..position]);
        if let Some(template) = &template {
            substitute(
                &mut out,
                template,
                &input_units,
                position,
                needle_units.len(),
            );
        } else {
            let result = call(
                &replacement,
                f64::from_bits(crate::value::TAG_UNDEFINED),
                &[boxed(&needle), position as f64, boxed(&input)],
            );
            let result = scope.root_nanbox_f64(result);
            out.extend_from_slice(&units(&text(&scope, &result)));
        }
        end = position + needle_units.len();
    }
    out.extend_from_slice(&input_units[end..]);
    js_nanbox_string(finish(&out) as i64)
}

pub(crate) fn split(receiver: f64, separator: f64, limit: f64) -> f64 {
    if let Some(parts) = super::split::split_string_by_string(receiver, separator, limit) {
        return parts;
    }
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let separator = scope.root_nanbox_f64(separator);
    let limit = scope.root_nanbox_f64(limit);
    let input = text(&scope, &receiver);
    let limit = if limit.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
        u32::MAX as usize
    } else {
        let primitive = if crate::proxy::reflect_value_is_object(limit.get_nanbox_f64()) {
            to_primitive(&limit, false)
        } else {
            limit.get_nanbox_f64()
        };
        if crate::value::JSValue::from_bits(primitive.to_bits()).is_bigint() {
            crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
        }
        let n = crate::builtins::js_number_coerce(primitive);
        if !n.is_finite() || n == 0.0 {
            0
        } else {
            n.trunc().rem_euclid(4_294_967_296.0) as usize
        }
    };
    // ToString(separator) precedes the zero-limit return, including its throws.
    let needle = text(&scope, &separator);
    let output = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let push = |value: f64| {
        let grown = output.with_mut_ptr(|p| crate::array::js_array_push_f64(p, value));
        output.set_raw_mut_ptr(grown);
    };
    if limit > 0 {
        if separator.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
            push(boxed(&input));
        } else {
            let input = units(&input);
            let needle = units(&needle);
            if needle.is_empty() {
                for unit in input.iter().take(limit) {
                    push(js_nanbox_string(finish(&[*unit]) as i64));
                }
            } else {
                let mut end = 0;
                let mut count = 0;
                for position in positions(&input, &needle, true) {
                    push(js_nanbox_string(finish(&input[end..position]) as i64));
                    count += 1;
                    end = position + needle.len();
                    if count == limit {
                        break;
                    }
                }
                if count < limit {
                    push(js_nanbox_string(finish(&input[end..]) as i64));
                }
            }
        }
    }
    output.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| js_nanbox_pointer(p as i64))
}
