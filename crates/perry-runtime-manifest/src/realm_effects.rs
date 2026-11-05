//! The `effects = { ... }` clause of a realm attribute: what a built-in does
//! to its inputs, in a few verbs. Parsed here into the manifest's JSON; the
//! typed schema is `realm_effect.rs` (serde, read by the compiler builds).
//!
//! ```text
//! effects  := "{" effect (";" effect)* ";"? "}"
//! effect   := reads(slot, ...) | coerce(slot, Protocol) | stores(slot -> slot) | stores(fresh)
//!           | calls(slot, this = operand, args = [operand, ...])
//!           | returns(operand) | open
//! slot     := this | result | arg(N) | rest(N) | returned(N) | elems(slot) | field(slot, key)
//! operand  := value ("|" value)*
//! value    := slot | prim(kind | kind ...) | fresh(Realm.path, elems = [slot, ...])
//!           | species(slot, args = [operand, ...], elems = [slot, ...])
//!
//! `returned(N)`: the value the N-th `calls` of this declaration (0-based)
//! returned. `species(..)`: ArraySpeciesCreate on the slot (see
//! `RealmOperand::Species`).
//! Protocol := ToPrimitive | Iterate | IterNext | IterResult | Json | HasInstance | OwnKeys
//! kind     := undefined | null | bool | num | str | bigint | symbol
//! ```
use super::info;
use super::Token;
use crate::realm_json::Value;

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn tagged(tag: &str, value: Value) -> Value {
    object(vec![(tag, value)])
}

/// `name ( inner )` -> (name, inner); a bare word -> (name, None).
fn call<'a, 's>(ts: &'a [Token<'s>]) -> Result<(&'s str, Option<&'a [Token<'s>]>), String> {
    let text = info::text(ts);
    match ts {
        [] => Err("empty effect term".into()),
        [word] => Ok((word.text, None)),
        [word, open, inner @ .., close] if open.text == "(" && close.text == ")" => {
            Ok((word.text, Some(inner)))
        }
        _ => Err(format!("malformed effect term `{text}`")),
    }
}

fn index(name: &str, inner: Option<&[Token<'_>]>) -> Result<Value, String> {
    let text = inner.map(info::text).unwrap_or_default();
    let n: u8 = text
        .parse()
        .map_err(|_| format!("{name}(N) needs an argument index, got `{name}({text})`"))?;
    Ok(Value::Number(n.to_string()))
}

/// Builds the slot outermost-first, so steps come out innermost-first.
fn slot(ts: &[Token<'_>]) -> Result<Value, String> {
    let (name, inner) = call(ts)?;
    let (root, steps) = match (name, inner) {
        ("this", None) => (Value::String("this".into()), Vec::new()),
        ("result", None) => (Value::String("result".into()), Vec::new()),
        ("arg" | "rest" | "returned", Some(_)) => {
            (tagged(name, index(name, inner)?), Vec::new())
        }
        ("elems", Some(inner)) => {
            let Value::Object(mut base) = slot(inner)? else { unreachable!() };
            let Some(Value::Array(mut steps)) = base.remove("steps") else { unreachable!() };
            steps.push(Value::String("elems".into()));
            (base.remove("root").unwrap(), steps)
        }
        ("field", Some(inner)) => {
            let parts = info::split(inner, ",");
            let [of, key] = parts.as_slice() else {
                return Err("field(slot, key) takes a slot and a key".into());
            };
            let key = match key {
                [t] => t.string.clone().unwrap_or_else(|| t.text.to_owned()),
                _ => return Err(format!("field key must be one word or string, got `{}`", info::text(key))),
            };
            if key.is_empty() {
                return Err("field key is empty".into());
            }
            let Value::Object(mut base) = slot(of)? else { unreachable!() };
            let Some(Value::Array(mut steps)) = base.remove("steps") else { unreachable!() };
            steps.push(tagged("field", Value::String(key)));
            (base.remove("root").unwrap(), steps)
        }
        _ => {
            return Err(format!(
                "unknown effect slot `{}` (this, result, arg(N), rest(N), returned(N), elems(slot), field(slot, key))",
                info::text(ts)
            ))
        }
    };
    Ok(object(vec![("root", root), ("steps", Value::Array(steps))]))
}

fn slots(ts: &[Token<'_>]) -> Result<Vec<Value>, String> {
    if ts.is_empty() {
        return Ok(Vec::new());
    }
    info::split(ts, ",").into_iter().map(slot).collect()
}

fn bracketed<'a, 's>(what: &str, ts: &'a [Token<'s>]) -> Result<&'a [Token<'s>], String> {
    match ts {
        [open, inner @ .., close] if open.text == "[" && close.text == "]" => Ok(inner),
        _ => Err(format!("{what} must be a [list]")),
    }
}

fn keyword<'a, 's>(key: &str, ts: &'a [Token<'s>]) -> Result<&'a [Token<'s>], String> {
    match ts {
        [k, eq, rest @ ..] if k.text == key && eq.text == "=" && !rest.is_empty() => Ok(rest),
        _ => Err(format!("expected `{key} = ...`, got `{}`", info::text(ts))),
    }
}

fn operand(ts: &[Token<'_>]) -> Result<Value, String> {
    let members = info::split(ts, "|");
    if members.len() > 1 {
        let members = members
            .into_iter()
            .map(value)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(tagged("union", Value::Array(members)));
    }
    value(ts)
}

fn value(ts: &[Token<'_>]) -> Result<Value, String> {
    let (name, inner) = call(ts)?;
    match (name, inner) {
        ("prim", Some(inner)) => {
            let kinds = info::split(inner, "|")
                .into_iter()
                .map(|kind| match info::text(kind).as_str() {
                    k @ ("undefined" | "null" | "bool" | "num" | "str" | "bigint" | "symbol") => {
                        Ok(Value::String(k.into()))
                    }
                    k => Err(format!(
                        "unknown primitive kind `{k}` (undefined, null, bool, num, str, bigint, symbol)"
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if kinds.is_empty() {
                return Err("prim() names no primitive kind".into());
            }
            Ok(tagged("prim", Value::Array(kinds)))
        }
        ("fresh", Some(inner)) => {
            let parts = info::split(inner, ",");
            let (proto, elems) = match parts.as_slice() {
                [proto] => (proto, Vec::new()),
                [proto, elems] => (
                    proto,
                    slots(bracketed("fresh elems", keyword("elems", elems)?)?)?,
                ),
                _ => {
                    return Err(
                        "fresh(Proto, elems = [slot, ...]) takes a prototype and elements".into(),
                    )
                }
            };
            Ok(tagged(
                "fresh",
                object(vec![
                    ("proto", Value::String(super::facts::path(proto)?)),
                    ("elems", Value::Array(elems)),
                ]),
            ))
        }
        ("species", Some(inner)) => {
            let parts = info::split(inner, ",");
            let usage = "species(slot, args = [operand, ...], elems = [slot, ...])";
            let [of, args, rest @ ..] = parts.as_slice() else {
                return Err(format!("{usage} takes a slot, args and elems"));
            };
            let args = info::split(bracketed("species args", keyword("args", args)?)?, ",")
                .into_iter()
                .map(operand)
                .collect::<Result<Vec<_>, _>>()?;
            let elems = match rest {
                [] => Vec::new(),
                [elems] => slots(bracketed("species elems", keyword("elems", elems)?)?)?,
                _ => return Err(format!("{usage} takes a slot, args and elems")),
            };
            Ok(tagged(
                "species",
                object(vec![
                    ("of", slot(of)?),
                    ("args", Value::Array(args)),
                    ("elems", Value::Array(elems)),
                ]),
            ))
        }
        _ => Ok(tagged("slot", slot(ts)?)),
    }
}

fn effect(ts: &[Token<'_>]) -> Result<Value, String> {
    let (verb, inner) = call(ts)?;
    let inner = || inner.ok_or_else(|| format!("effect `{verb}` needs (arguments)"));
    Ok(match verb {
        "open" if ts.len() == 1 => Value::String("open".into()),
        "reads" => {
            let read = slots(inner()?)?;
            if read.is_empty() {
                return Err("reads() names no slot".into());
            }
            tagged("reads", Value::Array(read))
        }
        "coerce" => {
            let parts = info::split(inner()?, ",");
            let [of, protocol] = parts.as_slice() else {
                return Err("coerce(slot, Protocol) takes a slot and a protocol".into());
            };
            let protocol = match info::text(protocol).as_str() {
                "ToPrimitive" => "to_primitive",
                "Iterate" => "iterate",
                "IterNext" => "iter_next",
                "IterResult" => "iter_result",
                "Json" => "json",
                "HasInstance" => "has_instance",
                "OwnKeys" => "own_keys",
                "Any" => return Err("coerce(.., Any) is not declarable: name the protocol".into()),
                p => {
                    return Err(format!(
                        "unknown protocol `{p}` (ToPrimitive, Iterate, IterNext, IterResult, Json, HasInstance, OwnKeys)"
                    ))
                }
            };
            tagged(
                "coerce",
                object(vec![
                    ("slot", slot(of)?),
                    ("protocol", Value::String(protocol.into())),
                ]),
            )
        }
        "calls" => {
            let parts = info::split(inner()?, ",");
            let [callee, this, args] = parts.as_slice() else {
                return Err(
                    "calls(slot, this = operand, args = [operand, ...]) takes three parts".into(),
                );
            };
            let args = info::split(bracketed("calls args", keyword("args", args)?)?, ",")
                .into_iter()
                .map(operand)
                .collect::<Result<Vec<_>, _>>()?;
            tagged(
                "calls",
                object(vec![
                    ("callee", slot(callee)?),
                    ("this", operand(keyword("this", this)?)?),
                    ("args", Value::Array(args)),
                ]),
            )
        }
        "stores" => {
            let inner = inner()?;
            if let [word] = inner {
                if word.text == "fresh" {
                    // The body stores only into objects it creates itself.
                    return Ok(Value::String("stores_fresh".into()));
                }
            }
            let arrow = inner
                .windows(2)
                .position(|w| w[0].text == "-" && w[1].text == ">")
                .ok_or("stores(src -> dst) needs `->`")?;
            tagged(
                "stores",
                object(vec![
                    ("src", slot(&inner[..arrow])?),
                    ("dst", slot(&inner[arrow + 2..])?),
                ]),
            )
        }
        "returns" => tagged("returns", operand(inner()?)?),
        _ => {
            return Err(format!(
                "unknown effect `{}` (reads, coerce, calls, stores, returns, open)",
                info::text(ts)
            ))
        }
    })
}

/// The value of `effects = { ... }`: the list of effects, in source order.
pub(super) fn parse(ts: &[Token<'_>]) -> Result<Value, String> {
    let inner = match ts {
        [open, inner @ .., close] if open.text == "{" && close.text == "}" => inner,
        _ => return Err("effects must be a { verb(..); ... } block".into()),
    };
    let effects = info::split(inner, ";")
        .into_iter()
        .filter(|e| !e.is_empty())
        .map(|e| effect(e).map_err(|err| format!("effects: {err}")))
        .collect::<Result<Vec<_>, _>>()?;
    if effects.is_empty() {
        return Err(
            "effects: empty block (omit `effects` for unknown code, or write `open`)".into(),
        );
    }
    let open = effects
        .iter()
        .filter(|e| matches!(e, Value::String(s) if s == "open"))
        .count();
    if open != 0 && effects.len() > 1 {
        return Err("effects: `open` cannot be combined with other effects".into());
    }
    Ok(Value::Array(effects))
}

/// Text form, for tests and tools: `{ reads(this); returns(prim(num)) }` -> JSON.
pub fn parse_text(text: &str) -> Result<String, String> {
    let ts = super::tokens(text)?;
    Ok(parse(&ts)?.encode())
}

#[cfg(test)]
mod tests {
    use super::parse_text;

    #[test]
    fn each_verb_parses_to_the_schema_form() {
        assert_eq!(
            parse_text("{ reads(this, elems(arg(0))) }").unwrap(),
            r#"[{"reads":[{"root":"this","steps":[]},{"root":{"arg":0},"steps":["elems"]}]}]"#
        );
        assert_eq!(
            parse_text("{ coerce(elems(this), ToPrimitive) }").unwrap(),
            r#"[{"coerce":{"protocol":"to_primitive","slot":{"root":"this","steps":["elems"]}}}]"#
        );
        assert_eq!(
            parse_text("{ calls(arg(0), this = arg(1), args = [elems(this), prim(num), this]) }")
                .unwrap(),
            r#"[{"calls":{"args":[{"slot":{"root":"this","steps":["elems"]}},{"prim":["num"]},{"slot":{"root":"this","steps":[]}}],"callee":{"root":{"arg":0},"steps":[]},"this":{"slot":{"root":{"arg":1},"steps":[]}}}}]"#
        );
        assert_eq!(
            parse_text("{ stores(rest(0) -> elems(this)); }").unwrap(),
            r#"[{"stores":{"dst":{"root":"this","steps":["elems"]},"src":{"root":{"rest":0},"steps":[]}}}]"#
        );
        assert_eq!(
            parse_text("{ returns(prim(num | undefined)); returns(fresh(Array.prototype, elems = [elems(this)])); returns(field(this, \"k\")) }").unwrap(),
            r#"[{"returns":{"prim":["num","undefined"]}},{"returns":{"fresh":{"elems":[{"root":"this","steps":["elems"]}],"proto":"Array.prototype"}}},{"returns":{"slot":{"root":"this","steps":[{"field":"k"}]}}}]"#
        );
        assert_eq!(parse_text("{ open }").unwrap(), r#"["open"]"#);
        assert_eq!(
            parse_text("{ reads(this); stores(fresh) }").unwrap(),
            r#"[{"reads":[{"root":"this","steps":[]}]},"stores_fresh"]"#
        );
        assert_eq!(
            parse_text("{ calls(arg(0), this = arg(1), args = [elems(this), prim(num) | elems(this)]); returns(fresh(Array.prototype, elems = [returned(0)])) }").unwrap(),
            r#"[{"calls":{"args":[{"slot":{"root":"this","steps":["elems"]}},{"union":[{"prim":["num"]},{"slot":{"root":"this","steps":["elems"]}}]}],"callee":{"root":{"arg":0},"steps":[]},"this":{"slot":{"root":{"arg":1},"steps":[]}}}},{"returns":{"fresh":{"elems":[{"root":{"returned":0},"steps":[]}],"proto":"Array.prototype"}}}]"#
        );
        assert_eq!(
            parse_text("{ returns(species(this, args = [prim(num)], elems = [elems(this)])) }")
                .unwrap(),
            r#"[{"returns":{"species":{"args":[{"prim":["num"]}],"elems":[{"root":"this","steps":["elems"]}],"of":{"root":"this","steps":[]}}}}]"#
        );
    }

    #[test]
    fn malformed_declarations_name_what_is_wrong() {
        for (text, needle) in [
            ("{ writes(this) }", "unknown effect `writes(this)`"),
            ("{ reads() }", "reads() names no slot"),
            ("{ reads(self) }", "unknown effect slot `self`"),
            ("{ reads(arg(x)) }", "needs an argument index"),
            ("{ coerce(this, Any) }", "not declarable"),
            ("{ coerce(this, ValueOf) }", "unknown protocol `ValueOf`"),
            ("{ stores(arg(0), this) }", "needs `->`"),
            ("{ stores(fresh(Array.prototype)) }", "needs `->`"),
            ("{ returns(species(this)) }", "takes a slot, args and elems"),
            (
                "{ returns(species(this, elems = [this])) }",
                "expected `args = ...`",
            ),
            ("{ reads(returned(x)) }", "needs an argument index"),
            ("{ calls(arg(0), arg(1)) }", "takes three parts"),
            (
                "{ calls(arg(0), this = undefined, args = []) }",
                "unknown effect slot `undefined`",
            ),
            ("{ returns(prim(int)) }", "unknown primitive kind `int`"),
            ("{ open; reads(this) }", "`open` cannot be combined"),
            ("{ }", "empty block"),
            ("reads(this)", "must be a { verb(..); ... } block"),
        ] {
            let err = parse_text(text).expect_err(text);
            assert!(err.contains(needle), "{text}: {err}");
        }
    }
}
