//! Constructor-family encodings come from the runtime's existing declaration
//! macro, including its enum order. The realm source never repeats those words.
use super::{info, tokens, Token};
use crate::realm_json::Value;
use std::collections::BTreeMap;

pub(super) fn read(source: &str) -> Result<BTreeMap<String, Value>, String> {
    let ts = tokens(source)?;
    let start = ts
        .windows(3)
        .position(|w| w[0].text == "builtin_constructors" && w[1].text == "!" && w[2].text == "{")
        .ok_or("missing builtin constructor declarations")?
        + 3;
    let mut depth = 1;
    let mut end = start;
    while depth != 0 {
        let token = ts.get(end).ok_or("unterminated builtin constructors")?;
        match token.text {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ => {}
        }
        if depth != 0 {
            end += 1;
        }
    }
    let mut out = BTreeMap::new();
    let rows = info::split(&ts[start..end], ",");
    for (index, row) in rows.iter().enumerate() {
        let clauses = info::split(row, ";");
        let head = clauses[0];
        if head.len() < 4 || head[1].text != "=" || head[2].text != ">" {
            return Err("malformed constructor family".into());
        }
        let name = head[0].text.to_owned();
        let mut facts = BTreeMap::from([
            ("identity".into(), Value::Number((index + 1).to_string())),
            ("prototype_callable".into(), Value::Bool(false)),
        ]);
        for clause in &clauses[1..] {
            let field = match clause[0].text {
                "class" => "prototype_class",
                "serial" => "prototype_serial",
                "sealed" => "sealed_serial",
                "named_serial" => "named_serial",
                "instance" => "instance_identity",
                "prototype_callable" => "prototype_callable",
                "length" => continue,
                other => return Err(format!("unknown constructor family field {other}")),
            };
            let value = if field == "prototype_callable" {
                match info::text(&clause[1..]).as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => return Err("constructor callability must be a boolean".into()),
                }
            } else {
                let mut value = expression(&clause[1..])?;
                // realm-v1 stores these as source expressions. Preserve the
                // original producer's trailing-comment spelling until that
                // wire representation changes, without duplicating comments.
                if field == "instance_identity"
                    && clause.last().unwrap().end == row.last().unwrap().end
                {
                    let next = rows
                        .get(index + 1)
                        .map_or(ts[end].start, |row| row[0].start);
                    let tail = source[row.last().unwrap().end..next].trim_end();
                    if tail.contains("//") {
                        value.push_str(tail.split(';').next().unwrap());
                    }
                }
                Value::String(value)
            };
            facts.insert(field.into(), value);
        }
        if out.insert(name.clone(), Value::Object(facts)).is_some() {
            return Err(format!("duplicate constructor family {name}"));
        }
    }
    Ok(out)
}

fn expression(ts: &[Token<'_>]) -> Result<String, String> {
    let mut value = info::text(ts);
    // Hook slots re-export the ABI constants; the wire identity names the
    // public defining surface, independent of which runtime importer uses it.
    let hook_prefix = "crate::typedarray_hook_slots::";
    if let Some(at) = value.find(hook_prefix) {
        let name = value[at + hook_prefix.len()..]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap();
        let exports = tokens(include_str!(
            "../../perry-runtime/src/typedarray_hook_slots.rs"
        ))?;
        let start = exports
            .windows(9)
            .position(|w| info::text(w) == "pubusecrate::typedarray::{")
            .ok_or("missing typed-array ABI re-export")?
            + 9;
        let end = start
            + exports[start..]
                .iter()
                .position(|t| t.text == "}")
                .ok_or("unterminated typed-array ABI exports")?;
        if !exports[start..end].iter().any(|t| t.text == name) {
            return Err(format!("unresolved typed-array ABI re-export {name}"));
        }
        value = value.replace(hook_prefix, "crate::typedarray::");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v1_family_expressions_preserve_comment_spelling_and_resolve_reexports() {
        let source = "builtin_constructors! { A => info; instance Class(1),\n    // compatible spelling; more commentary\n    B => info, }";
        let families = read(source).unwrap();
        assert_eq!(
            families["A"]
                .get("instance_identity")
                .and_then(Value::string),
            Some("Class(1),\n    // compatible spelling")
        );
        assert_eq!(
            expression(
                &tokens("Class(crate::typedarray_hook_slots::CLASS_ID_UINT8_ARRAY)").unwrap()
            )
            .unwrap(),
            "Class(crate::typedarray::CLASS_ID_UINT8_ARRAY)"
        );
        assert!(
            expression(&tokens("Class(crate::typedarray_hook_slots::UNKNOWN)").unwrap()).is_err()
        );
    }

    #[test]
    fn enum_words_and_family_fields_follow_the_owning_macro() {
        let source = "builtin_constructors! { Array => info; instance BuiltinInstanceIdentity::Class(0xFFFF0024); named_serial crate::shape::ARRAY, Function => info; prototype_callable true, }";
        let families = read(source).unwrap();
        assert_eq!(
            families["Array"].get("identity"),
            Some(&Value::Number("1".into()))
        );
        assert_eq!(
            families["Function"].get("identity"),
            Some(&Value::Number("2".into()))
        );
        assert_eq!(
            families["Function"].get("prototype_callable"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            families["Array"]
                .get("named_serial")
                .and_then(Value::string),
            Some("crate::shape::ARRAY")
        );
    }
}
