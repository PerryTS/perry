//! Boolean capture bindings follow existing helper calls at their owning births.
use super::{info, tokens, Token};
use std::collections::BTreeSet;
fn close_block(ts: &[Token<'_>], start: usize) -> Result<usize, String> {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(start) {
        if t.text == "{" {
            depth += 1;
        }
        if t.text == "}" {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
    }
    Err("unterminated capture provider".into())
}
fn argument_index(ts: &[Token<'_>], function: &str, argument: &str) -> Result<usize, String> {
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
    let end = super::closing_paren(ts, start)?;
    info::split(&ts[start + 1..end], ",")
        .iter()
        .position(|p| p.first().is_some_and(|t| t.text == argument))
        .ok_or("unknown capture helper argument".into())
}
pub(super) fn flag(
    source: &str,
    key: &str,
    function: &str,
    argument: &str,
    owner: Option<&str>,
) -> Result<u64, String> {
    let ts = tokens(source)?;
    let index = argument_index(&ts, function, argument)?;
    let mut regions = Vec::new();
    if let Some(owner) = owner {
        for i in 0..ts.len().saturating_sub(3) {
            if ts[i].text != "fn" {
                continue;
            }
            let Some(offset) = ts[i + 2..].iter().position(|t| t.text == "{") else {
                continue;
            };
            let start = i + 2 + offset;
            let end = close_block(&ts, start)?;
            let body = &ts[start + 1..end];
            if body.windows(3).any(|w| {
                w[0].text == "realm" && w[1].text == "{" && w[2].string.as_deref() == Some(owner)
            }) {
                regions.push((start + 1, end));
            }
        }
        if regions.len() != 1 {
            return Err(format!("capture owner {owner} needs one birth function"));
        }
    } else {
        regions.push((0, ts.len()));
    }
    let mut values = BTreeSet::new();
    for (start, end) in regions {
        for i in start..end.saturating_sub(2) {
            if ts[i].text != function || ts[i + 1].text != "(" || (i > 0 && ts[i - 1].text == "fn")
            {
                continue;
            }
            if owner.is_none()
                && !(i >= 3
                    && ts[i - 3].text == "("
                    && ts[i - 2].string.as_deref() == Some(key)
                    && ts[i - 1].text == ",")
            {
                continue;
            }
            let end = super::closing_paren(&ts, i + 1)?;
            let args = info::split(&ts[i + 2..end], ",");
            let value = info::text(args.get(index).ok_or("missing capture argument")?);
            values.insert(match value.as_str() {
                "false" => 0,
                "true" => 1,
                _ => return Err("capture flag argument must be an existing boolean literal".into()),
            });
        }
    }
    if values.len() != 1 {
        return Err("capture flag binding is missing or ambiguous".into());
    }
    Ok(*values.first().unwrap())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boolean_bindings_follow_runtime_calls_and_fail_closed() {
        let source = r#"fn helper(secure:bool,once:bool){} fn birth(){ let x=realm_decl!{birth allocate();realm{"node:test";}}; helper(true,false); } fn row(){ let x=[("on",helper(false,false)),("once",helper(false,true))]; }"#;
        assert_eq!(
            flag(source, "", "helper", "secure", Some("node:test")).unwrap(),
            1
        );
        assert_eq!(flag(source, "once", "helper", "once", None).unwrap(), 1);
        assert_eq!(flag(source, "on", "helper", "once", None).unwrap(), 0);
        assert!(flag(source, "", "helper", "secure", Some("missing")).is_err());
    }
}
