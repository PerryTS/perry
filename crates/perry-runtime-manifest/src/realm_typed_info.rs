//! Existing named JsFunctionInfo declarations remain the callable ABI owner.
//! A realm_decl block can name a shared static without introducing another one.
use super::{info, tokens, Token};
use std::collections::BTreeMap;

fn initializer<'a, 's>(ts: &'a [Token<'s>], name: &str) -> Result<&'a [Token<'s>], String> {
    let starts = ts
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| {
            (matches!(w[0].text, "static" | "const") && w[1].text == name).then_some(i)
        })
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err(format!(
            "info binding {name} must name one local static/const"
        ));
    }
    let start = starts[0];
    let equals = start
        + ts[start..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing info initializer")?;
    info::split(&ts[equals + 1..], ";")
        .first()
        .copied()
        .ok_or_else(|| "empty info initializer".into())
}
fn wrapper(source: &[Token<'_>], name: &str) -> Result<(), String> {
    let start = source
        .windows(4)
        .position(|w| {
            w[0].text == "macro_rules" && w[1].text == "!" && w[2].text == name && w[3].text == "{"
        })
        .ok_or_else(|| format!("missing info wrapper {name}"))?;
    let mut end = start + 4;
    let mut depth = 1;
    while depth != 0 {
        match source.get(end).ok_or("unterminated info wrapper")?.text {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    let text = info::text(&source[start + 4..end - 1]);
    // This existing wrapper is only a typed pointer cast. Its own definition
    // must prove that; accepting a macro name alone would invent ABI facts.
    if text != "($body:path,$n:tt)=>{JsFunctionInfo::of($bodyascrate::fn_info!(@ty$n))};" {
        return Err(format!("unsupported info wrapper definition {name}"));
    }
    Ok(())
}
pub(super) fn named(
    source: &str,
    name: &str,
    constants: &BTreeMap<String, u64>,
) -> Result<info::Info, String> {
    let all = tokens(source)?;
    let ts = initializer(&all, name)?;
    expression(source, ts, constants)
}

pub(super) fn expression(
    source: &str,
    ts: &[Token<'_>],
    constants: &BTreeMap<String, u64>,
) -> Result<info::Info, String> {
    let open = ts
        .iter()
        .position(|t| t.text == "(")
        .ok_or("info initializer has no constructor")?;
    let end = super::closing_paren(ts, open)?;
    let constructor = info::text(&ts[..open]);
    let args = info::split(&ts[open + 1..end], ",");
    let mut signature = if constructor.ends_with("!") {
        wrapper(&tokens(source)?, constructor.trim_end_matches('!'))?;
        if args.len() != 2 {
            return Err("typed info wrapper needs body and arity".into());
        }
        format!("{},{}", info::text(args[0]), info::text(args[1]))
    } else if constructor.ends_with("JsFunctionInfo::of_native_args") {
        if args.len() != 2 {
            return Err("native info needs body and fixed arity".into());
        }
        let cast = args[0]
            .iter()
            .position(|t| t.text == "as")
            .unwrap_or(args[0].len());
        format!(
            "native_args {},{}",
            info::text(&args[0][..cast]),
            info::text(args[1])
        )
    } else if constructor.ends_with("JsFunctionInfo::of") {
        if args.len() != 1 {
            return Err("typed info needs one body".into());
        }
        let cast = args[0]
            .iter()
            .position(|t| t.text == "as")
            .ok_or("typed info needs an explicit body type")?;
        let arity = args[0][cast + 1..]
            .iter()
            .find_map(|t| {
                t.text
                    .strip_prefix("JsBody")
                    .and_then(|n| n.parse::<u64>().ok())
            })
            .ok_or("unknown typed body ABI")?;
        format!("{},{}", info::text(&args[0][..cast]), arity)
    } else {
        return Err(format!("unsupported info constructor {constructor}"));
    };
    let mut next = end + 1;
    let mut first = true;
    while next < ts.len() {
        if ts.get(next).is_none_or(|t| t.text != ".")
            || ts.get(next + 2).is_none_or(|t| t.text != "(")
        {
            return Err("unknown info initializer suffix".into());
        }
        let close = super::closing_paren(ts, next + 2)?;
        signature.push(if first { ';' } else { ',' });
        first = false;
        signature.push_str(&info::text(&ts[next + 1..close + 1]));
        next = close + 1;
    }
    info::fn_info(&tokens(&signature)?, constants)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_infos_derive_native_arguments_and_reject_unowned_bindings() {
        let source="static EMIT: JsFunctionInfo = JsFunctionInfo::of_native_args(emit,1).with_declared(2);";
        let info = named(source, "EMIT", super::super::abi_constants()).unwrap();
        assert_eq!(info.params, 0);
        assert_eq!(info.rest_fixed, 1);
        assert_eq!(info.declared, 2);
        assert!(named(source, "MISSING", super::super::abi_constants()).is_err());
        assert!(named(
            &format!("{source}{source}"),
            "EMIT",
            super::super::abi_constants()
        )
        .is_err());
    }
}
