//! Focused `util.parseArgs(config)` support for Node-compatible CLI option
//! parsing.

use std::collections::HashMap;

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_keys, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, js_string_materialize_to_heap, StringHeader};
use crate::value::{js_nanbox_pointer, JSValue, TAG_FALSE, TAG_TRUE, TAG_UNDEFINED};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Boolean,
    String,
}

struct OptionSpec {
    kind: OptionKind,
    multiple: bool,
    default_value: Option<f64>,
}

#[derive(Default)]
struct ParseSpecs {
    options: HashMap<String, OptionSpec>,
    short_to_long: HashMap<char, String>,
    order: Vec<String>,
}

#[derive(Clone)]
struct ArgValue {
    value: f64,
    text: Option<String>,
}

impl ArgValue {
    fn result_value(&self) -> f64 {
        match &self.text {
            Some(text) => string_value(text),
            None => self.value,
        }
    }
}

#[no_mangle]
pub extern "C" fn js_util_parse_args(config_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let config_handle = scope.root_nanbox_f64(config_value);

    let strict = get_bool_prop(config_handle.get_nanbox_f64(), b"strict").unwrap_or(true);
    let allow_positionals =
        get_bool_prop(config_handle.get_nanbox_f64(), b"allowPositionals").unwrap_or(!strict);
    let allow_negative =
        get_bool_prop(config_handle.get_nanbox_f64(), b"allowNegative").unwrap_or(false);
    let return_tokens = get_bool_prop(config_handle.get_nanbox_f64(), b"tokens").unwrap_or(false);

    let args_value = get_prop(config_handle.get_nanbox_f64(), b"args");
    let args = args_from_value(args_value);
    let options_value = get_prop(config_handle.get_nanbox_f64(), b"options");
    let specs = specs_from_value(options_value);

    let values = js_object_alloc(0, 0);
    let positionals = js_array_alloc(0);
    let tokens = js_array_alloc(0);
    let result = js_object_alloc(0, 0);

    let values_handle = scope.root_raw_mut_ptr(values);
    let positionals_handle = scope.root_raw_mut_ptr(positionals);
    let tokens_handle = scope.root_raw_mut_ptr(tokens);
    let result_handle = scope.root_raw_mut_ptr(result);

    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];

        let Some(arg_text) = arg.text.as_deref() else {
            if !allow_positionals {
                throw_parse_args_error(
                    "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL",
                    "Unexpected positional argument",
                );
            }
            push_positional(&positionals_handle, arg);
            if return_tokens {
                push_token(
                    &tokens_handle,
                    TokenParts {
                        kind: "positional",
                        name: None,
                        raw_name: None,
                        value: Some(arg.result_value()),
                        index: i,
                        inline_value: None,
                    },
                );
            }
            i += 1;
            continue;
        };

        if arg_text == "--" {
            if return_tokens {
                push_token(
                    &tokens_handle,
                    TokenParts {
                        kind: "option-terminator",
                        name: None,
                        raw_name: Some("--"),
                        value: None,
                        index: i,
                        inline_value: None,
                    },
                );
            }
            i += 1;
            while i < args.len() {
                push_positional(&positionals_handle, &args[i]);
                if return_tokens {
                    push_token(
                        &tokens_handle,
                        TokenParts {
                            kind: "positional",
                            name: None,
                            raw_name: None,
                            value: Some(args[i].result_value()),
                            index: i,
                            inline_value: None,
                        },
                    );
                }
                i += 1;
            }
            break;
        }

        if let Some(long) = arg_text.strip_prefix("--") {
            parse_long_option(
                &args,
                &mut i,
                long,
                &specs,
                allow_negative,
                strict,
                &values_handle,
                if return_tokens {
                    Some(&tokens_handle)
                } else {
                    None
                },
            );
            i += 1;
            continue;
        }

        if arg_text.starts_with('-') && arg_text.len() > 1 {
            parse_short_option(
                &args,
                &mut i,
                arg_text,
                &specs,
                strict,
                &values_handle,
                if return_tokens {
                    Some(&tokens_handle)
                } else {
                    None
                },
            );
            i += 1;
            continue;
        }

        if !allow_positionals {
            throw_parse_args_error(
                "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL",
                "Unexpected positional argument",
            );
        }
        push_positional(&positionals_handle, arg);
        if return_tokens {
            push_token(
                &tokens_handle,
                TokenParts {
                    kind: "positional",
                    name: None,
                    raw_name: None,
                    value: Some(arg.result_value()),
                    index: i,
                    inline_value: None,
                },
            );
        }
        i += 1;
    }

    apply_defaults(&values_handle, &specs);

    set_prop_on_obj(
        result_handle.get_raw_mut_ptr(),
        b"values",
        boxed_ptr(values_handle.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
    );
    set_prop_on_obj(
        result_handle.get_raw_mut_ptr(),
        b"positionals",
        boxed_ptr(positionals_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>() as *const u8),
    );
    if return_tokens {
        set_prop_on_obj(
            result_handle.get_raw_mut_ptr(),
            b"tokens",
            boxed_ptr(tokens_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>() as *const u8),
        );
    }

    boxed_ptr(result_handle.get_raw_mut_ptr::<ObjectHeader>() as *const u8)
}

fn parse_long_option(
    args: &[ArgValue],
    index: &mut usize,
    long: &str,
    specs: &ParseSpecs,
    allow_negative: bool,
    strict: bool,
    values: &crate::gc::RuntimeHandle<'_>,
    tokens: Option<&crate::gc::RuntimeHandle<'_>>,
) {
    if let Some(negative_name) = long.strip_prefix("no-") {
        if allow_negative {
            if let Some(spec) = specs.options.get(negative_name) {
                if spec.kind == OptionKind::Boolean {
                    let raw_negative = format!("--no-{negative_name}");
                    set_option_value(values, negative_name, bool_value(false), spec);
                    if let Some(tokens) = tokens {
                        push_token(
                            tokens,
                            TokenParts {
                                kind: "option",
                                name: Some(negative_name),
                                raw_name: Some(&raw_negative),
                                value: None,
                                index: *index,
                                inline_value: None,
                            },
                        );
                    }
                    return;
                }
            }
        }
        if strict {
            throw_unknown_option(&format!("--{long}"));
        }
    }

    let (name, inline_value) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (long, None),
    };
    let raw_name = format!("--{name}");
    let Some(spec) = specs.options.get(name) else {
        if strict {
            throw_unknown_option(&raw_name);
        }
        let value = match inline_value {
            Some(value) => string_value(value),
            None => bool_value(true),
        };
        set_value(values, name, value);
        if let Some(tokens) = tokens {
            push_token(
                tokens,
                TokenParts {
                    kind: "option",
                    name: Some(name),
                    raw_name: Some(&raw_name),
                    value: inline_value.map(string_value),
                    index: *index,
                    inline_value: inline_value.map(|_| true),
                },
            );
        }
        return;
    };

    match spec.kind {
        OptionKind::Boolean => {
            if inline_value.is_some() {
                throw_invalid_option_value(&raw_name);
            }
            set_option_value(values, name, bool_value(true), spec);
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(name),
                        raw_name: Some(&raw_name),
                        value: None,
                        index: *index,
                        inline_value: None,
                    },
                );
            }
        }
        OptionKind::String => {
            let (value, inline) = match inline_value {
                Some(value) => (string_value(value), Some(true)),
                None => {
                    if *index + 1 >= args.len() {
                        throw_invalid_option_value(&raw_name);
                    }
                    *index += 1;
                    if args[*index].text.is_none() {
                        throw_invalid_option_value(&raw_name);
                    }
                    (args[*index].result_value(), Some(false))
                }
            };
            set_option_value(values, name, value, spec);
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(name),
                        raw_name: Some(&raw_name),
                        value: Some(value),
                        index: *index - usize::from(!inline.unwrap_or(false)),
                        inline_value: inline,
                    },
                );
            }
        }
    }
}

fn parse_short_option(
    args: &[ArgValue],
    index: &mut usize,
    arg: &str,
    specs: &ParseSpecs,
    strict: bool,
    values: &crate::gc::RuntimeHandle<'_>,
    tokens: Option<&crate::gc::RuntimeHandle<'_>>,
) {
    let chars: Vec<(usize, char)> = arg[1..].char_indices().collect();
    let mut pos = 0usize;
    while pos < chars.len() {
        let (byte_offset, short) = chars[pos];
        let raw_short = format!("-{short}");
        let Some(long_name) = specs.short_to_long.get(&short) else {
            if strict {
                throw_unknown_option(&raw_short);
            }
            let unknown_name = short.to_string();
            set_value(values, &unknown_name, bool_value(true));
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(&unknown_name),
                        raw_name: Some(&raw_short),
                        value: None,
                        index: *index,
                        inline_value: None,
                    },
                );
            }
            pos += 1;
            continue;
        };
        let spec = specs
            .options
            .get(long_name)
            .expect("short option must resolve to an option spec");
        match spec.kind {
            OptionKind::Boolean => {
                set_option_value(values, long_name, bool_value(true), spec);
                if let Some(tokens) = tokens {
                    push_token(
                        tokens,
                        TokenParts {
                            kind: "option",
                            name: Some(long_name),
                            raw_name: Some(&raw_short),
                            value: None,
                            index: *index,
                            inline_value: None,
                        },
                    );
                }
                pos += 1;
            }
            OptionKind::String => {
                let rest_start = 1 + byte_offset + short.len_utf8();
                let rest = &arg[rest_start..];
                let (value, inline) = if rest.is_empty() {
                    if *index + 1 >= args.len() {
                        throw_invalid_option_value(&raw_short);
                    }
                    *index += 1;
                    if args[*index].text.is_none() {
                        throw_invalid_option_value(&raw_short);
                    }
                    (args[*index].result_value(), Some(false))
                } else {
                    (string_value(rest), Some(true))
                };
                set_option_value(values, long_name, value, spec);
                if let Some(tokens) = tokens {
                    push_token(
                        tokens,
                        TokenParts {
                            kind: "option",
                            name: Some(long_name),
                            raw_name: Some(&raw_short),
                            value: Some(value),
                            index: *index - usize::from(!inline.unwrap_or(false)),
                            inline_value: inline,
                        },
                    );
                }
                break;
            }
        }
    }
}

fn specs_from_value(options_value: f64) -> ParseSpecs {
    let mut specs = ParseSpecs::default();
    if is_nullish(options_value) {
        return specs;
    };
    let Some(options_obj) = object_ptr(options_value) else {
        throw_invalid_arg_type(
            "The \"options\" argument must be of type object",
            options_value,
        );
    };
    let keys = js_object_keys(options_obj);
    let key_count = js_array_length(keys) as usize;
    for i in 0..key_count {
        let key_value = js_array_get_f64(keys, i as u32);
        let Some(name) = string_from_value(key_value) else {
            continue;
        };
        let Some(key_ptr) = string_ptr_from_value(key_value) else {
            continue;
        };
        let desc_value = js_object_get_field_by_name_f64(options_obj, key_ptr);
        let Some(desc_obj) = object_ptr(desc_value) else {
            throw_invalid_arg_type(
                &format!("The \"options.{name}\" property must be of type object"),
                desc_value,
            );
        };
        let type_value = get_prop(desc_value, b"type");
        let kind = match string_from_value(type_value).as_deref() {
            Some("string") => OptionKind::String,
            Some("boolean") => OptionKind::Boolean,
            _ => throw_invalid_union_type(&format!("options.{name}.type"), type_value),
        };
        if object_has_own_property(desc_obj, b"short") {
            let short_value = get_prop(desc_value, b"short");
            let Some(short_string) = string_from_value(short_value) else {
                throw_invalid_arg_type(
                    &format!("The \"options.{name}.short\" property must be of type string"),
                    short_value,
                );
            };
            if short_string.chars().count() != 1 {
                throw_invalid_arg_value_single_char(
                    &format!("options.{name}.short"),
                    &short_string,
                );
            }
            let short = short_string
                .chars()
                .next()
                .expect("validated one-char short option");
            specs.short_to_long.insert(short, name.clone());
        }
        if object_has_own_property(desc_obj, b"multiple") {
            let multiple_value = get_prop(desc_value, b"multiple");
            if !JSValue::from_bits(multiple_value.to_bits()).is_bool() {
                throw_invalid_arg_type(
                    &format!("The \"options.{name}.multiple\" property must be of type boolean"),
                    multiple_value,
                );
            }
        }
        let multiple = get_bool_prop(desc_value, b"multiple").unwrap_or(false);
        let default_value = default_from_descriptor(&name, desc_value, kind, multiple);
        specs.order.push(name.clone());
        specs.options.insert(
            name,
            OptionSpec {
                kind,
                multiple,
                default_value,
            },
        );
    }
    specs
}

fn default_from_descriptor(
    name: &str,
    desc_value: f64,
    kind: OptionKind,
    multiple: bool,
) -> Option<f64> {
    if !object_has_own_property(object_ptr(desc_value)?, b"default") {
        return None;
    }
    let default_value = get_prop(desc_value, b"default");
    if JSValue::from_bits(default_value.to_bits()).is_undefined() {
        return None;
    }

    if multiple {
        let Some(arr) = array_ptr(default_value) else {
            throw_invalid_arg_type(
                &format!("The \"options.{name}.default\" property must be an instance of Array"),
                default_value,
            );
        };
        let len = js_array_length(arr) as usize;
        for i in 0..len {
            let value = js_array_get_f64(arr, i as u32);
            validate_default_element(name, kind, value);
        }
        return Some(default_value);
    }

    validate_default_element(name, kind, default_value);
    Some(default_value)
}

fn validate_default_element(name: &str, kind: OptionKind, value: f64) {
    match kind {
        OptionKind::String => {
            if string_ptr_from_value(value).is_none() {
                throw_invalid_arg_type(
                    &format!("The \"options.{name}.default\" property must be of type string"),
                    value,
                );
            }
        }
        OptionKind::Boolean => {
            if !JSValue::from_bits(value.to_bits()).is_bool() {
                throw_invalid_arg_type(
                    &format!("The \"options.{name}.default\" property must be of type boolean"),
                    value,
                );
            }
        }
    }
}

fn args_from_value(args_value: f64) -> Vec<ArgValue> {
    let jsvalue = JSValue::from_bits(args_value.to_bits());
    if jsvalue.is_undefined() {
        return process_argv_args();
    }
    if jsvalue.is_null() {
        return Vec::new();
    }
    let Some(args_ptr) = array_ptr(args_value) else {
        throw_invalid_arg_type(
            "The \"args\" argument must be an instance of Array",
            args_value,
        );
    };
    let len = js_array_length(args_ptr) as usize;
    let mut args = Vec::with_capacity(len);
    for i in 0..len {
        let value = js_array_get_f64(args_ptr, i as u32);
        args.push(arg_from_value(value));
    }
    args
}

fn process_argv_args() -> Vec<ArgValue> {
    let argv = crate::os::js_process_argv();
    if argv.is_null() {
        return Vec::new();
    }
    let len = js_array_length(argv) as usize;
    let mut args = Vec::with_capacity(len.saturating_sub(2));
    for i in 2..len {
        let value = js_array_get_f64(argv, i as u32);
        args.push(arg_from_value(value));
    }
    args
}

fn arg_from_value(value: f64) -> ArgValue {
    ArgValue {
        value,
        text: string_from_value(value),
    }
}

struct TokenParts<'a> {
    kind: &'a str,
    name: Option<&'a str>,
    raw_name: Option<&'a str>,
    value: Option<f64>,
    index: usize,
    inline_value: Option<bool>,
}

fn push_token(tokens: &crate::gc::RuntimeHandle<'_>, parts: TokenParts<'_>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let token = js_object_alloc(0, 0);
    let token_handle = scope.root_raw_mut_ptr(token);
    set_prop_on_obj(
        token_handle.get_raw_mut_ptr(),
        b"kind",
        string_value(parts.kind),
    );
    set_prop_on_obj(
        token_handle.get_raw_mut_ptr(),
        b"index",
        JSValue::number(parts.index as f64).as_f64_bits(),
    );
    if let Some(name) = parts.name {
        set_prop_on_obj(token_handle.get_raw_mut_ptr(), b"name", string_value(name));
    }
    if let Some(raw_name) = parts.raw_name {
        set_prop_on_obj(
            token_handle.get_raw_mut_ptr(),
            b"rawName",
            string_value(raw_name),
        );
    }
    if let Some(value) = parts.value {
        set_prop_on_obj(token_handle.get_raw_mut_ptr(), b"value", value);
    }
    if let Some(inline_value) = parts.inline_value {
        set_prop_on_obj(
            token_handle.get_raw_mut_ptr(),
            b"inlineValue",
            bool_value(inline_value),
        );
    }
    push_array_value(
        tokens,
        boxed_ptr(token_handle.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
    );
}

fn push_positional(positionals: &crate::gc::RuntimeHandle<'_>, value: &ArgValue) {
    push_array_value(positionals, value.result_value());
}

fn push_array_value(arr_handle: &crate::gc::RuntimeHandle<'_>, value: f64) {
    let arr = arr_handle.get_raw_mut_ptr();
    let arr = js_array_push_f64(arr, value);
    arr_handle.set_raw_mut_ptr(arr);
}

fn set_value(values: &crate::gc::RuntimeHandle<'_>, name: &str, value: f64) {
    set_prop_on_obj(values.get_raw_mut_ptr(), name.as_bytes(), value);
}

fn set_option_value(
    values: &crate::gc::RuntimeHandle<'_>,
    name: &str,
    value: f64,
    spec: &OptionSpec,
) {
    if !spec.multiple {
        set_value(values, name, value);
        return;
    }

    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let current = js_object_get_field_by_name_f64(values.get_raw_mut_ptr(), key);
    if let Some(arr) = array_ptr(current) {
        let pushed = js_array_push_f64(arr as *mut crate::array::ArrayHeader, value);
        set_prop_on_obj(
            values.get_raw_mut_ptr(),
            name.as_bytes(),
            boxed_ptr(pushed as *const u8),
        );
        return;
    }

    let arr = js_array_alloc(0);
    let arr = js_array_push_f64(arr, value);
    set_prop_on_obj(
        values.get_raw_mut_ptr(),
        name.as_bytes(),
        boxed_ptr(arr as *const u8),
    );
}

fn apply_defaults(values: &crate::gc::RuntimeHandle<'_>, specs: &ParseSpecs) {
    for name in &specs.order {
        let Some(spec) = specs.options.get(name) else {
            continue;
        };
        let Some(default_value) = spec.default_value else {
            continue;
        };
        if object_has_own_property(values.get_raw_mut_ptr(), name.as_bytes()) {
            continue;
        }
        set_value(values, name, default_value);
    }
}

fn get_prop(value: f64, name: &[u8]) -> f64 {
    let Some(obj) = object_ptr(value) else {
        return TAG_UNDEFINED_F64;
    };
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name_f64(obj, key)
}

fn get_bool_prop(value: f64, name: &[u8]) -> Option<bool> {
    match get_prop(value, name).to_bits() {
        TAG_TRUE => Some(true),
        TAG_FALSE => Some(false),
        _ => None,
    }
}

fn set_prop_on_obj(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn object_ptr(value: f64) -> Option<*const ObjectHeader> {
    let ptr = heap_ptr_with_gc_type(value, crate::gc::GC_TYPE_OBJECT)?;
    Some(ptr as *const ObjectHeader)
}

fn heap_ptr_with_gc_type(value: f64, expected_type: u8) -> Option<*const u8> {
    let jsvalue = JSValue::from_bits(value.to_bits());
    if !jsvalue.is_pointer() {
        return None;
    }
    let ptr = jsvalue.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type == expected_type {
        Some(ptr)
    } else {
        None
    }
}

fn array_ptr(value: f64) -> Option<*const crate::array::ArrayHeader> {
    let ptr = heap_ptr_with_gc_type(value, crate::gc::GC_TYPE_ARRAY)?;
    Some(ptr as *const crate::array::ArrayHeader)
}

fn is_nullish(value: f64) -> bool {
    let jsvalue = JSValue::from_bits(value.to_bits());
    jsvalue.is_undefined() || jsvalue.is_null()
}

fn object_has_own_property(obj: *const ObjectHeader, name: &[u8]) -> bool {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    unsafe { crate::object::own_key_present(obj as *mut ObjectHeader, key) }
}

fn throw_invalid_arg_type(prefix: &str, value: f64) -> ! {
    let message = format!(
        "{prefix}. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_union_type(property: &str, value: f64) -> ! {
    let message = format!(
        "The \"{property}\" property must be ('string|boolean'). Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_arg_value_single_char(property: &str, value: &str) -> ! {
    let message = format!(
        "The property '{property}' must be a single character. Received '{}'",
        value
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn string_ptr_from_value(value: f64) -> Option<*const StringHeader> {
    let ptr = js_string_materialize_to_heap(value);
    if ptr.is_null() {
        None
    } else {
        Some(ptr as *const StringHeader)
    }
}

fn string_from_value(value: f64) -> Option<String> {
    let ptr = string_ptr_from_value(value)?;
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn throw_unknown_option(raw_name: &str) -> ! {
    throw_parse_args_error(
        "ERR_PARSE_ARGS_UNKNOWN_OPTION",
        &format!("Unknown option '{raw_name}'"),
    )
}

fn throw_invalid_option_value(raw_name: &str) -> ! {
    throw_parse_args_error(
        "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
        &format!("Invalid option value for '{raw_name}'"),
    )
}

fn throw_parse_args_error(code: &'static str, message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

trait JSValueBits {
    fn as_f64_bits(self) -> f64;
}

impl JSValueBits for JSValue {
    fn as_f64_bits(self) -> f64 {
        f64::from_bits(self.bits())
    }
}
