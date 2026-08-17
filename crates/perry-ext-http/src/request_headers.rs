//! Client-side `options.headers` normalization.
//!
//! Node accepts both an object and a raw `[[name, value], ...]` array. The raw
//! form preserves duplicate fields and deliberately suppresses implicit
//! `Authorization`/`Host` generation. Perry's transport uses a map, so duplicate
//! values are combined with the separators Node exposes through
//! `IncomingMessage.headers` (`; ` for cookies, `, ` otherwise).

use std::collections::HashMap;

use base64::{engine::general_purpose, Engine as _};

fn header_value_string(name: &str, value: &serde_json::Value) -> String {
    if let Some(values) = value.as_array() {
        let separator = if name.eq_ignore_ascii_case("cookie") {
            "; "
        } else {
            ", "
        };
        return values
            .iter()
            .map(|value| header_value_string(name, value))
            .collect::<Vec<_>>()
            .join(separator);
    }

    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn append_header(out: &mut HashMap<String, String>, name: &str, value: &serde_json::Value) {
    let value = header_value_string(name, value);
    if let Some(existing) = out
        .iter_mut()
        .find_map(|(key, current)| key.eq_ignore_ascii_case(name).then_some(current))
    {
        existing.push_str(if name.eq_ignore_ascii_case("cookie") {
            "; "
        } else {
            ", "
        });
        existing.push_str(&value);
    } else {
        out.insert(name.to_string(), value);
    }
}

pub(crate) fn headers_from_options(opts: &serde_json::Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let headers = opts.get("headers");
    let headers_are_array = headers.is_some_and(serde_json::Value::is_array);

    match headers {
        Some(serde_json::Value::Object(headers)) => {
            for (name, value) in headers {
                append_header(&mut out, name, value);
            }
        }
        Some(serde_json::Value::Array(headers)) => {
            for pair in headers {
                let Some(pair) = pair.as_array() else {
                    continue;
                };
                let (Some(name), Some(value)) =
                    (pair.first().and_then(|v| v.as_str()), pair.get(1))
                else {
                    continue;
                };
                append_header(&mut out, name, value);
            }
        }
        _ => {}
    }

    let has_authorization = out
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization"));
    if !headers_are_array && !has_authorization {
        if let Some(auth) = opts.get("auth").and_then(serde_json::Value::as_str) {
            let encoded = general_purpose::STANDARD.encode(auth.as_bytes());
            out.insert("Authorization".to_string(), format!("Basic {encoded}"));
        }
    }
    out
}
