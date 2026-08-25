use crate::codec;
use crate::options::{ArrayFormat, StringifyOptions};
use crate::runtime;
use perry_ffi::{
    js_array_get, js_array_length, throw_with_code, value_byte_slice, ErrorKind,
    TransientRootScope, TransientRootedNanbox,
};
use std::cmp::Ordering;

pub(crate) fn stringify(value: f64, options: f64) -> String {
    let scope = TransientRootScope::enter();
    let options = StringifyOptions::from_js(&scope, options);
    let mut root = scope.root_nanbox(value);

    if let Some(filter) = &options.filter {
        root = apply_filter(&scope, filter, "", root.get());
    }

    let root_value = runtime::from_f64(root.get());
    if !root_value.is_pointer() || root_value.is_null() || runtime::is_closure(root_value) {
        return String::new();
    }

    let mut keys = options
        .filter_keys
        .clone()
        .unwrap_or_else(|| own_keys(&scope, &root));
    sort_keys(&scope, &options, &mut keys);

    let mut values = Vec::new();
    let mut ancestors = vec![root];
    for key in keys {
        let value = runtime::field_by_name(&scope, &root, &key);
        if options.skip_nulls && value.is_null() {
            continue;
        }
        values.extend(stringify_value(
            &scope,
            &options,
            runtime::as_f64(value),
            key,
            &mut ancestors,
        ));
    }

    let joined = values.join(&options.delimiter);
    if joined.is_empty() {
        return joined;
    }

    let mut prefix = String::new();
    if options.add_query_prefix {
        prefix.push('?');
    }
    if options.charset_sentinel {
        match options.charset {
            codec::Charset::Utf8 => prefix.push_str("utf8=%E2%9C%93"),
            codec::Charset::Latin1 => prefix.push_str("utf8=%26%2310003%3B"),
        }
        prefix.push_str(&options.delimiter);
    }
    prefix + joined.as_str()
}

fn stringify_value(
    scope: &TransientRootScope,
    options: &StringifyOptions,
    raw: f64,
    mut prefix: String,
    ancestors: &mut Vec<TransientRootedNanbox>,
) -> Vec<String> {
    let mut value = scope.root_nanbox(raw);

    if let Some(filter) = &options.filter {
        value = apply_filter(scope, filter, &prefix, value.get());
    } else if runtime::is_date(value.get()) {
        value = if let Some(callback) = &options.serialize_date {
            scope.root_nanbox(runtime::call1(scope, callback, value.get()))
        } else {
            let iso = runtime::date_iso(scope, value.get());
            scope.root_nanbox(runtime::alloc_string_value(&iso))
        };
    }

    let js = runtime::from_f64(value.get());
    if js.is_null() {
        if options.strict_null_handling {
            return vec![encode_key(scope, options, &prefix)];
        }
        value = scope.root_nanbox(runtime::alloc_string_value(""));
    }

    let js = runtime::from_f64(value.get());
    if js.is_undefined() || runtime::is_closure(js) {
        return Vec::new();
    }

    if let Some(bytes) = value_byte_slice(js) {
        let text = String::from_utf8_lossy(bytes).into_owned();
        return vec![format!(
            "{}={}",
            encode_key(scope, options, &prefix),
            encode_text(scope, options, &text, false)
        )];
    }

    if !js.is_pointer() {
        return vec![format!(
            "{}={}",
            encode_key(scope, options, &prefix),
            encode_value(scope, options, value.get())
        )];
    }

    let is_array = runtime::is_array(value.get());
    if is_array && options.array_format == ArrayFormat::Comma {
        return stringify_comma_array(scope, options, &value, prefix);
    }

    if ancestors
        .iter()
        .any(|ancestor| same_heap_value(ancestor.get(), value.get()))
    {
        throw_with_code("Cyclic object value", "", ErrorKind::RangeError);
    }
    ancestors.push(value);

    let mut keys = options
        .filter_keys
        .clone()
        .unwrap_or_else(|| own_keys(scope, &value));
    sort_keys(scope, options, &mut keys);

    if options.encode_dot_in_keys {
        prefix = prefix.replace('.', "%2E");
    }
    let adjusted_prefix = if is_array
        && options.array_format == ArrayFormat::Comma
        && options.comma_round_trip
        && keys.len() == 1
    {
        format!("{prefix}[]")
    } else {
        prefix
    };

    if options.allow_empty_arrays && is_array && keys.is_empty() {
        ancestors.pop();
        return vec![format!("{adjusted_prefix}[]")];
    }

    let mut values = Vec::new();
    for key in keys {
        let child = runtime::field_by_name(scope, &value, &key);
        if options.skip_nulls && child.is_null() {
            continue;
        }
        let key = if options.allow_dots && options.encode_dot_in_keys {
            key.replace('.', "%2E")
        } else {
            key
        };
        let child_prefix = if is_array {
            match options.array_format {
                ArrayFormat::Brackets => format!("{adjusted_prefix}[]"),
                ArrayFormat::Indices => format!("{adjusted_prefix}[{key}]"),
                ArrayFormat::Repeat => adjusted_prefix.clone(),
                ArrayFormat::Comma => unreachable!(),
            }
        } else if options.allow_dots {
            format!("{adjusted_prefix}.{key}")
        } else {
            format!("{adjusted_prefix}[{key}]")
        };
        values.extend(stringify_value(
            scope,
            options,
            runtime::as_f64(child),
            child_prefix,
            ancestors,
        ));
    }
    ancestors.pop();
    values
}

fn stringify_comma_array(
    scope: &TransientRootScope,
    options: &StringifyOptions,
    value: &TransientRootedNanbox,
    mut prefix: String,
) -> Vec<String> {
    let array = runtime::from_f64(value.get()).as_pointer();
    let length = unsafe { js_array_length(array) };
    if length == 0 {
        return if options.allow_empty_arrays {
            vec![format!("{prefix}[]")]
        } else {
            Vec::new()
        };
    }

    if options.comma_round_trip && length == 1 {
        prefix.push_str("[]");
    }
    let mut parts = Vec::with_capacity(length as usize);
    for index in 0..length {
        let array = runtime::from_f64(value.get()).as_pointer();
        let mut item = unsafe { js_array_get(array, index) };
        if runtime::is_date(runtime::as_f64(item)) {
            item = if let Some(callback) = &options.serialize_date {
                runtime::from_f64(runtime::call1(scope, callback, runtime::as_f64(item)))
            } else {
                let iso = runtime::date_iso(scope, runtime::as_f64(item));
                runtime::from_f64(runtime::alloc_string_value(&iso))
            };
        }
        if item.is_null() || item.is_undefined() {
            parts.push(String::new());
        } else {
            let text = runtime::owned_string(scope, runtime::as_f64(item));
            parts.push(if options.encode_values_only && options.encode {
                encode_text(scope, options, &text, false)
            } else {
                text
            });
        }
    }
    let joined = parts.join(",");
    if joined.is_empty() && options.strict_null_handling {
        vec![encode_key(scope, options, &prefix)]
    } else {
        let encoded_value = if options.encode_values_only && options.encode {
            codec::format_encoded(joined, options.format)
        } else {
            encode_text(scope, options, &joined, false)
        };
        vec![format!(
            "{}={}",
            encode_key(scope, options, &prefix),
            encoded_value
        )]
    }
}

fn own_keys(scope: &TransientRootScope, value: &TransientRootedNanbox) -> Vec<String> {
    let keys = runtime::object_keys(scope, value);
    let array = runtime::from_f64(keys.get()).as_pointer();
    let length = unsafe { js_array_length(array) };
    let mut result = Vec::with_capacity(length as usize);
    for index in 0..length {
        let array = runtime::from_f64(keys.get()).as_pointer();
        let key = unsafe { js_array_get(array, index) };
        result.push(runtime::owned_string(scope, runtime::as_f64(key)));
    }
    result
}

fn sort_keys(scope: &TransientRootScope, options: &StringifyOptions, keys: &mut [String]) {
    let Some(callback) = &options.sort else {
        return;
    };
    keys.sort_by(|left, right| {
        let left = scope.root_nanbox(runtime::alloc_string_value(left));
        let right = scope.root_nanbox(runtime::alloc_string_value(right));
        let result = runtime::from_f64(runtime::call2(scope, callback, left.get(), right.get()));
        let number = result.to_number();
        if number < 0.0 {
            Ordering::Less
        } else if number > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
}

fn apply_filter(
    scope: &TransientRootScope,
    callback: &TransientRootedNanbox,
    prefix: &str,
    value: f64,
) -> TransientRootedNanbox {
    let value = scope.root_nanbox(value);
    let prefix = scope.root_nanbox(runtime::alloc_string_value(prefix));
    scope.root_nanbox(runtime::call2(scope, callback, prefix.get(), value.get()))
}

fn encode_key(scope: &TransientRootScope, options: &StringifyOptions, key: &str) -> String {
    if options.encode_values_only {
        codec::format_encoded(key.to_owned(), options.format)
    } else {
        encode_text(scope, options, key, true)
    }
}

fn encode_value(scope: &TransientRootScope, options: &StringifyOptions, value: f64) -> String {
    if !options.encode {
        return codec::format_encoded(runtime::owned_string(scope, value), options.format);
    }
    if let Some(callback) = &options.encoder {
        let encoded = runtime::call1(scope, callback, value);
        return codec::format_encoded(runtime::owned_string(scope, encoded), options.format);
    }
    let text = runtime::owned_string(scope, value);
    codec::encode(&text, options.charset, options.format)
}

fn encode_text(
    scope: &TransientRootScope,
    options: &StringifyOptions,
    text: &str,
    _is_key: bool,
) -> String {
    if !options.encode {
        return codec::format_encoded(text.to_owned(), options.format);
    }
    if let Some(callback) = &options.encoder {
        let value = runtime::alloc_string_value(text);
        let encoded = runtime::call1(scope, callback, value);
        return codec::format_encoded(runtime::owned_string(scope, encoded), options.format);
    }
    codec::encode(text, options.charset, options.format)
}

fn same_heap_value(left: f64, right: f64) -> bool {
    let left = runtime::from_f64(left);
    let right = runtime::from_f64(right);
    left.is_pointer() && right.is_pointer() && left.as_pointer::<u8>() == right.as_pointer::<u8>()
}
