//! Read the ABI facts already declared by fn_info!; no reflected ABI records.
use super::{tokens, Token};
use crate::realm_json::Value;
use std::collections::BTreeMap;

pub(super) fn text(ts: &[Token<'_>]) -> String {
    ts.iter().map(|t| t.text).collect()
}

pub(super) fn split<'a, 's>(ts: &'a [Token<'s>], separator: &str) -> Vec<&'a [Token<'s>]> {
    let mut depth = 0;
    let mut start = 0;
    let mut result = Vec::new();
    for (i, token) in ts.iter().enumerate() {
        match token.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
        if depth == 0 && token.text == separator {
            result.push(&ts[start..i]);
            start = i + 1;
        }
    }
    if start < ts.len() {
        result.push(&ts[start..]);
    }
    result
}

pub(super) fn integer(expr: &str, constants: &BTreeMap<String, u64>) -> Result<u64, String> {
    let expr = expr.trim();
    // These are the integer operations used by the ABI's flag declarations.
    // Reject other Rust expressions instead of silently inventing a value.
    for operator in ["|", "&", "<<", ">>"] {
        if let Some((left, right)) = expr.rsplit_once(operator) {
            let a = integer(left, constants)?;
            let b = integer(right, constants)?;
            return Ok(match operator {
                "|" => a | b,
                "&" => a & b,
                "<<" if b < 64 => a << b,
                ">>" if b < 64 => a >> b,
                _ => return Err(format!("invalid ABI shift: {expr}")),
            });
        }
    }
    if let Some(inner) = expr.strip_prefix('!') {
        return Ok(!integer(inner, constants)?);
    }
    let leaf = expr.rsplit("::").next().unwrap();
    if let Some(value) = constants.get(leaf) {
        return Ok(*value);
    }
    let number = leaf.replace('_', "");
    if let Some(hex) = number.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        number.parse()
    }
    .map_err(|_| format!("not a constant ABI integer: {expr}"))
}

/// GC header facts: the runtime's gc/types.rs plus the shared ABI constants it
/// re-exports (each is defined once, in perry-abi).
pub(super) fn gc_constants() -> Result<BTreeMap<String, u64>, String> {
    constants(concat!(
        include_str!("../../perry-runtime/src/gc/types.rs"),
        "\n",
        include_str!("../../perry-abi/src/lib.rs"),
        "\n",
        include_str!("../../perry-abi/src/realm_cells.rs"),
    ))
}

pub(super) fn constants(source: &str) -> Result<BTreeMap<String, u64>, String> {
    let ts = tokens(source)?;
    let mut pending = Vec::new();
    for (i, token) in ts.iter().enumerate() {
        if token.text != "const" {
            continue;
        }
        let Some(name) = ts.get(i + 1) else { continue };
        if ts.get(i + 2).is_none_or(|t| t.text != ":")
            || ts
                .get(i + 3)
                .is_none_or(|t| !matches!(t.text, "u8" | "u16" | "u32" | "u64" | "usize"))
        {
            continue;
        }
        let Some(end) = ts[i + 2..].iter().position(|t| t.text == ";") else {
            continue;
        };
        let declaration = &ts[i + 2..i + 2 + end];
        if let Some(equals) = declaration.iter().position(|t| t.text == "=") {
            pending.push((name.text.to_owned(), text(&declaration[equals + 1..])));
        }
    }
    let mut values = BTreeMap::new();
    loop {
        let before = pending.len();
        pending.retain(|(name, expr)| {
            if let Ok(value) = integer(expr, &values) {
                values.insert(name.clone(), value);
                false
            } else {
                true
            }
        });
        if pending.len() == before {
            break;
        }
    }
    Ok(values)
}

#[derive(Clone, Debug)]
pub(super) struct Info {
    pub body: String,
    pub params: u64,
    pub rest_fixed: u64,
    pub flags: u64,
    pub length: u64,
    pub declared: u64,
    pub plain_params: u64,
}

impl Info {
    pub fn public_length(&self, constants: &BTreeMap<String, u64>) -> Result<u64, String> {
        let flag = |name: &str| {
            constants
                .get(name)
                .copied()
                .ok_or_else(|| format!("missing {name}"))
        };
        Ok(if self.flags & flag("FN_HAS_LENGTH")? != 0 {
            self.length
        } else if self.flags & flag("FN_HAS_DECLARED")? != 0 {
            self.declared
        } else if self.flags & flag("FN_REST_MASK")? != 0 {
            self.rest_fixed
        } else {
            self.params
        })
    }

    /// Bare closures have no installer metadata. Registry::closure_length
    /// consults explicit length, then rest arity, then declared arity; an
    /// unrecorded arity is exposed as zero, regardless of the physical ABI.
    pub fn bare_length(&self, constants: &BTreeMap<String, u64>) -> Result<u64, String> {
        let flag = |name| {
            constants
                .get(name)
                .copied()
                .ok_or_else(|| format!("missing {name}"))
        };
        Ok(if self.flags & flag("FN_HAS_LENGTH")? != 0 {
            self.length
        } else if self.flags & flag("FN_REST_MASK")? != 0 {
            self.rest_fixed
        } else if self.flags & flag("FN_HAS_DECLARED")? != 0 {
            self.declared
        } else {
            0
        })
    }

    pub fn facts(&self, source_symbol: String, native_method: bool) -> Value {
        let mut facts = BTreeMap::new();
        for (name, value) in [
            ("params", self.params),
            ("rest_fixed", self.rest_fixed),
            ("flags", self.flags),
            ("length", self.length),
            ("declared", self.declared),
            ("plain_params", self.plain_params),
        ] {
            facts.insert(name.into(), Value::Number(value.to_string()));
        }
        facts.insert("source_symbol".into(), Value::String(source_symbol));
        facts.insert("native_method".into(), Value::Bool(native_method));
        Value::Object(facts)
    }

    fn builder(
        &mut self,
        ts: &[Token<'_>],
        constants: &BTreeMap<String, u64>,
    ) -> Result<(), String> {
        let name = ts.first().ok_or("empty fn_info builder")?.text;
        if ts.get(1).is_none_or(|t| t.text != "(") || ts.last().is_none_or(|t| t.text != ")") {
            return Err(format!("malformed fn_info builder: {}", text(ts)));
        }
        let args = split(&ts[2..ts.len() - 1], ",")
            .iter()
            .map(|arg| integer(&text(arg), constants))
            .collect::<Result<Vec<_>, _>>()?;
        let flag = |name: &str| {
            constants
                .get(name)
                .copied()
                .ok_or_else(|| format!("missing {name}"))
        };
        let arg = |i: usize| {
            args.get(i)
                .copied()
                .ok_or_else(|| format!("missing {name} argument"))
        };
        match name {
            "with_flags" if args.len() == 1 => self.flags |= arg(0)?,
            "with_declared" if args.len() == 1 => {
                self.declared = arg(0)?;
                self.flags |= flag("FN_HAS_DECLARED")?;
            }
            "with_length" if args.len() == 1 => {
                self.length = arg(0)?;
                self.flags |= flag("FN_HAS_LENGTH")?;
            }
            "with_rest" if args.len() == 1 => {
                self.rest_fixed = arg(0)?;
                self.flags = (self.flags & !flag("FN_REST_MASK")?) | flag("FN_REST_USER")?;
            }
            "with_rest_kind" if args.len() == 2 => {
                self.rest_fixed = arg(0)?;
                self.flags = (self.flags & !flag("FN_REST_MASK")?) | arg(1)?;
            }
            "plain" if args.is_empty() => self.plain_params = self.params + 1,
            _ => return Err(format!("unsupported fn_info builder: {}", text(ts))),
        }
        if self.flags & flag("FN_REST_MASK")? != 0 {
            self.plain_params = 0;
        }
        Ok(())
    }
}

pub(super) fn metadata_binding(ts: &[Token<'_>]) -> bool {
    let clauses = split(ts, ";");
    clauses.len() == 3
        && clauses[2].len() == 4
        && clauses[2][0].text == "bind"
        && clauses[2][2].text == ","
        && [1, 3].into_iter().all(|i| {
            let name = clauses[2][i].text;
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.chars().next().unwrap().is_ascii_digit()
        })
}

pub(super) fn fn_info(ts: &[Token<'_>], constants: &BTreeMap<String, u64>) -> Result<Info, String> {
    let clauses = split(ts, ";");
    let head = split(clauses.first().ok_or("empty fn_info")?, ",");
    if head.len() != 2 || (clauses.len() > 2 && !metadata_binding(ts)) {
        return Err(format!("malformed fn_info: {}", text(ts)));
    }
    let mut body = head[0];
    // An owning site exporting its record names the body by its full path.
    while body
        .first()
        .is_some_and(|t| matches!(t.text, "reference" | "export"))
    {
        body = &body[1..];
    }
    let native = body.first().is_some_and(|t| t.text == "native_args");
    if body
        .first()
        .is_some_and(|t| matches!(t.text, "native_args" | "unwind" | "unwind_in_tests"))
    {
        body = &body[1..];
    }
    let arity = integer(&text(head[1]), constants)?;
    let mut info = Info {
        body: text(body),
        params: if native { 0 } else { arity },
        rest_fixed: if native { arity } else { 0 },
        flags: if native {
            *constants
                .get("FN_REST_NATIVE_ARGS")
                .ok_or("missing native args flag")?
        } else {
            0
        },
        length: 0,
        declared: 0,
        plain_params: 0,
    };
    if let Some(builders) = clauses.get(1) {
        for builder in split(builders, ",") {
            info.builder(builder, constants)?;
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn abi() -> BTreeMap<String, u64> {
        constants("pub const FN_HAS_LENGTH:u32=1<<3; pub const FN_HAS_DECLARED:u32=1<<11; pub const FN_BUILTIN:u32=1<<10; pub const FN_REST_USER:u32=1; pub const FN_REST_NATIVE_ARGS:u32=1<<16; pub const FN_REST_MASK:u32=FN_REST_USER|FN_REST_NATIVE_ARGS;").unwrap()
    }
    #[test]
    fn source_builders_determine_abi_and_visible_default_length() {
        let constants = abi();
        let source = tokens("unwind crate::atomics::add, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)").unwrap();
        let info = fn_info(&source, &constants).unwrap();
        assert_eq!(info.body, "crate::atomics::add");
        assert_eq!(info.params, 3);
        assert_eq!(info.flags, 3072);
        assert_eq!(info.public_length(&constants).unwrap(), 2);
        assert_eq!(
            info.facts("source:add".into(), false).get("declared"),
            Some(&Value::Number("2".into()))
        );
    }
    #[test]
    fn native_arguments_and_builder_order_preserve_runtime_rules() {
        let constants = abi();
        let source = tokens("native_args body, 3; with_length(1), plain()").unwrap();
        let info = fn_info(&source, &constants).unwrap();
        assert_eq!(info.params, 0);
        assert_eq!(info.rest_fixed, 3);
        assert_eq!(info.plain_params, 0);
        assert_eq!(info.public_length(&constants).unwrap(), 1);
        let source = tokens("body,2; plain(),with_rest(1)").unwrap();
        assert_eq!(fn_info(&source, &constants).unwrap().plain_params, 0);
    }
    #[test]
    fn unknown_expressions_and_builders_fail_closed() {
        let constants = abi();
        for source in [
            "body, dynamic_arity",
            "body,2; with_unknown(1)",
            "body,2; with_length()",
            "body,2; with_flags(MISSING)",
        ] {
            assert!(
                fn_info(&tokens(source).unwrap(), &constants).is_err(),
                "{source}"
            );
        }
    }
    #[test]
    fn bare_length_uses_recorded_arity_not_physical_parameters() {
        let constants = abi();
        let unrecorded = fn_info(&tokens("body,3").unwrap(), &constants).unwrap();
        assert_eq!(unrecorded.bare_length(&constants).unwrap(), 0);
        assert_eq!(unrecorded.public_length(&constants).unwrap(), 3);
        let rest = fn_info(
            &tokens("body,3;with_declared(3),with_rest(1)").unwrap(),
            &constants,
        )
        .unwrap();
        assert_eq!(rest.bare_length(&constants).unwrap(), 1);
    }
}
