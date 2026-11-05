//! Resolve internal-cell facts from a literal owned by the runtime birth.
//! `entries_source` exists only in a source declaration; the emitted manifest
//! carries ordinary RealmValue entries, consumed identically on every target.
use crate::realm_json::Value;
use crate::realm_source::{tokens, Token};
use std::collections::BTreeMap;

pub(super) fn resolve(
    source: &str,
    declaration: &mut BTreeMap<String, Value>,
) -> Result<(), String> {
    let Some(Value::Object(storage)) = declaration.get_mut("storage") else {
        return Ok(());
    };
    let Some(reference) = storage.remove("entries_source") else {
        return Ok(());
    };
    let reference = reference
        .string()
        .ok_or("entries_source must be a scoped constant name")?;
    if storage.get("kind").and_then(Value::string) != Some("set") || storage.contains_key("entries")
    {
        return Err("entries_source requires Set storage without duplicate entries".into());
    }
    let (function, constant) = reference
        .split_once("::")
        .ok_or("entries_source must name function::CONSTANT")?;
    if function.is_empty() || constant.is_empty() || constant.contains("::") {
        return Err("entries_source must name function::CONSTANT".into());
    }
    let ts = tokens(source)?;
    let mut candidates = ts
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0].text == "fn" && w[1].text == function);
    let (start, _) = candidates
        .next()
        .ok_or_else(|| format!("storage owner {function} was not found"))?;
    if candidates.next().is_some() {
        return Err(format!("ambiguous storage owner {function}"));
    }
    let open = (start + 2..ts.len())
        .find(|&i| matches!(ts[i].text, "{" | ";"))
        .ok_or("storage owner has no body")?;
    if ts[open].text != "{" {
        return Err("storage owner has no body".into());
    }
    let end = matching_brace(&ts, open)?;
    let body = &ts[open + 1..end];
    let mut matches = body
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0].text == "const" && w[1].text == constant);
    let (at, _) = matches
        .next()
        .ok_or_else(|| format!("storage constant {reference} was not found"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous storage constant {reference}"));
    }
    let eq = (at + 2..body.len())
        .find(|&i| matches!(body[i].text, "=" | ";"))
        .ok_or("storage constant has no initializer")?;
    if body[eq].text != "=" {
        return Err("storage constant has no initializer".into());
    }
    let mut at = eq + 1;
    if body.get(at).is_some_and(|t| t.text == "&") {
        at += 1;
    }
    if body.get(at).is_none_or(|t| t.text != "[") {
        return Err("storage entries must be a literal string slice".into());
    }
    at += 1;
    let mut entries = Vec::new();
    while body.get(at).is_some_and(|t| t.text != "]") {
        let text = body[at]
            .string
            .as_ref()
            .ok_or("storage entry is not a string literal")?;
        entries.push(Value::Object(BTreeMap::from([
            ("kind".into(), Value::String("string".into())),
            ("value".into(), Value::String(text.clone())),
        ])));
        at += 1;
        match body.get(at).map(|t| t.text) {
            Some(",") => at += 1,
            Some("]") => {}
            _ => return Err("storage entries must be separated by commas".into()),
        }
    }
    if body.get(at).is_none_or(|t| t.text != "]") || body.get(at + 1).is_none_or(|t| t.text != ";")
    {
        return Err("storage entries must be a complete literal initializer".into());
    }
    storage.insert("entries".into(), Value::Array(entries));
    Ok(())
}

fn matching_brace(ts: &[Token<'_>], start: usize) -> Result<usize, String> {
    let mut depth = 0;
    for (at, token) in ts.iter().enumerate().skip(start) {
        match token.text {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    return Ok(at);
                }
            }
            _ => {}
        }
    }
    Err("unterminated storage owner body".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn declaration() -> BTreeMap<String, Value> {
        let Value::Object(d) = crate::realm_json::parse(
            r#"{"storage":{"kind":"set","entries_source":"birth::FLAGS"}}"#,
        )
        .unwrap() else {
            unreachable!()
        };
        d
    }
    #[test]
    fn entries_are_read_from_the_named_birth_without_a_second_list() {
        let mut d = declaration();
        resolve(r##"const FLAGS: &[&str] = &["wrong"]; fn birth() { const FLAGS: &[&str] = &["--a", /* ] */ r#"--b"#]; use_flags(FLAGS) }"##, &mut d).unwrap();
        let entries = d["storage"].get("entries").unwrap().array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("value").and_then(Value::string), Some("--a"));
        assert_eq!(entries[1].get("value").and_then(Value::string), Some("--b"));
        assert!(d["storage"].get("entries_source").is_none());
    }
    #[test]
    fn incomplete_or_ambiguous_birth_facts_are_errors() {
        for source in [
            "fn birth() {}",
            "fn birth() { const FLAGS: &[&str] = make_flags(); }",
            "fn birth() { const FLAGS: &[&str] = &[OTHER]; }",
            "fn birth() { const FLAGS: &[&str] = &[]; } fn birth() {}",
        ] {
            assert!(resolve(source, &mut declaration()).is_err(), "{source}");
        }
    }
}
