//! Focused `util.parseArgs(config)` support for Node-compatible CLI option
//! parsing.

use std::collections::HashMap;

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_keys, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, js_string_materialize_to_heap, StringHeader};
use crate::value::{
    js_nanbox_pointer, JSValue, POINTER_MASK, POINTER_TAG, TAG_FALSE, TAG_MASK, TAG_TRUE,
    TAG_UNDEFINED,
};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Boolean,
    String,
}

struct OptionSpec {
    kind: OptionKind,
}

#[derive(Default)]
struct ParseSpecs {
    options: HashMap<String, OptionSpec>,
    short_to_long: HashMap<char, String>,
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

        if arg == "--" {
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
                            value: Some(&args[i]),
                            index: i,
                            inline_value: None,
                        },
                    );
                }
                i += 1;
            }
            break;
        }

        if let Some(long) = arg.strip_prefix("--") {
            parse_long_option(
                &args,
                &mut i,
                long,
                arg,
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

        if arg.starts_with('-') && arg.len() > 1 {
            parse_short_option(
                &args,
                &mut i,
                arg,
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
                    value: Some(arg),
                    index: i,
                    inline_value: None,
                },
            );
        }
        i += 1;
    }

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
    args: &[String],
    index: &mut usize,
    long: &str,
    raw_name: &str,
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
                    set_value(values, negative_name, bool_value(false));
                    if let Some(tokens) = tokens {
                        push_token(
                            tokens,
                            TokenParts {
                                kind: "option",
                                name: Some(negative_name),
                                raw_name: Some(raw_name),
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
            throw_unknown_option(raw_name);
        }
    }

    let (name, inline_value) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (long, None),
    };
    let Some(spec) = specs.options.get(name) else {
        if strict {
            throw_unknown_option(raw_name);
        }
        return;
    };

    match spec.kind {
        OptionKind::Boolean => {
            if inline_value.is_some() {
                throw_invalid_option_value(raw_name);
            }
            set_value(values, name, bool_value(true));
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(name),
                        raw_name: Some(raw_name),
                        value: None,
                        index: *index,
                        inline_value: None,
                    },
                );
            }
        }
        OptionKind::String => {
            let (value, inline) = match inline_value {
                Some(value) => (value.to_string(), Some(true)),
                None => {
                    if *index + 1 >= args.len() {
                        throw_invalid_option_value(raw_name);
                    }
                    *index += 1;
                    (args[*index].clone(), Some(false))
                }
            };
            set_value(values, name, string_value(&value));
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(name),
                        raw_name: Some(raw_name),
                        value: Some(&value),
                        index: *index - usize::from(!inline.unwrap_or(false)),
                        inline_value: inline,
                    },
                );
            }
        }
    }
}

fn parse_short_option(
    args: &[String],
    index: &mut usize,
    arg: &str,
    specs: &ParseSpecs,
    strict: bool,
    values: &crate::gc::RuntimeHandle<'_>,
    tokens: Option<&crate::gc::RuntimeHandle<'_>>,
) {
    let mut chars = arg[1..].chars();
    let Some(short) = chars.next() else {
        return;
    };
    let Some(long_name) = specs.short_to_long.get(&short) else {
        if strict {
            throw_unknown_option(arg);
        }
        return;
    };
    let spec = specs
        .options
        .get(long_name)
        .expect("short option must resolve to an option spec");
    let raw_short = format!("-{short}");
    match spec.kind {
        OptionKind::Boolean => {
            set_value(values, long_name, bool_value(true));
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
        }
        OptionKind::String => {
            let rest = chars.as_str();
            let (value, inline) = if rest.is_empty() {
                if *index + 1 >= args.len() {
                    throw_invalid_option_value(arg);
                }
                *index += 1;
                (args[*index].clone(), Some(false))
            } else {
                (rest.to_string(), Some(true))
            };
            set_value(values, long_name, string_value(&value));
            if let Some(tokens) = tokens {
                push_token(
                    tokens,
                    TokenParts {
                        kind: "option",
                        name: Some(long_name),
                        raw_name: Some(&raw_short),
                        value: Some(&value),
                        index: *index - usize::from(!inline.unwrap_or(false)),
                        inline_value: inline,
                    },
                );
            }
        }
    }
}

fn specs_from_value(options_value: f64) -> ParseSpecs {
    let mut specs = ParseSpecs::default();
    let Some(options_obj) = object_ptr(options_value) else {
        return specs;
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
        let kind = match string_from_value(get_prop(desc_value, b"type")).as_deref() {
            Some("string") => OptionKind::String,
            _ => OptionKind::Boolean,
        };
        let short =
            string_from_value(get_prop(desc_value, b"short")).and_then(|s| s.chars().next());
        if let Some(short) = short {
            specs.short_to_long.insert(short, name.clone());
        }
        specs.options.insert(name, OptionSpec { kind });
    }
    specs
}

fn args_from_value(args_value: f64) -> Vec<String> {
    let Some(args_ptr) = array_ptr(args_value) else {
        return Vec::new();
    };
    let len = js_array_length(args_ptr) as usize;
    let mut args = Vec::with_capacity(len);
    for i in 0..len {
        let value = js_array_get_f64(args_ptr, i as u32);
        args.push(string_from_value(value).unwrap_or_default());
    }
    args
}

struct TokenParts<'a> {
    kind: &'a str,
    name: Option<&'a str>,
    raw_name: Option<&'a str>,
    value: Option<&'a str>,
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
        set_prop_on_obj(
            token_handle.get_raw_mut_ptr(),
            b"value",
            string_value(value),
        );
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

fn push_positional(positionals: &crate::gc::RuntimeHandle<'_>, value: &str) {
    push_array_value(positionals, string_value(value));
}

fn push_array_value(arr_handle: &crate::gc::RuntimeHandle<'_>, value: f64) {
    let arr = arr_handle.get_raw_mut_ptr();
    let arr = js_array_push_f64(arr, value);
    arr_handle.set_raw_mut_ptr(arr);
}

fn set_value(values: &crate::gc::RuntimeHandle<'_>, name: &str, value: f64) {
    set_prop_on_obj(values.get_raw_mut_ptr(), name.as_bytes(), value);
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
    let bits = value.to_bits();
    if (bits & TAG_MASK) == POINTER_TAG {
        Some((bits & POINTER_MASK) as *const ObjectHeader)
    } else {
        None
    }
}

fn array_ptr(value: f64) -> Option<*const crate::array::ArrayHeader> {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == POINTER_TAG {
        Some((bits & POINTER_MASK) as *const crate::array::ArrayHeader)
    } else {
        None
    }
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
