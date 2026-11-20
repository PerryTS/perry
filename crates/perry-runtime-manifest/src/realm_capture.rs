//! Capture encodings derived from the typed declarations consumed at births.
use super::{info, tokens};
use std::collections::BTreeSet;

pub(super) fn number_constant(source: &str, name: &str) -> Result<u64, String> {
    let ts = tokens(source)?;
    let starts = ts
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[0].text == "const" && w[1].text == name).then_some(i))
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err("numeric capture must name one local constant".into());
    }
    let start = starts[0];
    if ts.get(start + 2).is_none_or(|t| t.text != ":")
        || ts.get(start + 3).is_none_or(|t| t.text != "f64")
        || ts.get(start + 4).is_none_or(|t| t.text != "=")
    {
        return Err("numeric capture constant must be f64".into());
    }
    let end = start
        + 5
        + ts[start + 5..]
            .iter()
            .position(|t| t.text == ";")
            .ok_or("unterminated numeric capture constant")?;
    let value = super::facts::constant(&ts[start + 5..end])?;
    let bits = value
        .get("value")
        .and_then(crate::realm_json::Value::string)
        .ok_or("capture constant is not numeric")?;
    u64::from_str_radix(bits, 16).map_err(|e| e.to_string())
}

/// The runtime installs property bindings from (name, Enum::Variant) rows
/// and captures the repr(u8) discriminant as an f64. Read those same rows;
/// neither an ordinal nor its floating-point representation is a source fact.
pub(super) fn enum_property(source: &str, name: &str, key: &str) -> Result<u64, String> {
    let ts = tokens(source)?;
    let starts = ts
        .windows(3)
        .enumerate()
        .filter_map(|(i, w)| {
            (w[0].text == "enum" && w[1].text == name && w[2].text == "{").then_some(i)
        })
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err(format!(
            "capture enum {name} must have one local declaration"
        ));
    }
    let start = starts[0];
    if start < 7 || info::text(&ts[start - 7..start]) != "#[repr(u8)]" {
        return Err("numeric enum capture requires repr(u8)".into());
    }
    let end = start
        + 3
        + ts[start + 3..]
            .iter()
            .position(|t| t.text == "}")
            .ok_or("unterminated capture enum")?;
    let mut variants = Vec::new();
    for row in info::split(&ts[start + 3..end], ",") {
        if row.len() != 1
            || row[0].text.is_empty()
            || !row[0]
                .text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("capture enum requires unit variants with implicit discriminants".into());
        }
        variants.push(row[0].text);
    }
    if variants.is_empty() || variants.len() > 256 {
        return Err("capture enum exceeds its u8 representation".into());
    }
    let mut bindings = BTreeSet::new();
    for row in ts.windows(8) {
        if row[0].text == "("
            && row[1].string.as_deref() == Some(key)
            && row[2].text == ","
            && row[3].text == name
            && row[4].text == ":"
            && row[5].text == ":"
            && matches!(row[7].text, ")" | ",")
        {
            bindings.insert(row[6].text);
        }
    }
    if bindings.len() != 1 {
        return Err(format!(
            "property {key} requires one unambiguous {name} binding"
        ));
    }
    let variant = bindings.first().unwrap();
    let ordinal = variants
        .iter()
        .position(|v| v == variant)
        .ok_or("property binding names an unknown capture variant")?;
    Ok((ordinal as f64).to_bits())
}

/// Numeric selectors belong to the existing runtime (property, selector) rows.
/// Realm source names the binding, never its machine or floating-point bits.
pub(super) fn numeric_property(source: &str, key: &str, floating: bool) -> Result<u64, String> {
    let ts = tokens(source)?;
    let mut values = BTreeSet::new();
    for i in 0..ts.len().saturating_sub(4) {
        if ts[i].text != "(" || ts[i + 1].string.as_deref() != Some(key) || ts[i + 2].text != "," {
            continue;
        }
        let end = super::closing_paren(&ts, i)?;
        let args = info::split(&ts[i + 1..end], ",");
        if args.len() < 2 {
            continue;
        }
        let literal = info::text(args[1]);
        let literal = literal.trim_end_matches("_i64");
        let value = if floating {
            literal.parse::<f64>().ok().map(f64::to_bits)
        } else {
            literal.parse::<u64>().ok()
        };
        if let Some(value) = value {
            values.insert(value);
        }
    }
    if values.len() != 1 {
        return Err(format!(
            "property {key} needs one unambiguous numeric binding"
        ));
    }
    Ok(*values.first().unwrap())
}

/// Resolve an f64 capture from the existing birth helper's named argument.
/// The property string selects the call; no selector number is repeated.
pub(super) fn number_argument(
    source: &str,
    key: &str,
    function: &str,
    argument: &str,
) -> Result<u64, String> {
    let ts = tokens(source)?;
    let starts = ts
        .windows(3)
        .enumerate()
        .filter_map(|(i, w)| {
            (w[0].text == "fn" && w[1].text == function && w[2].text == "(").then_some(i + 2)
        })
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err("capture helper needs one declaration".into());
    }
    let start = starts[0];
    let end = super::closing_paren(&ts, start)?;
    let parameters = info::split(&ts[start + 1..end], ",");
    let index = parameters
        .iter()
        .position(|arg| arg.first().is_some_and(|t| t.text == argument))
        .ok_or("unknown capture argument")?;
    let mut values = BTreeSet::new();
    for i in 0..ts.len().saturating_sub(2) {
        if ts[i].text != function || ts[i + 1].text != "(" || (i > 0 && ts[i - 1].text == "fn") {
            continue;
        }
        let end = super::closing_paren(&ts, i + 1)?;
        let args = info::split(&ts[i + 2..end], ",");
        if args.len() != parameters.len()
            || !args
                .iter()
                .any(|a| a.len() == 1 && a[0].string.as_deref() == Some(key))
        {
            continue;
        }
        let number = info::text(args[index])
            .parse::<f64>()
            .map_err(|_| "capture argument is not a numeric literal")?;
        values.insert(number.to_bits());
    }
    if values.len() != 1 {
        return Err(format!(
            "property {key} needs one unambiguous capture argument"
        ));
    }
    Ok(*values.first().unwrap())
}

pub(super) fn pointer_constant(source: &str, name: &str) -> Result<u64, String> {
    let ts = tokens(source)?;
    let starts = ts
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[0].text == "const" && w[1].text == name).then_some(i))
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err("pointer capture needs one local constant".into());
    }
    let start = starts[0];
    if info::text(&ts[start + 2..start + 5]) != ":i64=" {
        return Err("pointer capture constant must match the i64 setter ABI".into());
    }
    let end = start
        + 5
        + ts[start + 5..]
            .iter()
            .position(|t| t.text == ";")
            .ok_or("unterminated pointer capture constant")?;
    info::integer(&info::text(&ts[start + 5..end]), &info::constants(source)?)
}

/// The identity of a `runtime_symbol_atom!` symbol: `@runtime:` and the
/// description the atom declares.
fn runtime_symbol(atom: &str) -> Result<String, String> {
    let ts = tokens(include_str!(
        "../../perry-runtime/src/object/runtime_layouts/node_prototypes.rs"
    ))?;
    let mut found = None;
    for i in 0..ts.len().saturating_sub(5) {
        if ts[i].text == "runtime_symbol_atom"
            && ts[i + 1].text == "!"
            && ts[i + 2].text == "("
            && ts[i + 3].text == atom
            && ts[i + 4].text == ","
        {
            let description = ts[i + 5]
                .string
                .clone()
                .ok_or_else(|| format!("runtime symbol {atom} has no literal description"))?;
            if found.replace(description).is_some() {
                return Err(format!("runtime symbol {atom} is declared twice"));
            }
        }
    }
    found
        .map(|description| format!("@runtime:{description}"))
        .ok_or_else(|| format!("missing runtime symbol atom {atom}"))
}

pub(super) fn bindings(
    source: &str,
    key: &str,
    bindings: &[String],
    undefined: u64,
) -> Result<Vec<crate::realm_json::Value>, String> {
    use crate::realm_json::Value;
    let mut out = Vec::new();
    for binding in bindings {
        let ts = tokens(binding)?;
        let (kind, value) = if ts
            .first()
            .is_some_and(|t| matches!(t.text, "ref" | "raw_ref"))
            && ts.get(1).is_some_and(|t| t.text == "(")
            && ts.last().is_some_and(|t| t.text == ")")
        {
            (
                if ts[0].text == "raw_ref" {
                    "raw_reference"
                } else {
                    "reference"
                },
                super::facts::path(&ts[2..ts.len() - 1])?,
            )
        } else if ts.len() == 4 && ts[0].text == "symbol" && ts[1].text == "(" && ts[3].text == ")"
        {
            ("symbol", runtime_symbol(ts[2].text)?)
        } else {
            let word = if ts.len() == 4
                && ts[0].text == "pointer_constant"
                && ts[1].text == "("
                && ts[3].text == ")"
            {
                pointer_constant(source, ts[2].text)?
            } else if ts.first().is_some_and(|t| t.text == "number_argument")
                && ts.get(1).is_some_and(|t| t.text == "(")
                && ts.last().is_some_and(|t| t.text == ")")
            {
                let args = info::split(&ts[2..ts.len() - 1], ",");
                if args.len() != 2 || args.iter().any(|a| a.len() != 1) {
                    return Err("number_argument needs helper and argument identifiers".into());
                }
                number_argument(source, key, args[0][0].text, args[1][0].text)?
            } else if ts
                .first()
                .is_some_and(|t| matches!(t.text, "flag_owner" | "flag_property"))
                && ts.get(1).is_some_and(|t| t.text == "(")
                && ts.last().is_some_and(|t| t.text == ")")
            {
                let args = info::split(&ts[2..ts.len() - 1], ",");
                let owner = ts[0].text == "flag_owner";
                if args.len() != if owner { 3 } else { 2 }
                    || args[0].len() != 1
                    || args[1].len() != 1
                {
                    return Err("flag binding requires helper, argument, and optional owner".into());
                }
                let owner = if owner {
                    Some(super::facts::path(args[2])?)
                } else {
                    None
                };
                super::capture_call::flag(
                    source,
                    key,
                    args[0][0].text,
                    args[1][0].text,
                    owner.as_deref(),
                )?
            } else {
                match binding.as_str() {
                    "undefined" => undefined,
                    "number_property" => numeric_property(source, key, true)?,
                    "word_property" => numeric_property(source, key, false)?,
                    _ => return Err("unsupported symbolic capture binding".into()),
                }
            };
            ("raw_word", format!("{word:#x}"))
        };
        out.push(Value::Object(
            [
                ("kind".into(), Value::String(kind.into())),
                ("value".into(), Value::String(value)),
            ]
            .into_iter()
            .collect(),
        ));
    }
    if out.is_empty() {
        return Err("omit empty capture bindings".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn capture_arguments_follow_existing_helper_calls() {
        let source = r#"fn birth(owner:f64,name:&str,kind:u8) {} birth(owner,"method",3);"#;
        assert_eq!(
            super::number_argument(source, "method", "birth", "kind").unwrap(),
            3f64.to_bits()
        );
        assert!(super::number_argument(source, "missing", "birth", "kind").is_err());
        assert!(super::number_argument(source, "method", "birth", "missing").is_err());
        assert!(super::number_argument(
            &(source.to_owned() + r#"birth(owner,"method",4);"#),
            "method",
            "birth",
            "kind"
        )
        .is_err());
    }
    #[test]
    fn numeric_rows_and_references_derive_capture_words() {
        let source = r#"[("state", 1.0), ("op", 2_i64)]"#;
        assert_eq!(
            super::numeric_property(source, "state", true).unwrap(),
            1.0_f64.to_bits()
        );
        assert_eq!(super::numeric_property(source, "op", false).unwrap(), 2);
        assert!(super::numeric_property(&format!(r#"{source} ("op", 3)"#), "op", false).is_err());
        assert!(super::numeric_property(source, "missing", false).is_err());
        let bindings = super::bindings(
            source,
            "op",
            &[
                "undefined".into(),
                "word_property".into(),
                "ref(process.stdin)".into(),
            ],
            99,
        )
        .unwrap();
        assert_eq!(bindings[1].get("value").unwrap().string(), Some("0x2"));
        assert_eq!(bindings[2].get("kind").unwrap().string(), Some("reference"));
        assert!(super::bindings(source, "op", &["0x2".into()], 99).is_err());
    }
    #[test]
    fn capture_constants_are_resolved_without_copying_numeric_bits() {
        assert_eq!(
            super::number_constant("const OP: f64 = -0.0;", "OP").unwrap(),
            (-0.0f64).to_bits()
        );
        assert!(super::number_constant("const OP: u8 = 1;", "OP").is_err());
        assert!(super::number_constant("const OP: f64 = 1.0; const OP: f64 = 2.0;", "OP").is_err());
    }
    use super::*;
    #[test]
    fn derives_typed_property_discriminant_and_rejects_ambiguity() {
        let source = r#"#[repr(u8)] enum Prop { Closed, Flowing, }
            let rows = [("flowing", Prop::Flowing), ("closed", Prop::Closed)];"#;
        assert_eq!(
            enum_property(source, "Prop", "flowing").unwrap(),
            1_f64.to_bits()
        );
        assert!(enum_property(source, "Prop", "missing").is_err());
        assert!(enum_property(
            &format!(r#"{source} ("flowing", Prop::Closed)"#),
            "Prop",
            "flowing"
        )
        .is_err());
        assert!(
            enum_property(&source.replace("Closed,", "Closed = 4,"), "Prop", "flowing").is_err()
        );
    }
}
