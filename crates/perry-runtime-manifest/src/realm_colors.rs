//! The inspect color map and its numeric pairs share the runtime's AnsiStyle
//! declarations. No color values, key order or array encoding is restated.
use super::{info, tokens};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn number(value: u64) -> Value {
    Value::Number(value.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn property(key: &str, attrs: u64, value: Value) -> Value {
    object([
        ("key", string(key)),
        ("symbol", Value::Bool(false)),
        ("attributes", number(attrs)),
        ("value", value),
    ])
}
fn numeric(value: f64) -> Value {
    object([
        ("kind", string("number_bits")),
        ("value", string(format!("{:016x}", value.to_bits()))),
    ])
}
fn rows(source: &str) -> Result<Vec<(String, i32, i32)>, String> {
    let ts = tokens(source)?;
    let start = ts
        .windows(2)
        .position(|w| w[0].text == "const" && w[1].text == "INSPECT_COLOR_STYLES")
        .ok_or("missing AnsiStyle declaration")?;
    let equals = start
        + ts[start..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing color values")?;
    if info::text(&ts[equals + 1..equals + 3]) != "&[" {
        return Err("color styles must be a slice".into());
    }
    let end = equals
        + 3
        + ts[equals + 3..]
            .iter()
            .position(|t| t.text == "]")
            .ok_or("unterminated color styles")?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for row in info::split(&ts[equals + 3..end], ",") {
        if row.len() < 3
            || row[0].text != "AnsiStyle"
            || row[1].text != "{"
            || row.last().unwrap().text != "}"
        {
            return Err("color values must be typed AnsiStyle rows".into());
        }
        let mut fields = BTreeMap::new();
        for field in info::split(&row[2..row.len() - 1], ",") {
            if field.len() < 3
                || field[1].text != ":"
                || fields.insert(field[0].text, &field[2..]).is_some()
            {
                return Err("invalid color field".into());
            }
        }
        if fields.len() != 3 {
            return Err("AnsiStyle needs name, open and close".into());
        }
        let key = fields
            .get("name")
            .filter(|v| v.len() == 1)
            .and_then(|v| v[0].string.clone())
            .ok_or("missing color name")?;
        if !seen.insert(key.clone()) {
            return Err("duplicate color name".into());
        }
        let integer = |name| -> Result<i32, String> {
            info::text(fields.get(name).ok_or("missing color code")?)
                .parse::<i32>()
                .map_err(|e| e.to_string())
        };
        result.push((key, integer("open")?, integer("close")?));
    }
    Ok(result)
}
fn expand(name: &str, styles: &str) -> Result<Vec<Value>, String> {
    let rows = rows(styles)?;
    let abi = super::abi_constants();
    let gc = info::constants(include_str!("../../perry-runtime/src/gc/types.rs"))?;
    let mut declarations = Vec::new();
    let mut properties = Vec::new();
    let mut keys = Vec::new();
    let mut births = BTreeSet::from(["Object.prototype".to_string()]);
    for (key, open, close) in rows {
        let path = format!("{name}.{key}");
        keys.push(string(&key));
        births.insert(path.clone());
        properties.push(property(
            &key,
            7,
            object([("kind", string("reference")), ("value", string(&path))]),
        ));
        declarations.push(object([
            ("name", string(path)),
            ("kind", string("namespace")),
            (
                "properties",
                Value::Array(vec![
                    property("0", 7, numeric(open as f64)),
                    property("1", 7, numeric(close as f64)),
                    property("length", 1, numeric(2.0)),
                ]),
            ),
            (
                "own_key_order",
                Value::Array(["0", "1", "length"].into_iter().map(string).collect()),
            ),
            ("own_symbol_order", Value::Array(Vec::new())),
            (
                "header",
                object([
                    (
                        "gc_type",
                        number(*gc.get("GC_TYPE_ARRAY").ok_or("missing array ABI")?),
                    ),
                    (
                        "reserved",
                        number(
                            *abi.get("GC_LAYOUT_POINTER_FREE")
                                .ok_or("missing numeric array layout")?,
                        ),
                    ),
                ]),
            ),
            ("prototype", string("Array.prototype")),
            ("births", Value::Array(vec![string("Array.prototype")])),
        ]));
    }
    properties.sort_by_key(|p| p.get("key").and_then(Value::string).unwrap().to_owned());
    declarations.push(object([
        ("name", string(name)),
        ("kind", string("namespace")),
        ("properties", Value::Array(properties)),
        ("own_key_order", Value::Array(keys)),
        ("own_symbol_order", Value::Array(Vec::new())),
        (
            "header",
            object([
                (
                    "gc_type",
                    number(*abi.get("GC_TYPE_OBJECT").ok_or("missing object ABI")?),
                ),
                ("reserved", number(0)),
                ("class_id", number(0)),
            ]),
        ),
        ("prototype", string("Object.prototype")),
        (
            "births",
            Value::Array(births.into_iter().map(string).collect()),
        ),
    ]));
    Ok(declarations)
}
pub(super) fn birth(name: &str, source: &str) -> Result<Vec<Value>, String> {
    let source = info::text(&tokens(source)?);
    if !source.contains("forstyleincrate::util_style_text::INSPECT_COLOR_STYLES{native_set_field(obj,style.name,native_color_tuple(style.open,style.close));}") {
        return Err("AnsiStyle provider must consume the declared color rows at its birth".into());
    }
    if !source.contains("fnnative_color_tuple(open:i32,close:i32)->f64{letarr=crate::array::js_array_alloc_with_length(2);crate::array::js_array_set_f64(arr,0,openasf64);crate::array::js_array_set_f64(arr,1,closeasf64);f64::from_bits(JSValue::array_ptr(arr).bits())}") {
        return Err("AnsiStyle numeric pair birth changed".into());
    }
    expand(
        name,
        include_str!("../../perry-runtime/src/util_style_text.rs"),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_color_rows_define_children_values_and_order() {
        let source = r#"const INSPECT_COLOR_STYLES:&[AnsiStyle]=&[AnsiStyle{name:"z",open:90,close:39},AnsiStyle{name:"a",open:1,close:22}];"#;
        let cells = expand("colors", source).unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(
            cells[0].get("name").and_then(Value::string),
            Some("colors.z")
        );
        assert_eq!(
            cells[2].get("own_key_order").unwrap().encode(),
            r#"["z","a"]"#
        );
        assert!(expand("colors", &source.replace("name:\"a\"", "name:\"z\"")).is_err());
        assert!(birth("colors", "").is_err());
    }
}
