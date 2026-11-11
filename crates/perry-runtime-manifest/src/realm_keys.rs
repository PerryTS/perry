//! Source-spelled keys used by runtime member lookup helpers. Declarations and
//! comments are not reads; Rust tokens preserve wrapped calls and raw literals.
use super::{rust_tokens, Token};
use std::collections::BTreeSet;

fn member_helper(name: &str) -> bool {
    name.starts_with("js_native_call_method")
        || name.starts_with("js_object_get_field_by_name")
        || name.starts_with("get_field_by_name_")
        || matches!(
            name,
            "js_native_call_method"
                | "call_method"
                | "call_method_value"
                | "call_method_no_args"
                | "get_field_by_name"
                | "js_object_get_field_by_name"
                | "runtime_state_key"
                | "canonical_key"
                | "call_method_for_primitive"
                | "call_function_method"
        )
        || name.ends_with("get_property")
        || name.ends_with("get_own_property")
        || name.ends_with("get_property_by_name")
}

pub(super) fn spelled(source: &str) -> Result<BTreeSet<String>, String> {
    let ts = rust_tokens(source)?;
    let mut keys = BTreeSet::new();
    let mut skip_until = 0;
    for (i, token) in ts.iter().enumerate() {
        if i < skip_until {
            continue;
        }
        let tail = &ts[i..];
        let test_attribute = tail.len() >= 4
            && tail[..4].iter().map(|t| t.text).eq(["#", "[", "test", "]"])
            || tail.len() >= 7
                && tail[..7]
                    .iter()
                    .map(|t| t.text)
                    .eq(["#", "[", "cfg", "(", "test", ")", "]"]);
        if test_attribute {
            let attr_len = if ts[i + 2].text == "cfg" { 7 } else { 4 };
            skip_until = test_item_end(&ts, i + attr_len);
            continue;
        }
        if !member_helper(token.text) || i > 0 && ts[i - 1].text == "fn" {
            continue;
        }
        let open = i + 1 + usize::from(ts.get(i + 1).is_some_and(|t| t.text == "!"));
        if !ts.get(open).is_some_and(|t| t.text == "(") {
            continue;
        }
        let end = super::closing_paren(&ts, open)?;
        strings(&ts[open + 1..end], &mut keys);
    }
    Ok(keys)
}

fn test_item_end(ts: &[Token<'_>], start: usize) -> usize {
    let mut braces = 0;
    let mut body = false;
    for (i, token) in ts.iter().enumerate().skip(start) {
        match token.text {
            "{" => {
                body = true;
                braces += 1;
            }
            "}" if body => {
                braces -= 1;
                if braces == 0 {
                    return i + 1;
                }
            }
            ";" if !body => return i + 1,
            _ => {}
        }
    }
    ts.len()
}

fn strings(ts: &[Token<'_>], keys: &mut BTreeSet<String>) {
    for token in ts {
        if let Some(key) = &token.string {
            keys.insert(key.clone());
        }
    }
}

pub(super) fn attach(rows: &mut [crate::realm_json::Value], keys: &BTreeSet<String>) {
    use crate::realm_json::Value;
    for row in rows {
        let Some(properties) = row.get("properties").and_then(Value::array) else {
            continue;
        };
        let flags: Vec<_> = properties
            .iter()
            .filter_map(|p| p.get("key").and_then(Value::string))
            .filter(|key| keys.contains(*key))
            .map(|key| Value::String(format!("runtime_key:{key}")))
            .collect();
        if flags.is_empty() {
            continue;
        }
        let Value::Object(map) = row else { continue };
        let Value::Array(existing) = map
            .entry("flags".into())
            .or_insert_with(|| Value::Array(Vec::new()))
        else {
            unreachable!()
        };
        existing.extend(flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_tokens_and_wrapped_literal_calls() {
        let keys = spelled(
            r##"
            // call_method(v, "comment");
            const DOC: &str = "call_method(v, \"string\")";
            fn call_method(v: V, k: &str) {}
            call_method(value,
                r#"next"#);
            crate::runtime_state_key!(b"then").get(v);
            js_object_get_field_by_name_f64(value, named_key(b"done"));
            js_native_call_method_nullsafe(value, "return", args);
            realm_decl! { realm { Example, members = { unused; }; } }
            #[cfg(test)] mod tests { fn test() { call_method(v, "test_only"); } }
        "##,
        )
        .unwrap();
        assert_eq!(
            keys,
            BTreeSet::from(["next".into(), "then".into(), "done".into(), "return".into()])
        );
    }
}
