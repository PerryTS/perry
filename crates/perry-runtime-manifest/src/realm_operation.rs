//! Operations describe calls and writes at their allocation owner, with no cell.
use super::{facts, info, Token};
use crate::realm_json::Value;
use std::collections::BTreeMap;

pub(super) fn parse(ts: &[Token<'_>]) -> Result<Value, String> {
    let clauses = info::split(ts, ",");
    let [_, symbol] = clauses[0] else {
        return Err("operation needs a symbol".into());
    };
    let mut map = BTreeMap::from([
        (
            "name".into(),
            Value::String(format!("%Exec:{}%", symbol.text)),
        ),
        ("kind".into(), Value::String("namespace".into())),
        ("operation".into(), Value::String(symbol.text.into())),
    ]);
    for clause in &clauses[1..] {
        if clause.len() < 4
            || clause[1].text != "="
            || clause[2].text != "["
            || clause.last().unwrap().text != "]"
        {
            return Err("operation facts need lists".into());
        }
        let key = clause[0].text;
        let mut values = Vec::new();
        for value in info::split(&clause[3..clause.len() - 1], ",") {
            values.push(match key {
                "births" | "calls" | "demands" => {
                    let path = facts::path(value)?;
                    if key == "demands" && !(path.starts_with("%Layout:") && path.ends_with('%')) {
                        return Err("an operation demand must name a layout descriptor".into());
                    }
                    Value::String(path)
                }
                "writes" => {
                    if value.first().is_none_or(|t| t.text != "(")
                        || value.last().is_none_or(|t| t.text != ")")
                    {
                        return Err("write needs (holder,key,value)".into());
                    }
                    let parts = info::split(&value[1..value.len() - 1], ",");
                    if parts.len() != 3 {
                        return Err("write needs three paths".into());
                    }
                    Value::Object(
                        ["holder", "key", "value"]
                            .into_iter()
                            .zip(parts)
                            .map(|(k, v)| Ok((k.into(), Value::String(facts::path(v)?))))
                            .collect::<Result<_, String>>()?,
                    )
                }
                _ => return Err(format!("unknown operation fact {key}")),
            });
        }
        if map.insert(key.into(), Value::Array(values)).is_some() {
            return Err(format!("duplicate operation fact {key}"));
        }
    }
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_demand_has_no_heap_birth_or_holder() {
        let ts =
            super::super::tokens(r#"operation read, demands=["%Layout:GLOBAL_OBJECT%"]"#).unwrap();
        let facts = parse(&ts).unwrap();
        assert!(facts.get("births").is_none());
        assert!(facts.get("header").is_none());
        assert_eq!(
            facts.get("demands").unwrap().array().unwrap()[0].string(),
            Some("%Layout:GLOBAL_OBJECT%")
        );
        let invalid = super::super::tokens(r#"operation read, demands=["%Global%"]"#).unwrap();
        assert!(parse(&invalid).unwrap_err().contains("layout descriptor"));
    }

    #[test]
    fn operation_declares_calls_writes_and_births_without_a_cell() {
        let ts = super::super::tokens(r#"operation seed, calls=[bootstrap], writes=[("%Global%","x","X")], births=["%Global%"]"#).unwrap();
        let value = parse(&ts).unwrap();
        assert_eq!(value.get("operation").and_then(Value::string), Some("seed"));
        assert!(value.get("header").is_none());
        assert_eq!(
            value.get("writes").unwrap().array().unwrap()[0]
                .get("value")
                .and_then(Value::string),
            Some("X")
        );
    }
}
