//! Namespace facts come from the typed Member rows consumed by their runtime
//! factory. Member order, constants and tags are not repeated in realm source.
use super::{info, tokens};
use crate::realm_json::Value;
use std::collections::BTreeSet;

fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn number(value: u64) -> Value {
    Value::Number(value.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn numeric(expression: &str) -> Result<f64, String> {
    Ok(match expression {
        "std::f64::consts::E" => std::f64::consts::E,
        "std::f64::consts::LN_10" => std::f64::consts::LN_10,
        "std::f64::consts::LN_2" => std::f64::consts::LN_2,
        "std::f64::consts::LOG10_E" => std::f64::consts::LOG10_E,
        "std::f64::consts::LOG2_E" => std::f64::consts::LOG2_E,
        "std::f64::consts::PI" => std::f64::consts::PI,
        "std::f64::consts::FRAC_1_SQRT_2" => std::f64::consts::FRAC_1_SQRT_2,
        "std::f64::consts::SQRT_2" => std::f64::consts::SQRT_2,
        _ => expression
            .parse::<f64>()
            .map_err(|_| format!("unknown namespace constant {expression}"))?,
    })
}
fn bracket_end(ts: &[super::Token<'_>], start: usize) -> Result<usize, String> {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(start) {
        if t.text == "[" {
            depth += 1;
        }
        if t.text == "]" {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
    }
    Err("unterminated namespace members".into())
}
pub(super) fn sealed_namespace(name: &str) -> Result<bool, String> {
    let ts = tokens(include_str!("../../perry-abi/src/lib.rs"))?;
    let start = ts
        .windows(2)
        .position(|w| w[0].text == "const" && w[1].text == "BUILTIN_NAMESPACES")
        .ok_or("missing namespace family declarations")?;
    let equals = start
        + ts[start..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing namespace family values")?;
    let end = bracket_end(&ts, equals + 1)?;
    Ok(ts[equals + 2..end]
        .iter()
        .any(|t| t.string.as_deref() == Some(name)))
}

pub(super) fn birth(
    name: &str,
    table: &str,
    source: &str,
    extra_births: Vec<String>,
) -> Result<Value, String> {
    let ts = tokens(source)?;
    if !ts
        .windows(7)
        .any(|w| info::text(w) == format!("builtin_namespace::create({table})"))
    {
        return Err("namespace Member declaration is not consumed by its birth".into());
    }
    let start = ts
        .windows(2)
        .position(|w| w[0].text == "const" && w[1].text == table)
        .ok_or_else(|| format!("missing namespace declaration {table}"))?;
    let equals = start
        + ts[start..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing namespace members")?;
    if info::text(&ts[equals + 1..equals + 3]) != "&[" {
        return Err("namespace members must be an ordered slice".into());
    }
    let end = bracket_end(&ts, equals + 2)?;
    let mut properties = Vec::new();
    let mut keys = Vec::new();
    let mut symbols = Vec::new();
    let mut seen = BTreeSet::new();
    let mut births = extra_births.into_iter().collect::<BTreeSet<_>>();
    births.insert("Object.prototype".into());
    for row in info::split(&ts[equals + 3..end], ",") {
        let open = row
            .iter()
            .position(|t| t.text == "(")
            .ok_or("malformed namespace member")?;
        if row.last().is_none_or(|t| t.text != ")") {
            return Err("malformed namespace member call".into());
        }
        let constructor = info::text(&row[..open]);
        let args = info::split(&row[open + 1..row.len() - 1], ",");
        let label = args
            .first()
            .filter(|a| a.len() == 1)
            .and_then(|a| a[0].string.clone())
            .ok_or("namespace member needs a literal key")?;
        let (key, symbol, attrs, value) = match constructor.as_str() {
            "Member::method" if args.len() == 3 => {
                let target = format!("{name}.{label}");
                births.insert(target.clone());
                (
                    label,
                    false,
                    5,
                    object([("kind", string("reference")), ("value", string(target))]),
                )
            }
            "Member::constant" if args.len() == 2 => {
                let number = numeric(&info::text(args[1]))?;
                (
                    label,
                    false,
                    0,
                    object([
                        ("kind", string("number_bits")),
                        ("value", string(format!("{:016x}", number.to_bits()))),
                    ]),
                )
            }
            "Member::tag" if args.len() == 1 => (
                "@@toStringTag".into(),
                true,
                4,
                object([("kind", string("string")), ("value", string(label))]),
            ),
            _ => return Err(format!("unsupported namespace member {constructor}")),
        };
        if !seen.insert((symbol, key.clone())) {
            return Err(format!("duplicate namespace key {key}"));
        }
        if symbol {
            symbols.push(string(key.clone()));
        } else {
            keys.push(string(key.clone()));
        }
        properties.push(object([
            ("key", string(key)),
            ("symbol", Value::Bool(symbol)),
            ("attributes", number(attrs)),
            ("value", value),
        ]));
    }
    properties.sort_by_key(|p| {
        (
            p.get("symbol") == Some(&Value::Bool(true)),
            p.get("key").and_then(Value::string).unwrap().to_owned(),
        )
    });
    let flags = info::gc_constants()?;
    let reserved = flags
        .get("OBJ_FLAG_HAS_DESCRIPTORS")
        .ok_or("missing descriptor flag")?
        | if sealed_namespace(name)? {
            *flags
                .get("OBJ_FLAG_NEVER_MOVE")
                .ok_or("missing intrinsic stability flag")?
        } else {
            0
        };
    let abi = info::constants(include_str!("../../perry-abi/src/lib.rs"))?;
    births.remove(name);
    Ok(object([
        ("name", string(name)),
        ("kind", string("namespace")),
        ("properties", Value::Array(properties)),
        ("own_key_order", Value::Array(keys)),
        ("own_symbol_order", Value::Array(symbols)),
        (
            "header",
            object([
                (
                    "gc_type",
                    number(*abi.get("GC_TYPE_OBJECT").ok_or("missing object ABI kind")?),
                ),
                ("reserved", number(reserved)),
                ("class_id", number(0)),
            ]),
        ),
        ("prototype", string("Object.prototype")),
        (
            "births",
            Value::Array(births.into_iter().map(string).collect()),
        ),
    ]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_rows_supply_order_attributes_constants_and_symbol_tags() {
        let source = r#"const MEMBERS:&[Member]=&[Member::method("z",fn_info!(body,1),1),Member::constant("a",-0.0),Member::tag("Math")]; fn make(){builtin_namespace::create(MEMBERS)}"#;
        let d = birth("Math", "MEMBERS", source, Vec::new()).unwrap();
        assert_eq!(d.get("own_key_order").unwrap().encode(), r#"["z","a"]"#);
        let properties = d.get("properties").unwrap().array().unwrap();
        assert_eq!(
            properties[0]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("8000000000000000")
        );
        assert_eq!(properties[2].get("symbol"), Some(&Value::Bool(true)));
        assert!(birth(
            "Math",
            "MEMBERS",
            &source.replace("Member::tag", "Member::unknown"),
            Vec::new()
        )
        .is_err());
    }
}
