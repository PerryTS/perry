//! Build-only projection of the runtime_layout! declarations. The runtime
//! macro and this reader consume the same keys, slot map and scalar facts.
use crate::realm_json::Value;
use crate::realm_source::{rust_tokens as tokens, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn object(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn number(n: u64) -> Value {
    crate::realm_json::parse(&n.to_string()).expect("integer JSON")
}
fn string(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
fn matching(ts: &[Token<'_>], at: usize) -> Result<usize, String> {
    let close = match ts[at].text {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return Err("expected opening delimiter".into()),
    };
    let mut i = at + 1;
    while i < ts.len() {
        if ts[i].text == close {
            return Ok(i);
        }
        if matches!(ts[i].text, "(" | "[" | "{") {
            i = matching(ts, i)?;
        }
        i += 1;
    }
    Err("unterminated layout declaration".into())
}
fn split<'a, 's>(ts: &'a [Token<'s>]) -> Result<Vec<&'a [Token<'s>]>, String> {
    let mut parts = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < ts.len() {
        if matches!(ts[i].text, "(" | "[" | "{") {
            i = matching(ts, i)?;
        } else if ts[i].text == "," {
            parts.push(&ts[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    if start < ts.len() {
        parts.push(&ts[start..]);
    }
    Ok(parts)
}
fn text(ts: &[Token<'_>]) -> String {
    ts.iter().map(|t| t.text).collect::<Vec<_>>().join(" ")
}
fn literal(ts: &[Token<'_>]) -> Result<String, String> {
    if ts.len() != 1 {
        return Err("layout spelling must be a string literal".into());
    }
    ts[0]
        .string
        .clone()
        .ok_or_else(|| "layout spelling must be a string literal".into())
}
fn fields<'a, 's>(ts: &'a [Token<'s>]) -> Result<BTreeMap<&'s str, &'a [Token<'s>]>, String> {
    split(ts)?
        .into_iter()
        .map(|part| {
            if part.len() < 3 || part[1].text != ":" {
                return Err("invalid atom initializer".into());
            }
            Ok((part[0].text, &part[2..]))
        })
        .collect()
}

fn operator(ts: &[Token<'_>], op: &str) -> Result<Option<usize>, String> {
    let mut i = 0;
    while i < ts.len() {
        if matches!(ts[i].text, "(" | "[" | "{") {
            i = matching(ts, i)?;
        } else if ts[i].text == op {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}

fn target(ts: &[Token<'_>], mut at: usize) -> Result<&'static str, String> {
    let mut result = "any";
    while at > 1 && ts[at - 1].text == "]" {
        let begin = (1..at - 1)
            .rev()
            .find(|&i| ts[i].text == "[" && ts[i - 1].text == "#")
            .ok_or("layout attribute lacks opening bracket")?;
        if matching(ts, begin)? != at - 1 {
            return Err("invalid layout attribute".into());
        }
        let attr = &ts[begin + 1..at - 1];
        if attr.first().is_some_and(|t| t.text == "cfg") {
            if result != "any" {
                return Err("multiple layout cfg predicates need an explicit target model".into());
            }
            result = match text(attr).as_str() {
                "cfg ( windows )" => "windows",
                "cfg ( not ( windows ) )" => "non_windows",
                other => return Err(format!("unsupported layout target predicate {other}")),
            };
        }
        at = begin - 1;
    }
    Ok(result)
}

fn variants(declaration: &mut Value) -> &mut Vec<Value> {
    let Value::Object(d) = declaration else {
        unreachable!()
    };
    let Some(Value::Object(storage)) = d.get_mut("storage") else {
        unreachable!()
    };
    let Some(Value::Array(variants)) = storage.get_mut("variants") else {
        unreachable!()
    };
    variants
}

#[derive(Default)]
struct Constants(BTreeMap<String, BTreeSet<String>>);
impl Constants {
    fn read(&mut self, ts: &[Token<'_>]) {
        let mut scopes = Vec::new();
        for i in 0..ts.len().saturating_sub(2) {
            if ts[i].text == "mod" && ts[i + 2].text == "{" {
                if let Ok(end) = matching(ts, i + 2) {
                    scopes.push((i + 2, end, ts[i + 1].text));
                }
            }
        }
        for i in 0..ts.len().saturating_sub(3) {
            if ts[i].text != "const" || ts[i + 2].text != ":" || ts[i + 1].text == "_" {
                continue;
            }
            let Some(eq) = (i + 3..ts.len()).find(|&j| matches!(ts[j].text, "=" | ";")) else {
                continue;
            };
            if ts[eq].text != "=" {
                continue;
            }
            let Some(end) = (eq + 1..ts.len()).find(|&j| ts[j].text == ";") else {
                continue;
            };
            let expression = text(&ts[eq + 1..end]);
            self.0
                .entry(ts[i + 1].text.into())
                .or_default()
                .insert(expression.clone());
            let mut path = scopes
                .iter()
                .filter(|(begin, end, _)| *begin < i && i < *end)
                .map(|(_, _, name)| *name)
                .collect::<Vec<_>>();
            if !path.is_empty() {
                path.push(ts[i + 1].text);
                self.0
                    .entry(path.join("::"))
                    .or_default()
                    .insert(expression);
            }
        }
    }
    fn value(&self, ts: &[Token<'_>]) -> Result<u64, String> {
        self.eval(ts, 0)
    }
    fn eval(&self, ts: &[Token<'_>], depth: usize) -> Result<u64, String> {
        if depth > 16 || ts.is_empty() {
            return Err("cyclic or empty layout constant".into());
        }
        if let Some(i) = operator(ts, "|")? {
            return Ok(self.eval(&ts[..i], depth + 1)? | self.eval(&ts[i + 1..], depth + 1)?);
        }
        if let Some(i) = operator(ts, "<<")? {
            return self
                .eval(&ts[..i], depth + 1)?
                .checked_shl(
                    u32::try_from(self.eval(&ts[i + 1..], depth + 1)?)
                        .map_err(|_| "layout shift overflow")?,
                )
                .ok_or_else(|| "layout shift overflow".into());
        }
        if ts[0].text == "(" && matching(ts, 0)? == ts.len() - 1 {
            return self.eval(&ts[1..ts.len() - 1], depth + 1);
        }
        let last = ts.last().unwrap().text;
        if ts.len() == 1 && last.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            let s = last.replace('_', "");
            let s = ["u64", "u32", "u16", "u8", "usize"]
                .iter()
                .find_map(|suffix| s.strip_suffix(suffix))
                .unwrap_or(&s);
            return if let Some(s) = s.strip_prefix("0x") {
                u64::from_str_radix(s, 16)
            } else {
                s.parse()
            }
            .map_err(|_| format!("invalid layout integer {last}"));
        }
        if ts.iter().any(|t| {
            !t.text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && t.text != "::"
        }) {
            return Err(format!("unsupported layout constant {}", text(ts)));
        }
        let qualified = ts.iter().map(|t| t.text).collect::<String>();
        let expressions = self
            .0
            .get(&qualified)
            .or_else(|| self.0.get(last))
            .ok_or_else(|| format!("unknown layout constant {}", text(ts)))?;
        let mut values = BTreeSet::new();
        for expression in expressions {
            values.insert(self.eval(&tokens(expression)?, depth + 1)?);
        }
        if values.len() != 1 {
            return Err(format!("ambiguous layout constant {last}"));
        }
        Ok(*values.first().unwrap())
    }
}

fn insert_atom(atoms: &mut BTreeMap<String, Value>, name: &str, atom: Value) -> Result<(), String> {
    if let Some(old) = atoms.insert(name.into(), atom.clone()) {
        if old != atom {
            return Err(format!("ambiguous runtime layout atom {name}"));
        }
    }
    Ok(())
}
fn atom(kind: u8, spelling: Option<String>, index: u64) -> Value {
    object([
        ("kind", number(kind.into())),
        ("text", spelling.map_or(Value::Null, string)),
        ("index", number(index)),
    ])
}

fn macro_defaults(
    ts: &[Token<'_>],
    constants: &Constants,
) -> Result<BTreeMap<String, Value>, String> {
    let mut found = None;
    for i in 1..ts.len().saturating_sub(1) {
        if ts[i - 1].text != "=" || ts[i].text != "PerryLayoutDesc" || ts[i + 1].text != "{" {
            continue;
        }
        let end = matching(ts, i + 1)?;
        let initializer = fields(&ts[i + 2..end])?;
        if text(
            initializer
                .get("methods")
                .ok_or("runtime layout lacks method facts")?,
        ) != "std :: ptr :: null ( )"
        {
            return Err("runtime layout method bodies need source-owned image facts".into());
        }
        let mut defaults = BTreeMap::new();
        for field in ["integrity", "rep", "class_id", "parent_class_id"] {
            defaults.insert(
                field.into(),
                number(
                    constants.value(
                        initializer
                            .get(field)
                            .ok_or_else(|| format!("runtime_layout! lacks {field}"))?,
                    )?,
                ),
            );
        }
        if found.replace(defaults).is_some() {
            return Err("multiple runtime layout templates".into());
        }
    }
    found.ok_or_else(|| "runtime_layout! descriptor template was not found".into())
}

fn symbol_registrations(
    parsed: &[Vec<Token<'_>>],
) -> Result<BTreeMap<String, (u64, bool)>, String> {
    let mut result = BTreeMap::new();
    for ts in parsed {
        for i in 0..ts.len().saturating_sub(1) {
            if ts[i].text != "static" || ts[i + 1].text != "RUNTIME_SYMBOL_ATOMS" {
                continue;
            }
            let eq = (i + 2..ts.len())
                .find(|&j| ts[j].text == "=")
                .ok_or("runtime symbols lack an initializer")?;
            if ts[eq + 1].text != "[" {
                return Err("runtime symbol bindings must be a literal array".into());
            }
            let end = matching(ts, eq + 1)?;
            for (index, entry) in split(&ts[eq + 2..end])?.into_iter().enumerate() {
                if entry.first().is_none_or(|t| t.text != "(")
                    || matching(entry, 0)? != entry.len() - 1
                {
                    return Err("invalid runtime symbol binding".into());
                }
                let pair = split(&entry[1..entry.len() - 1])?;
                if pair.len() != 2
                    || pair[0].len() != 2
                    || pair[0][0].text != "&"
                    || pair[1].len() != 1
                {
                    return Err("invalid runtime symbol binding".into());
                }
                let registered = match pair[1][0].text {
                    "true" => true,
                    "false" => false,
                    _ => return Err("runtime symbol registration must be literal".into()),
                };
                if result
                    .insert(pair[0][1].text.into(), (index as u64, registered))
                    .is_some()
                {
                    return Err("duplicate runtime symbol binding".into());
                }
            }
        }
    }
    Ok(result)
}

/// Read source, never a runtime archive or a sampled realm. The fixed ABI
/// names validate coverage; they are not a second definition of layout facts.
pub fn read(runtime_src: &Path) -> Result<Vec<Value>, String> {
    let dir = runtime_src.join("object/runtime_layouts");
    let mut paths = vec![runtime_src.join("object/runtime_layouts.rs")];
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "rs")
            && !path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .ends_with("_tests")
        {
            paths.push(path);
        }
    }
    paths.sort();
    let sources = paths
        .iter()
        .map(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let parsed = sources
        .iter()
        .map(|s| tokens(s))
        .collect::<Result<Vec<_>, _>>()?;
    let mut constants = Constants::default();
    for ts in &parsed {
        constants.read(ts);
    }
    let abi = runtime_src
        .parent()
        .and_then(Path::parent)
        .ok_or("missing crates directory")?
        .join("perry-abi/src");
    for path in [
        abi.join("lib.rs"),
        abi.join("layouts.rs"),
        abi.join("intrinsic_serials.rs"),
        runtime_src.join("object/key_attrs.rs"),
        runtime_src.join("object/layout_desc.rs"),
    ] {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        constants.read(&tokens(&source)?);
    }
    let template = paths
        .iter()
        .position(|path| *path == runtime_src.join("object/runtime_layouts.rs"))
        .unwrap();
    let defaults = macro_defaults(&parsed[template], &constants)?;
    let registered_symbols = symbol_registrations(&parsed)?;
    let mut atoms = BTreeMap::new();
    let mut families = BTreeMap::new();
    for ts in &parsed {
        let mut i = 0;
        while i < ts.len() {
            if i + 2 < ts.len() && ts[i + 1].text == "!" && ts[i + 2].text == "(" {
                let end = matching(ts, i + 2)?;
                let args = split(&ts[i + 3..end])?;
                match ts[i].text {
                    "runtime_atom"
                        if args.len() == 3 && args[0].first().is_some_and(|t| t.text != "$") =>
                    {
                        let name = args[0].last().ok_or("atom lacks name")?.text;
                        insert_atom(
                            &mut atoms,
                            name,
                            atom(perry_abi::ATOM_KIND_STRING, Some(literal(args[1])?), 0),
                        )?;
                    }
                    "atom" if args.len() == 6 && args[0].len() == 1 && args[1].len() == 1 => {
                        insert_atom(
                            &mut atoms,
                            args[1][0].text,
                            atom(perry_abi::ATOM_KIND_STRING, Some(literal(args[2])?), 0),
                        )?;
                    }
                    "runtime_symbol_atom" if args.len() == 3 && args[0].len() == 1 => {
                        let name = args[0][0].text;
                        let index = constants.value(args[2])?;
                        let &(declared_index, registered) =
                            registered_symbols.get(name).ok_or_else(|| {
                                format!("runtime symbol {name} has no birth registration fact")
                            })?;
                        if index != declared_index {
                            return Err(format!(
                                "runtime symbol {name} has inconsistent index facts"
                            ));
                        }
                        let mut value = atom(
                            perry_abi::ATOM_KIND_PROGRAM_SYMBOL,
                            Some(literal(args[1])?),
                            index,
                        );
                        let Value::Object(fields) = &mut value else {
                            unreachable!()
                        };
                        fields.insert("registered".into(), Value::Bool(registered));
                        insert_atom(&mut atoms, name, value)?;
                    }
                    "runtime_prototype_family" => {
                        for part in args {
                            if part.len() == 3 && part[1].text == "=" {
                                let serial = constants.value(&part[2..])?;
                                if families.insert(part[0].text.to_string(), serial).is_some() {
                                    return Err("duplicate runtime prototype family".into());
                                }
                            }
                        }
                    }
                    _ => {}
                }
                // Do not skip arbitrary macros: declarations may be nested.
            }
            if ts[i].text == "static"
                && i + 7 < ts.len()
                && ts[i + 2].text == ":"
                && ts[i + 3].text == "StaticAtom"
                && ts[i + 4].text == "="
                && ts[i + 5].text == "StaticAtom"
                && ts[i + 6].text == "{"
            {
                let end = matching(ts, i + 6)?;
                let f = fields(&ts[i + 7..end])?;
                let kind = constants.value(f.get("kind").ok_or("atom lacks kind")?)?;
                let index = constants.value(f.get("index").ok_or("atom lacks index")?)?;
                let spelling = if kind == perry_abi::ATOM_KIND_STRING.into() {
                    Some(
                        f.get("hash")
                            .ok_or("atom lacks hash")?
                            .iter()
                            .find_map(|t| t.string.clone())
                            .ok_or_else(|| {
                                format!("string atom {} lacks a literal hash input", ts[i + 1].text)
                            })?,
                    )
                } else {
                    None
                };
                insert_atom(
                    &mut atoms,
                    ts[i + 1].text,
                    atom(
                        u8::try_from(kind).map_err(|_| "atom kind overflow")?,
                        spelling,
                        index,
                    ),
                )?;
                i = end;
            }
            i += 1;
        }
    }
    let expected: BTreeSet<_> = perry_abi::NATIVE_LAYOUT_DESCRIPTORS
        .iter()
        .copied()
        .collect();
    let mut layouts: BTreeMap<String, Value> = BTreeMap::new();
    for ((path, source), ts) in paths.iter().zip(&sources).zip(&parsed) {
        for i in 0..ts.len().saturating_sub(3) {
            if ts[i].text != "runtime_layout" || ts[i + 1].text != "!" || ts[i + 2].text != "(" {
                continue;
            }
            let end = matching(ts, i + 2)?;
            let mut args = split(&ts[i + 3..end])?;
            if args
                .first()
                .is_none_or(|a| a.len() != 1 || !expected.contains(a[0].text))
            {
                continue;
            }
            let name = args.remove(0)[0].text;
            let kind = if args
                .first()
                .is_some_and(|a| a.len() >= 3 && a[0].text == "kind" && a[1].text == "=")
            {
                constants.value(&args.remove(0)[2..])?
            } else {
                0
            };
            let owners = if args.len() == 5 {
                let clause = args.pop().unwrap();
                if clause.len() < 4
                    || clause[0].text != "realm"
                    || clause[1].text != "="
                    || clause[2].text != "["
                    || matching(clause, 2)? != clause.len() - 1
                {
                    return Err(format!("invalid realm owners for layout {name}"));
                }
                split(&clause[3..clause.len() - 1])?
                    .into_iter()
                    .map(literal)
                    .map(|v| v.map(string))
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            if args.len() != 4 {
                return Err(format!("unexpected runtime_layout grammar for {name}"));
            }
            let family = constants.value(args[0])?;
            let variant = constants.value(args[1])?;
            let proto_name = args[2].last().ok_or("empty layout prototype")?.text;
            let prototype = match proto_name {
                "LAYOUT_PROTO_OBJECT" => object([("kind", string("object"))]),
                "LAYOUT_PROTO_NULL" => object([("kind", string("null"))]),
                _ => object([
                    ("kind", string("serial")),
                    (
                        "serial",
                        number(*families.get(proto_name).ok_or_else(|| {
                            format!("unknown runtime prototype family {proto_name}")
                        })?),
                    ),
                ]),
            };
            let key_tokens = args[3];
            if key_tokens.first().is_none_or(|t| t.text != "[")
                || matching(key_tokens, 0)? != key_tokens.len() - 1
            {
                return Err(format!("invalid key declaration for {name}"));
            }
            let mut keys = Vec::new();
            for (position, key) in split(&key_tokens[1..key_tokens.len() - 1])?
                .into_iter()
                .enumerate()
            {
                let identifier = key.first().ok_or("empty layout key")?.text;
                let value = atoms
                    .get(identifier)
                    .ok_or_else(|| format!("unknown layout atom {identifier} in {name}"))?
                    .clone();
                let attr_at = key.iter().position(|t| t.text == "=>");
                let slot_at = key.iter().position(|t| t.text == "@");
                let attributes = attr_at.map_or(Ok(0), |at| constants.value(&key[at + 1..]))?;
                let slot = slot_at.map_or(Ok(position as u64), |at| {
                    constants.value(&key[at + 1..attr_at.unwrap_or(key.len())])
                })?;
                if attributes > 255 || slot > u32::MAX as u64 {
                    return Err(format!("layout key overflow in {name}"));
                }
                keys.push(object([
                    ("atom", value),
                    ("slot", number(slot)),
                    ("attributes", number(attributes)),
                ]));
            }
            let relative = path
                .strip_prefix(runtime_src.parent().unwrap().parent().unwrap())
                .map_err(|e| e.to_string())?;
            let line = source[..ts[i].start]
                .bytes()
                .filter(|b| *b == b'\n')
                .count() as u64
                + 1;
            let mut facts = object([
                ("target", string(target(ts, i)?)),
                ("family", number(family)),
                ("variant", number(variant)),
                ("object_kind", number(kind)),
                ("prototype", prototype),
                ("keys", Value::Array(keys)),
                ("source", string(relative.to_string_lossy())),
                ("line", number(line)),
            ]);
            let Value::Object(fields) = &mut facts else {
                unreachable!()
            };
            fields.extend(defaults.clone());
            if let Some(existing) = layouts.get_mut(name) {
                if existing
                    .get("storage")
                    .and_then(|s| s.get("owners"))
                    .and_then(Value::array)
                    != Some(owners.as_slice())
                {
                    return Err(format!("layout {name} has inconsistent target ownership"));
                }
                variants(existing).push(facts);
            } else {
                let storage = object([
                    ("kind", string("runtime_layout")),
                    ("symbol", string(format!("perry_runtime_layout_{name}"))),
                    ("owners", Value::Array(owners)),
                    ("variants", Value::Array(vec![facts])),
                ]);
                layouts.insert(
                    name.to_string(),
                    object([
                        ("name", string(format!("%Layout:{name}%"))),
                        ("kind", string("exotic")),
                        ("storage", storage),
                        ("source", string(relative.to_string_lossy())),
                        ("line", number(line)),
                    ]),
                );
            }
        }
    }
    for name in expected {
        if !layouts.contains_key(name) {
            return Err(format!("missing source-owned runtime layout {name}"));
        }
    }
    for (name, declaration) in &mut layouts {
        let choices = variants(declaration);
        let mut targets = choices
            .iter()
            .map(|v| v.get("target").and_then(Value::string).unwrap())
            .collect::<Vec<_>>();
        targets.sort();
        if targets != ["any"] && targets != ["non_windows", "windows"] {
            return Err(format!(
                "overlapping or incomplete target variants for layout {name}"
            ));
        }
    }
    Ok(layouts.into_values().collect())
}

/// Add declared layout dependencies to the owning births before R is closed.
/// Missing owners fail the build; they never broaden a program to the catalog.
pub fn attach_births(declarations: &mut [Value]) -> Result<(), String> {
    let names = declarations
        .iter()
        .enumerate()
        .map(|(i, d)| {
            d.get("name")
                .and_then(Value::string)
                .map(|name| (name.to_string(), i))
                .ok_or("realm declaration lacks name")
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut edges = Vec::new();
    for declaration in declarations.iter() {
        let Some(storage) = declaration.get("storage") else {
            continue;
        };
        if storage.get("kind").and_then(Value::string) != Some("runtime_layout") {
            continue;
        }
        let name = declaration.get("name").and_then(Value::string).unwrap();
        for owner in storage
            .get("owners")
            .and_then(Value::array)
            .ok_or("layout lacks owner facts")?
        {
            let owner = owner.string().ok_or("layout owner is not a realm name")?;
            let index = *names
                .get(owner)
                .ok_or_else(|| format!("runtime layout {name} names undeclared birth {owner}"))?;
            edges.push((index, name.to_string()));
        }
    }
    for (index, layout) in edges {
        let Value::Object(declaration) = &mut declarations[index] else {
            unreachable!()
        };
        let Value::Array(births) = declaration
            .entry("births".into())
            .or_insert_with(|| Value::Array(Vec::new()))
        else {
            return Err("realm births are not an array".into());
        };
        if !births.iter().any(|v| v.string() == Some(&layout)) {
            births.push(string(layout));
        }
        births.sort_by(|a, b| a.string().cmp(&b.string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_runtime_layouts_have_one_complete_source_projection() {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-runtime/src");
        let layouts = read(&runtime).unwrap();
        assert_eq!(layouts.len(), perry_abi::NATIVE_LAYOUT_DESCRIPTORS.len());
        assert!(layouts.iter().any(
            |d| d.get("name").and_then(Value::string) == Some("%Layout:ARRAY_PROTOTYPE_NAMED%")
        ));
    }
    #[test]
    fn layout_owners_attach_only_declared_births_and_reject_missing_owners() {
        let mut declarations = crate::realm_json::parse(
            r#"[
            {"name":"Array", "births":["Array.prototype"]},
            {"name":"Array.prototype"},
            {"name":"RegExp"},
            {"name":"%Layout:ARRAY_NAMED%", "storage":{
                "kind":"runtime_layout", "owners":["Array"]}}
        ]"#,
        )
        .unwrap()
        .array()
        .unwrap()
        .to_vec();
        attach_births(&mut declarations).unwrap();
        let once = declarations.clone();
        attach_births(&mut declarations).unwrap();
        assert_eq!(declarations, once);
        let births = declarations[0].get("births").unwrap().array().unwrap();
        assert_eq!(
            births
                .iter()
                .map(|v| v.string().unwrap())
                .collect::<Vec<_>>(),
            ["%Layout:ARRAY_NAMED%", "Array.prototype"]
        );
        assert!(declarations[2].get("births").is_none());
        declarations.remove(0);
        assert!(attach_births(&mut declarations)
            .unwrap_err()
            .contains("undeclared birth Array"));
    }

    #[test]
    fn constants_resolve_source_aliases_and_refuse_ambiguity() {
        let mut c = Constants::default();
        c.read(&tokens("const A: u8 = 2; const B: u8 = A | 4;").unwrap());
        assert_eq!(c.value(&tokens("B").unwrap()).unwrap(), 6);
        assert_eq!(c.value(&tokens("(1 | 2) << 3").unwrap()).unwrap(), 24);
        c.read(&tokens("const A: u8 = 7;").unwrap());
        assert!(c.value(&tokens("B").unwrap()).is_err());
        assert!(c.value(&tokens("address()").unwrap()).is_err());
        c.read(
            &tokens("mod readable { const SLOT: u8 = 5; } mod writable { const SLOT: u8 = 6; }")
                .unwrap(),
        );
        assert_eq!(c.value(&tokens("readable::SLOT").unwrap()).unwrap(), 5);
        assert_eq!(c.value(&tokens("writable::SLOT").unwrap()).unwrap(), 6);
        assert!(c.value(&tokens("SLOT").unwrap()).is_err());
    }
}
