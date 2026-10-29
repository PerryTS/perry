//! Fixed record births inherit their layout and key order from runtime_record!.
use super::{birth, info, tokens, Token};
use crate::realm_json::Value;
use std::collections::BTreeMap;

pub(super) fn layouts(source: &str, name: &str) -> Result<Vec<Vec<String>>, String> {
    let ts = tokens(source)?;
    let mut atoms = BTreeMap::new();
    for i in 0..ts.len().saturating_sub(5) {
        if ts[i].text == "runtime_atom" && ts[i + 1].text == "!" && ts[i + 2].text == "(" {
            let name = i + 3 + usize::from(ts[i + 3].text == "reuse");
            if let Some(value) = ts.get(name + 2).and_then(|t| t.string.as_ref()) {
                atoms.insert(ts[name].text, value.clone());
            }
        }
    }
    let mut result = Vec::new();
    for i in 0..ts.len().saturating_sub(4) {
        if ts[i].text != "runtime_layout" || ts[i + 1].text != "!" || ts[i + 3].text != name {
            continue;
        }
        let end = super::closing_paren(&ts, i + 2)?;
        let args = info::split(&ts[i + 3..end], ",");
        // An optional sixth field names the owning realm cells (`realm = [..]`).
        let owned = args.len() == 6 && info::text(args[5]).starts_with("realm=");
        if !(args.len() == 5 || owned)
            || info::text(args[3]) != "crate::object::layout_desc::LAYOUT_PROTO_OBJECT"
        {
            return Err("record birth requires its ordinary object layout".into());
        }
        let keys = args[4];
        if keys.first().is_none_or(|t| t.text != "[") || keys.last().is_none_or(|t| t.text != "]") {
            return Err("record layout needs an atom list".into());
        }
        result.push(
            info::split(&keys[1..keys.len() - 1], ",")
                .into_iter()
                .map(|row| {
                    if row.len() != 1
                        && !(row.len() >= 4
                            && row[1].text == "="
                            && row[2].text == ">"
                            && info::text(&row[3..]).parse::<u8>().is_ok())
                    {
                        return Err("invalid record atom".into());
                    }
                    atoms
                        .get(row[0].text)
                        .cloned()
                        .ok_or_else(|| format!("missing record atom {}", row[0].text))
                })
                .collect::<Result<Vec<_>, String>>()?,
        );
    }
    if result.is_empty() {
        return Err(format!("missing runtime record layout {name}"));
    }
    Ok(result)
}

pub(super) fn birth(expression: &[Token<'_>], entry: &[Token<'_>]) -> Result<Value, String> {
    let args = info::split(expression, ",");
    if args.len() != 2 || args.iter().any(|arg| arg.len() != 1) {
        return Err("record birth needs the existing builder and layout identifiers".into());
    }
    let candidates = layouts(
        include_str!("../../perry-runtime/src/object/runtime_layouts/records.rs"),
        args[1][0].text,
    )?;
    let mut declaration = birth::birth(&tokens("js_object_alloc(0,0)")?, entry)?;
    let keys = declaration
        .get("own_key_order")
        .and_then(Value::array)
        .ok_or("record missing keys")?;
    let keys = keys
        .iter()
        .map(|key| key.string().unwrap().to_owned())
        .collect::<Vec<_>>();
    if !candidates.contains(&keys) {
        return Err(format!(
            "record {} members disagree with its owning layout",
            args[1][0].text
        ));
    }
    // perry_alloc_layout owns the ordinary-layout allocation flags. No
    // serialization ID, GC word or slot count is repeated in realm source.
    let Value::Object(map) = &mut declaration else {
        unreachable!()
    };
    let Value::Object(header) = map.get_mut("header").unwrap() else {
        unreachable!()
    };
    let flags = info::constants(include_str!("../../perry-abi/src/lib.rs"))?;
    let current = header["reserved"]
        .encode()
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    header.insert(
        "reserved".into(),
        Value::Number((current | flags["OBJ_FLAG_PLAIN_ORDINARY"]).to_string()),
    );
    Ok(declaration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_derive_order_from_atoms_and_fail_on_missing_layouts() {
        let source = r#"runtime_atom!(A,"first","");runtime_atom!(B,"second","");runtime_layout!(REC,9,0,crate::object::layout_desc::LAYOUT_PROTO_OBJECT,[B => 26,A]);"#;
        assert_eq!(
            layouts(source, "REC").unwrap(),
            vec![vec!["second", "first"]]
        );
        assert!(layouts(source, "MISSING").is_err());
        assert!(layouts(&source.replace("[B => 26,A]", "[B,UNKNOWN]"), "REC").is_err());
    }
}
