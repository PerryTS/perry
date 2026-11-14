//! Final capture words belong to the source writes which bind them.
//! The native call remains unchanged; only the compiler's cell fact is folded.
use super::{facts, info, tokens};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn attach<'a>(
    rows: &mut [Value],
    sources: impl IntoIterator<Item = &'a str>,
) -> Result<(), String> {
    let mut writes = BTreeMap::new();
    for source in sources {
        let ts = tokens(source)?;
        for i in 0..ts.len().saturating_sub(3) {
            if ts[i].text != "realm_decl"
                || ts[i + 1].text != "!"
                || ts[i + 2].text != "{"
                || ts[i + 3].text != "capture_write"
            {
                continue;
            }
            let tail = &ts[i + 4..];
            let end = tail
                .iter()
                .position(|t| t.text == ";")
                .ok_or("capture write must own its setter expression")?;
            let call = &tail[..end];
            let open = call
                .iter()
                .position(|t| t.text == "(")
                .ok_or("capture write needs a setter call")?;
            if open == 0 || call[open - 1].text != "js_closure_set_capture_f64" {
                return Err("capture write must own js_closure_set_capture_f64".into());
            }
            let close = super::closing_paren(call, open)?;
            let args = info::split(&call[open + 1..close], ",");
            if args.len() != 3 {
                return Err("capture setter requires three operands".into());
            }
            let tail = &tail[end + 1..];
            if tail.len() < 3 || tail[0].text != "realm" || tail[1].text != "{" {
                return Err("capture write needs a realm fact block".into());
            }
            let end = tail[2..]
                .iter()
                .position(|t| t.text == "}")
                .ok_or("unterminated capture write facts")?
                + 2;
            let mut fields = BTreeMap::new();
            for field in info::split(&tail[2..end], ",") {
                if field.len() < 3
                    || field[1].text != "="
                    || fields.insert(field[0].text, &field[2..]).is_some()
                {
                    return Err("invalid capture write field".into());
                }
            }
            let holder = facts::path(fields.remove("holder").ok_or("missing capture holder")?)?;
            let index = info::text(fields.remove("index").ok_or("missing capture index")?)
                .parse::<usize>()
                .map_err(|_| "capture index must be an ordinal")?;
            if info::text(args[1]) != index.to_string() {
                return Err("capture write ordinal differs from its setter".into());
            }
            let binding = info::text(fields.remove("value").ok_or("missing capture value")?);
            let value = super::capture::bindings(source, "", &[binding], perry_abi::TAG_UNDEFINED)?
                .remove(0);
            if !fields.is_empty() || writes.insert((holder, index), value).is_some() {
                return Err("unknown or conflicting final capture write".into());
            }
        }
    }
    let mut applied = BTreeSet::new();
    for row in rows {
        let name = row
            .get("name")
            .and_then(Value::string)
            .unwrap_or_default()
            .to_owned();
        let Value::Object(row) = row else { continue };
        let Some(Value::Array(captures)) = row.get_mut("captures") else {
            continue;
        };
        for ((holder, index), value) in &writes {
            if holder == &name {
                let slot = captures
                    .get_mut(*index)
                    .ok_or("capture write exceeds its birth")?;
                *slot = value.clone();
                applied.insert((holder.clone(), *index));
            }
        }
    }
    if applied.len() != writes.len() {
        return Err("capture write names no declared closure birth".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_write_folds_the_existing_capture_and_rejects_bad_ordinals() {
        let source = r#"realm_decl! { capture_write crate::closure::js_closure_set_capture_f64(getter,0,ctor); realm { holder="A.get",index=0,value=ref(A) } }"#;
        let original =
            crate::realm_json::parse(r#"{"name":"A.get","captures":[{"kind":"undefined"}]}"#)
                .unwrap();
        let mut rows = vec![original.clone()];
        attach(&mut rows, [source]).unwrap();
        assert_eq!(
            rows[0].get("captures").unwrap().array().unwrap()[0]
                .get("value")
                .and_then(Value::string),
            Some("A")
        );
        let invalid = source.replace("index=0", "index=1");
        assert!(attach(&mut [original.clone()], [invalid.as_str()]).is_err());
        let absent = source.replace("A.get", "B.get");
        assert!(attach(&mut [original], [absent.as_str()]).is_err());
    }
}
