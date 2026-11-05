//! The throws clause of the existing source-owned realm carrier.
use super::{closing_paren, facts, info, source_files, symbols, tokens, Token};
use crate::realm_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn operands(ts: &[Token<'_>]) -> Result<Value, String> {
    let ts =
        if ts.first().is_some_and(|t| t.text == "{") && ts.last().is_some_and(|t| t.text == "}") {
            &ts[1..ts.len() - 1]
        } else {
            ts
        };
    let mut out = Vec::new();
    for operand in info::split(ts, ";") {
        if operand.len() < 4
            || operand[0].text != "passes"
            || operand[1].text != "("
            || operand.last().unwrap().text != ")"
        {
            return Err("throws expects passes(i) or passes(Source.path, i)".into());
        }
        let args = info::split(&operand[2..operand.len() - 1], ",");
        if ![1, 2].contains(&args.len()) {
            return Err("passes needs an argument, optionally with its stored source".into());
        }
        let index = info::text(args.last().unwrap())
            .parse::<u16>()
            .map_err(|_| "passes argument must be a literal u16")?;
        let mut value = BTreeMap::from([
            ("argument".into(), Value::Number(index.to_string())),
            ("source".into(), Value::Null),
        ]);
        if args.len() == 2 {
            let source = facts::path(args[0])?;
            if source.is_empty() || source.contains([',', ';', ':', '\t', '\n']) {
                return Err("invalid stored passes source path".into());
            }
            value.insert("source".into(), Value::String(source));
        }
        let value = Value::Object(value);
        if out.contains(&value) {
            return Err("duplicate passes operand".into());
        }
        out.push(value);
    }
    if out.is_empty() {
        return Err("throws needs at least one passes operand".into());
    }
    Ok(Value::Array(out))
}

pub(super) fn declarations(
    source: &str,
    path: &str,
    mut resolve: impl FnMut(&str) -> Result<(String, String), String>,
) -> Result<Vec<Value>, String> {
    if !source.contains("throws") || !source.contains("realm") {
        return Ok(Vec::new());
    }
    let ts = tokens(source)?;
    let mut out = Vec::new();
    for i in 2..ts.len() {
        if ts[i].text != "realm" || ts[i - 1].text != "[" || ts[i - 2].text != "#" {
            continue;
        }
        let end = closing_paren(&ts, i + 1)?;
        if !ts.get(end + 2).is_some_and(|t| t.text == "declare") {
            if !info::split(&ts[i + 2..end], ",")
                .iter()
                .any(|c| c.first().is_some_and(|t| t.text == "throws"))
            {
                continue;
            }
            let humans = facts::parse_many(&ts[i + 2..end])?;
            if !humans.iter().any(|h| h.throws.is_some()) {
                continue;
            }
            let owner = (0..i)
                .rev()
                .find(|&j| {
                    ts[j].text == "fn_info"
                        && ts.get(j + 1).is_some_and(|t| t.text == "!")
                        && ts.get(j + 2).is_some_and(|t| t.text == "(")
                        && closing_paren(&ts, j + 2).is_ok_and(|j| j > end)
                })
                .ok_or("throws declaration must belong to fn_info!")?;
            let close = closing_paren(&ts, owner + 2)?;
            let body = info::fn_info(&ts[end + 2..close], super::abi_constants())?;
            let (_, symbol) = resolve(&body.body)?;
            for human in humans {
                if let Some(ops) = human.throws {
                    out.push(Value::Object(BTreeMap::from([
                        (
                            "symbol".into(),
                            Value::String(symbol.trim_start_matches("source:").into()),
                        ),
                        ("operands".into(), ops),
                        ("source".into(), Value::String(path.into())),
                        (
                            "line".into(),
                            Value::Number(
                                (source[..ts[i].start]
                                    .bytes()
                                    .filter(|&b| b == b'\n')
                                    .count()
                                    + 1)
                                .to_string(),
                            ),
                        ),
                    ])));
                }
            }
            continue;
        }
        let head = &ts[i + 2..end];
        if head.len() < 3 || head[0].text != "throws" || head[1].text != "=" {
            return Err("declaration-only realm accepts throws = passes(..)".into());
        }
        // Require the attribute to belong to the same fn_info! invocation.
        let owner = (0..i)
            .rev()
            .find(|&j| {
                ts[j].text == "fn_info"
                    && ts.get(j + 1).is_some_and(|t| t.text == "!")
                    && ts.get(j + 2).is_some_and(|t| t.text == "(")
                    && closing_paren(&ts, j + 2).is_ok_and(|j| j > end)
            })
            .ok_or("throws declaration must belong to fn_info!")?;
        let close = closing_paren(&ts, owner + 2)?;
        let function = (end + 3..close)
            .find(|&j| ts[j].text == "fn")
            .ok_or("throws declaration must wrap a function definition")?;
        let name = ts
            .get(function + 1)
            .ok_or("missing throws function name")?
            .text;
        let args_end = closing_paren(&ts, function + 2)?;
        let arity = info::split(&ts[function + 3..args_end], ",")
            .iter()
            .filter(|p| !p.is_empty())
            .count();
        let parsed = operands(&head[2..])?;
        for op in parsed.array().unwrap() {
            if op.get("source") == Some(&Value::Null) {
                let index = op
                    .get("argument")
                    .unwrap()
                    .encode()
                    .parse::<usize>()
                    .map_err(|_| "invalid passes index")?;
                if index >= arity {
                    return Err(format!(
                        "{name}: passes({index}) exceeds {arity} formal arguments"
                    ));
                }
            }
        }
        let (_, symbol) = resolve(name)?;
        out.push(Value::Object(BTreeMap::from([
            (
                "symbol".into(),
                Value::String(symbol.trim_start_matches("source:").into()),
            ),
            ("operands".into(), parsed),
            ("source".into(), Value::String(path.into())),
            (
                "line".into(),
                Value::Number(
                    (source[..ts[i].start]
                        .bytes()
                        .filter(|&b| b == b'\n')
                        .count()
                        + 1)
                    .to_string(),
                ),
            ),
        ])));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passes_grammar_rejects_false_or_malformed_operands() {
        for good in [
            "passes(0)",
            "passes(Promise.reject, 0)",
            "{passes(1);passes(Store.put,0)}",
        ] {
            assert!(operands(&tokens(good).unwrap()).is_ok(), "{good}");
        }
        for bad in [
            "core",
            "passes()",
            "passes(-1)",
            "passes(x)",
            "passes(65536)",
            "{passes(0);passes(0)}",
        ] {
            assert!(operands(&tokens(bad).unwrap()).is_err(), "{bad}");
        }
    }
    #[test]
    fn declaration_only_carrier_has_a_real_function_and_provenance() {
        let src="crate::fn_info!(#[realm(throws=passes(1))] declare #[no_mangle] pub extern \"C\" fn f(a:f64,b:f64)->f64 { b });";
        let ds = declarations(src, "src/test.rs", |name| {
            Ok((name.into(), format!("source:{name}")))
        })
        .unwrap();
        assert_eq!(ds[0].get("symbol").and_then(Value::string), Some("f"));
        assert_eq!(
            ds[0].get("source").and_then(Value::string),
            Some("src/test.rs")
        );
        assert!(declarations(
            "#[realm(throws=passes(0))] declare fn f() {}",
            "x",
            |_| unreachable!()
        )
        .is_err());
    }
}

pub(super) fn read(runtime_src: &Path) -> Result<Value, String> {
    let runtime_src = runtime_src.canonicalize().map_err(|e| e.to_string())?;
    let crates = runtime_src
        .parent()
        .and_then(Path::parent)
        .ok_or("runtime source has no crates parent")?;
    let mut files = Vec::new();
    for package in ["perry-runtime", "perry-stdlib"] {
        let src = crates.join(package).join("src");
        if src.is_dir() {
            source_files(&src, &mut files).map_err(|e| e.to_string())?;
        }
    }
    let sources: Vec<_> = files
        .into_iter()
        .map(|p| std::fs::read_to_string(&p).map(|s| (p, s)))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let symbols = symbols::Sources::new(&sources, crates)?;
    let mut result = Vec::new();
    for (file, source) in &sources {
        result.extend(declarations(
            source,
            &file.strip_prefix(crates).unwrap().to_string_lossy(),
            |body| symbols.resolve(body, file),
        )?);
    }
    Ok(Value::Array(result))
}
