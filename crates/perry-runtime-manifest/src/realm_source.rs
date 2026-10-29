//! Shared build producer. Both builds read declarations beside runtime bodies;
//! the compiler never loads or links runtime machine code to obtain these facts.
use crate::realm_json::{self as json, Value};
pub const REALM_MANIFEST_VERSION: u32 = 1;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
#[path = "realm_array.rs"]
mod array;
#[path = "realm_birth.rs"]
mod birth;
#[path = "realm_capture.rs"]
mod capture;
#[path = "realm_capture_call.rs"]
mod capture_call;
#[path = "realm_colors.rs"]
mod colors;
#[path = "realm_derive.rs"]
mod derive;
#[path = "realm_facts.rs"]
mod facts;
#[path = "realm_families.rs"]
mod families;
#[path = "realm_hash.rs"]
mod hash;
#[path = "realm_imports.rs"]
mod imports;
#[path = "realm_info.rs"]
mod info;
#[path = "realm_layout_birth.rs"]
mod layout_birth;
#[path = "realm_module_namespace.rs"]
mod module_namespace;
#[path = "realm_namespace.rs"]
mod namespace;
#[path = "realm_native.rs"]
mod native;
#[path = "realm_object.rs"]
mod object;
#[path = "realm_projection.rs"]
mod projection;
#[path = "realm_record.rs"]
mod record;
#[path = "realm_symbols.rs"]
mod symbols;
#[path = "realm_typed_info.rs"]
mod typed_info;

fn abi_constants() -> &'static std::collections::BTreeMap<String, u64> {
    static CONSTANTS: std::sync::OnceLock<std::collections::BTreeMap<String, u64>> =
        std::sync::OnceLock::new();
    CONSTANTS.get_or_init(|| {
        info::constants(include_str!("../../perry-abi/src/lib.rs"))
            .expect("read ABI source constants")
    })
}

fn closing_bracket(ts: &[Token<'_>], open: usize) -> Result<usize, String> {
    let mut depth = 0usize;
    for (i, t) in ts.iter().enumerate().skip(open) {
        match t.text {
            "[" => depth += 1,
            "]" => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    Err("unterminated bracket".into())
}

fn closing_paren(ts: &[Token<'_>], open: usize) -> Result<usize, String> {
    let mut depth = 0;
    for (i, token) in ts.iter().enumerate().skip(open) {
        if token.text == "(" {
            depth += 1;
        }
        if token.text == ")" {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
    }
    Err("unterminated realm/fn_info declaration".into())
}

#[derive(Debug)]
pub(super) struct Token<'a> {
    pub(super) text: &'a str,
    pub(super) start: usize,
    end: usize,
    pub(super) string: Option<String>,
}

// Small Rust token reader, not a search for annotation-shaped text. Comments
// (including nested block comments), raw strings and quoted strings are opaque.
pub(super) fn tokens(source: &str) -> Result<Vec<Token<'_>>, String> {
    tokenize(source, false)
}

/// Rust lexical tokens (`::`, `=>`, `<<`, whole numeric literals): the spelling
/// `stringify!` gives each token, which exported fn_info! link names are built
/// from. Declaration readers use the punctuation-per-character stream above.
pub(super) fn rust_tokens(source: &str) -> Result<Vec<Token<'_>>, String> {
    tokenize(source, true)
}

fn tokenize(source: &str, lexical: bool) -> Result<Vec<Token<'_>>, String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source[i..].starts_with("//") {
            i += source[i..].find('\n').unwrap_or(bytes.len() - i);
            continue;
        }
        if source[i..].starts_with("/*") {
            i += 2;
            let mut depth = 1;
            while depth != 0 && i < bytes.len() {
                if source[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if source[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
            }
            if depth != 0 {
                return Err("unterminated Rust comment".into());
            }
            continue;
        }
        let start = i;
        let raw = if bytes[i] == b'r' {
            Some(i + 1)
        } else if matches!(bytes[i], b'b' | b'c') && bytes.get(i + 1) == Some(&b'r') {
            Some(i + 2)
        } else {
            None
        };
        let raw = raw.and_then(|mut j| {
            let begin = j;
            while bytes.get(j) == Some(&b'#') {
                j += 1;
            }
            (bytes.get(j) == Some(&b'"')).then_some((j, j - begin))
        });
        if let Some((quote, hashes)) = raw {
            let end = format!("\"{}", "#".repeat(hashes));
            let content = quote + 1;
            let length = source[content..]
                .find(&end)
                .ok_or("unterminated Rust raw string")?;
            i = content + length + end.len();
            out.push(Token {
                text: &source[start..i],
                start,
                end: i,
                string: Some(source[content..content + length].into()),
            });
            continue;
        }
        let quote = if bytes[i] == b'"' {
            Some(i)
        } else if matches!(bytes[i], b'b' | b'c') && bytes.get(i + 1) == Some(&b'"') {
            Some(i + 1)
        } else {
            None
        };
        if let Some(quote) = quote {
            i = quote + 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i == bytes.len() {
                return Err("unterminated Rust string".into());
            }
            i += 1;
            // All realm selectors use ordinary JSON-compatible Rust strings.
            // Other Rust strings remain opaque even if they use Rust-only escapes.
            let string = json::parse(&source[quote..i])
                .ok()
                .and_then(|v| v.string().map(str::to_owned));
            out.push(Token {
                text: &source[start..i],
                start,
                end: i,
                string,
            });
            continue;
        }
        // A character literal may contain punctuation or a quote. Lifetimes
        // have no closing quote and are ordinary tokens for this scanner.
        if bytes[i] == b'\'' {
            let mut j = i + 1;
            if bytes.get(j) == Some(&b'\\') {
                j += 2;
            } else if j < bytes.len() {
                j += source[j..].chars().next().unwrap().len_utf8();
            }
            if bytes.get(j) == Some(&b'\'') {
                i = j + 1;
                out.push(Token {
                    text: &source[start..i],
                    start,
                    end: i,
                    string: None,
                });
                continue;
            }
        }
        if lexical && bytes[i].is_ascii_digit() {
            let nondecimal = source[i..].starts_with("0x")
                || source[i..].starts_with("0o")
                || source[i..].starts_with("0b");
            i += 1;
            while i < bytes.len() {
                if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                    i += 1;
                } else if !nondecimal
                    && bytes[i] == b'.'
                    && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
                {
                    i += 1;
                } else if !nondecimal
                    && matches!(bytes[i], b'+' | b'-')
                    && matches!(bytes[i - 1], b'e' | b'E')
                {
                    i += 1;
                } else {
                    break;
                }
            }
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        } else if lexical
            && ["<<=", ">>=", "..=", "..."]
                .iter()
                .any(|p| source[i..].starts_with(p))
        {
            i += 3;
        } else if lexical
            && [
                "..", "::", "<<", ">>", "==", "!=", "<=", ">=", "&&", "||", "->", "=>", "+=", "-=",
                "*=", "/=", "%=", "&=", "|=", "^=",
            ]
            .iter()
            .any(|p| source[i..].starts_with(p))
        {
            i += 2;
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
        out.push(Token {
            text: &source[start..i],
            start,
            end: i,
            string: None,
        });
    }
    Ok(out)
}

/// The `export` form names its static from the fully qualified body and ABI
/// builders. Read the same tokens at the owning declaration; no linker-name
/// inventory or source-file-to-Rust-module guess is needed.
pub(super) fn image_info_symbol(
    ts: &[Token<'_>],
    body_start: usize,
    package: &str,
) -> Result<String, String> {
    let comma = ts[body_start..]
        .iter()
        .position(|t| t.text == ",")
        .ok_or("exported fn_info has no parameter count")?
        + body_start;
    let body = ts[body_start..comma]
        .iter()
        .map(|t| t.text)
        .collect::<String>();
    if !body.starts_with("crate::") || !body.split("::").all(rust_identifier) {
        return Err("exported fn_info needs a fully qualified crate:: body path".into());
    }
    let mut pos = comma + 1;
    let count_start = pos;
    while ts.get(pos).is_some_and(|t| !matches!(t.text, ";" | ")")) {
        pos += 1;
    }
    let count = ts[count_start..pos]
        .iter()
        .map(|t| t.text)
        .collect::<String>();
    if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
        return Err("exported fn_info needs a literal parameter count".into());
    }
    let mut name = format!("perry_fn_info_{package}::{body}::{count}::");
    if ts.get(pos).is_some_and(|t| t.text == ")") {
        return Ok(name);
    }
    pos += 1;
    loop {
        let builder = ts.get(pos).ok_or("unterminated exported fn_info")?.text;
        if builder == ")" {
            break;
        }
        if !rust_identifier(builder) || ts.get(pos + 1).is_none_or(|t| t.text != "(") {
            return Err("malformed exported fn_info builder".into());
        }
        pos += 2;
        let start = pos;
        let mut depth = 1;
        while let Some(token) = ts.get(pos) {
            if token.text == "(" {
                depth += 1;
            } else if token.text == ")" {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            pos += 1;
        }
        if depth != 0 {
            return Err("unterminated exported fn_info builder".into());
        }
        // fn_info!'s export helper recursively concatenates lexical tokens,
        // preserving delimiters with one separator after each lexical token.
        // Separators keep distinct valid expressions (casts and identifiers,
        // for example) from collapsing to the same exported spelling.
        let argument = ts[start..pos]
            .iter()
            .map(|t| format!("{} ", t.text))
            .collect::<String>();
        name.push_str(&format!("{builder}({argument});"));
        pos += 1;
        match ts.get(pos).map(|t| t.text) {
            Some(",") => pos += 1,
            Some(")") => break,
            Some(";") if ts.get(pos + 1).is_some_and(|t| t.text == "bind") => break,
            _ => return Err("unterminated exported fn_info builder list".into()),
        }
    }
    Ok(name)
}

fn rust_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
fn declarations(
    source: &str,
    path: &str,
    package: &str,
    resolve: impl FnMut(&str) -> Result<(String, String), String>,
) -> Result<Vec<Value>, String> {
    let mut all = declarations_with_receivers(
        source,
        path,
        package,
        resolve,
        &mut Vec::new(),
        &native::runtime_abi()?,
    )?;
    derive::finish(&mut all)?;
    Ok(all)
}

fn declarations_with_receivers(
    source: &str,
    path: &str,
    package: &str,
    mut resolve: impl FnMut(&str) -> Result<(String, String), String>,
    native_receivers: &mut Vec<(String, String)>,
    native_abi: &native::Abi,
) -> Result<Vec<Value>, String> {
    if !source.contains("realm") {
        return Ok(Vec::new());
    }
    let ts = tokens(source)?;
    let mut out = Vec::new();
    let mut skip_until = 0;
    for (index, token) in ts.iter().enumerate() {
        if index < skip_until {
            continue;
        }
        let annotation = token.text == "realm"
            && index >= 2
            && ts[index - 1].text == "["
            && ts[index - 2].text == "#";
        let object = token.text == "realm_decl" && ts.get(index + 1).is_some_and(|t| t.text == "!");
        if !annotation && !object {
            continue;
        }
        let open = index + if object { 2 } else { 1 };
        if object
            && ts.get(open).is_some_and(|t| t.text == "(")
            && ts.get(open + 1).is_some_and(|t| t.text == "@")
        {
            // Internal projection helpers carry expressions, not realm births.
            continue;
        }
        if object && ts.get(open).is_some_and(|t| t.text == "{") {
            let mut depth = 1;
            let mut end = open + 1;
            while depth != 0 {
                let token = ts.get(end).ok_or("unterminated realm_decl! block")?;
                match token.text {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
                if depth != 0 {
                    end += 1;
                }
            }
            if ts.get(open + 1).is_some_and(|t| t.text == "$") {
                continue;
            }
            if ts
                .get(open + 1)
                .is_some_and(|t| t.text == "native_nonconstructors")
            {
                native::nonconstructors(&ts[open + 1..end])?;
                skip_until = end + 1;
                continue;
            }
            let mut entries = &ts[open + 1..end];
            if entries
                .first()
                .is_some_and(|t| matches!(t.text, "birth" | "record"))
            {
                let first = info::split(entries, ";")[0].len();
                let expression = &entries[1..first];
                let tail = &entries[first + 1..];
                if tail.len() < 3
                    || tail[0].text != "realm"
                    || tail[1].text != "{"
                    || tail.last().is_none_or(|t| t.text != "}")
                {
                    return Err("birth expression needs its realm block".into());
                }
                let mut birth_name = None;
                for entry in info::split(&tail[2..tail.len() - 1], ";") {
                    let mut declaration = if entries[0].text == "record" {
                        record::birth(expression, entry)?
                    } else {
                        birth::birth_at(expression, entry, source)?
                    };
                    let Value::Object(map) = &mut declaration else {
                        unreachable!()
                    };
                    // A Set born from a literal constant: its compiled cells read
                    // the same literal (the storage extension), never a copy.
                    let text = info::text(expression);
                    if let Some(constant) = text
                        .strip_prefix("module_set_value(")
                        .and_then(|rest| rest.strip_suffix(')'))
                    {
                        let owner = (0..index)
                            .rev()
                            .find(|&i| ts[i].text == "fn" && ts.get(i + 1).is_some())
                            .map(|i| ts[i + 1].text)
                            .ok_or("Set birth has no owning function")?;
                        map.insert(
                            "storage".into(),
                            Value::Object(BTreeMap::from([
                                ("kind".into(), Value::String("set".into())),
                                (
                                    "entries_source".into(),
                                    Value::String(format!("{owner}::{constant}")),
                                ),
                            ])),
                        );
                        crate::realm_storage::resolve(source, map)
                            .map_err(|e| format!("{path}: {e}"))?;
                    }
                    // Every path of one birth names the same cell: a dynamic
                    // value names that birth, the block's first path.
                    let name = map.get("name").and_then(Value::string).map(str::to_owned);
                    let birth_name = birth_name.get_or_insert(name).clone();
                    if let (Some(Value::Object(value)), Some(birth_name)) =
                        (map.get_mut("value"), birth_name)
                    {
                        if value.get("kind").and_then(Value::string) == Some("dynamic") {
                            value.insert("value".into(), Value::String(birth_name));
                        }
                    }
                    map.insert("source".into(), Value::String(path.replace('\\', "/")));
                    map.insert(
                        "line".into(),
                        Value::Number(
                            (source[..token.start]
                                .bytes()
                                .filter(|&b| b == b'\n')
                                .count()
                                + 1)
                            .to_string(),
                        ),
                    );
                    out.push(declaration);
                }
                skip_until = end + 1;
                continue;
            }
            if entries.first().is_some_and(|t| t.text == "object") {
                let mut declaration = object::birth(entries)?;
                let Value::Object(map) = &mut declaration else {
                    unreachable!()
                };
                map.insert("source".into(), Value::String(path.replace('\\', "/")));
                map.insert(
                    "line".into(),
                    Value::Number(
                        (source[..token.start]
                            .bytes()
                            .filter(|&b| b == b'\n')
                            .count()
                            + 1)
                        .to_string(),
                    ),
                );
                out.push(declaration);
                skip_until = end + 1;
                continue;
            }
            if entries.first().is_some_and(|t| t.text == "module_provider") {
                let function = entries
                    .iter()
                    .position(|t| t.text == "fn")
                    .ok_or("module provider needs its function item")?;
                let function_name = entries
                    .get(function + 1)
                    .ok_or("missing module provider name")?
                    .text;
                let open = entries[function + 2..]
                    .iter()
                    .position(|t| t.text == "{")
                    .ok_or("missing module provider body")?
                    + function
                    + 2;
                let mut depth = 1;
                let mut close = open + 1;
                while depth != 0 {
                    match entries
                        .get(close)
                        .ok_or("unterminated module provider body")?
                        .text
                    {
                        "{" => depth += 1,
                        "}" => depth -= 1,
                        _ => {}
                    }
                    close += 1;
                }
                let tail = &entries[close..];
                if tail.len() < 3
                    || tail[0].text != "realm"
                    || tail[1].text != "{"
                    || tail.last().unwrap().text != "}"
                {
                    return Err("module provider needs its realm exports".into());
                }
                let (body, _) = resolve(&format!("@provider:{function_name}"))?;
                for entry in info::split(&tail[2..tail.len() - 1], ";") {
                    let mut declaration = module_namespace::birth(entry)?;
                    let Value::Object(map) = &mut declaration else {
                        unreachable!()
                    };
                    map.insert("body".into(), Value::String(body.clone()));
                    map.insert("source".into(), Value::String(path.replace('\\', "/")));
                    map.insert(
                        "line".into(),
                        Value::Number(
                            (source[..token.start]
                                .bytes()
                                .filter(|&b| b == b'\n')
                                .count()
                                + 1)
                            .to_string(),
                        ),
                    );
                    out.push(declaration);
                }
                skip_until = end + 1;
                continue;
            }
            let native_key = if entries.first().is_some_and(|t| t.text == "native_key") {
                if entries.len() < 7
                    || entries[2].text != ";"
                    || entries[3].text != "realm"
                    || entries[4].text != "{"
                    || entries.last().unwrap().text != "}"
                {
                    return Err("native key birth requires a literal and realm cells".into());
                }
                let key = entries[1]
                    .string
                    .clone()
                    .ok_or("native key must be a literal")?;
                entries = &entries[5..entries.len() - 1];
                Some(key)
            } else {
                None
            };
            let owned_info = if entries.first().is_some_and(|t| t.text == "info_expr") {
                let first = info::split(entries, ";")[0].len();
                let body_info =
                    typed_info::expression(source, &entries[1..first], abi_constants())?;
                let tail = &entries[first + 1..];
                if tail.len() < 3
                    || tail[0].text != "realm"
                    || tail[1].text != "{"
                    || tail.last().is_none_or(|t| t.text != "}")
                {
                    return Err("owned info expression requires a realm cell block".into());
                }
                entries = &tail[2..tail.len() - 1];
                Some(body_info)
            } else if entries.first().is_some_and(|t| t.text == "info")
                && entries
                    .get(1)
                    .is_some_and(|t| !matches!(t.text, "pub" | "static" | "const"))
            {
                // The owning item is an exported record:
                // `fn_info!(export static [vis] NAME = ...);`.
                let first = info::split(entries, ";")[0];
                let at = first
                    .iter()
                    .position(|t| t.text == "static")
                    .filter(|&i| i > 0 && first[i - 1].text == "export")
                    .ok_or("info realm block must own its existing static declaration")?;
                let mut item = at + 1;
                if first.get(item).is_some_and(|t| t.text == "pub") {
                    item += 1;
                    if first.get(item).is_some_and(|t| t.text == "(") {
                        item = closing_paren(first, item)? + 1;
                    }
                }
                let binding = first.get(item).ok_or("missing owned info name")?.text;
                let tail = &entries[first.len() + 1..];
                if tail.len() < 3
                    || tail[0].text != "realm"
                    || tail[1].text != "{"
                    || tail.last().is_none_or(|t| t.text != "}")
                {
                    return Err("owned info requires a realm cell block".into());
                }
                entries = &tail[2..tail.len() - 1];
                Some(typed_info::named(source, binding, abi_constants())?)
            } else if entries.first().is_some_and(|t| t.text == "info") {
                let mut item = 1;
                if entries.get(item).is_some_and(|t| t.text == "pub") {
                    item += 1;
                    if entries.get(item).is_some_and(|t| t.text == "(") {
                        item = closing_paren(entries, item)? + 1;
                    }
                }
                if entries
                    .get(item)
                    .is_none_or(|t| !matches!(t.text, "static" | "const"))
                {
                    return Err("info realm block must own its existing static declaration".into());
                }
                let binding = entries.get(item + 1).ok_or("missing owned info name")?.text;
                let first = info::split(entries, ";")[0].len();
                let tail = &entries[first + 1..];
                if tail.len() < 3
                    || tail[0].text != "realm"
                    || tail[1].text != "{"
                    || tail.last().is_none_or(|t| t.text != "}")
                {
                    return Err("owned info requires a realm cell block".into());
                }
                entries = &tail[2..tail.len() - 1];
                Some(typed_info::named(source, binding, abi_constants())?)
            } else {
                None
            };
            for entry in info::split(entries, ";") {
                let mut human = facts::parse(entry)?;
                if let Some(key) = &native_key {
                    if human.native_method {
                        return Err("native key birth already determines its provider".into());
                    }
                    human.native_method = true;
                    human.native_key = Some(key.clone());
                }
                if human.native_method {
                    native_receivers.push((
                        human.canonical_name().to_owned(),
                        human.receiver.clone().ok_or("missing native receiver")?,
                    ));
                }
                let declarations = if human.ansi_colors {
                    if owned_info.is_some() || info::split(entry, ",").len() != 2 {
                        return Err(
                            "AnsiStyle provider owns its typed rows without extra facts".into()
                        );
                    }
                    colors::birth(&human.name, source)?
                } else if let Some(body_info) = &owned_info {
                    let (body, symbol) = resolve(&body_info.body)?;
                    vec![facts::callable(
                        human,
                        body_info,
                        abi_constants(),
                        body,
                        symbol,
                        source,
                    )?]
                } else {
                    vec![facts::logical(human, source, native_abi)?]
                };
                for mut declaration in declarations {
                    let Value::Object(ref mut map) = declaration else {
                        unreachable!()
                    };
                    map.insert("source".into(), Value::String(path.replace('\\', "/")));
                    map.insert(
                        "line".into(),
                        Value::Number(
                            (source[..token.start]
                                .bytes()
                                .filter(|&b| b == b'\n')
                                .count()
                                + 1)
                            .to_string(),
                        ),
                    );
                    out.push(declaration);
                }
            }
            skip_until = end + 1;
            continue;
        }
        if ts.get(open).is_none_or(|t| t.text != "(") {
            return Err(format!("{path}: malformed realm declaration"));
        }
        if ts.get(open + 1).is_some_and(|t| t.text == "$") {
            continue;
        }
        // Realm facts are declared at their birth; a JSON selector (the
        // retired companion transport) or inline JSON is not a declaration.
        if ts
            .get(open + 1)
            .and_then(|t| t.string.as_ref())
            .is_some_and(|literal| {
                literal.contains(".realm.json#") || literal.trim_start().starts_with(['[', '{'])
            })
        {
            return Err(format!(
                "{path}: realm facts are declared at their birth, not in a JSON selector"
            ));
        }
        if !annotation {
            return Err(format!(
                "{path}: expected structured fn_info realm attribute"
            ));
        }
        let end = closing_paren(&ts, open)?;
        if ts.get(end + 1).is_none_or(|t| t.text != "]") {
            return Err(format!("{path}: malformed realm attribute"));
        }
        let fn_start = (0..index)
            .rev()
            .find(|&i| {
                matches!(ts[i].text, "fn_info" | "js_function_info")
                    && ts.get(i + 1).is_some_and(|t| t.text == "!")
                    && ts.get(i + 2).is_some_and(|t| t.text == "(")
                    && closing_paren(&ts, i + 2).is_ok_and(|close| close > end)
            })
            .ok_or_else(|| format!("{path}: realm attribute must belong to fn_info!"))?;
        let fn_end = closing_paren(&ts, fn_start + 2)?;
        let body_info = info::fn_info(&ts[end + 2..fn_end], abi_constants())?;
        let (body, symbol) = resolve(&body_info.body)?;
        let humans = facts::parse_many(&ts[open + 1..end])?;
        let binding = info::metadata_binding(&ts[end + 2..fn_end]);
        if binding && humans.len() != 1 {
            return Err("metadata binding needs one physical cell".into());
        }
        for human in humans {
            if human.emit_metadata != (binding || human.metadata_macro.is_some()) {
                return Err("public metadata needs a runtime macro binding at fn_info!".into());
            }
            if binding && human.spec_name.is_none() && human.spec_length.is_none() {
                return Err("metadata binding requires a nondefault public name or length".into());
            }
            let mut declaration = facts::callable(
                human,
                &body_info,
                abi_constants(),
                body.clone(),
                symbol.clone(),
                source,
            )?;
            let Value::Object(ref mut map) = declaration else {
                unreachable!()
            };
            map.insert("source".into(), Value::String(path.replace('\\', "/")));
            map.insert(
                "line".into(),
                Value::Number(
                    (source[..ts[fn_start].start]
                        .bytes()
                        .filter(|&b| b == b'\n')
                        .count()
                        + 1)
                    .to_string(),
                ),
            );
            out.push(declaration);
        }
        skip_until = end + 1;
    }
    Ok(out)
}

fn package_name(cargo: &str) -> Result<String, String> {
    let mut package = false;
    for line in cargo.lines().map(str::trim) {
        if line.starts_with('[') {
            package = line == "[package]";
        } else if package {
            if let Some(("name", value)) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                return json::parse(value)?
                    .string()
                    .map(str::to_owned)
                    .ok_or("package name is not a string".into());
            }
        }
    }
    Err("missing Cargo package name".into())
}

fn source_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            source_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn read(runtime_src: &Path) -> Result<Value, String> {
    let runtime_src = runtime_src.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let crates = runtime_src
        .parent()
        .and_then(Path::parent)
        .ok_or("runtime source has no crates parent")?;
    let mut roots = std::fs::read_dir(crates)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    roots.sort_by_key(|e| e.file_name());
    for entry in roots {
        let src = entry.path().join("src");
        if src.is_dir() {
            println!("cargo:rerun-if-changed={}", src.display());
            source_files(&src, &mut files).map_err(|e| e.to_string())?;
        }
    }
    let mut all = Vec::new();
    let mut exports = crate::realm_exports::InfoExports::default();
    let mut export_sources = Vec::new();
    let mut native_receivers = Vec::new();
    let sources = files
        .into_iter()
        .map(|file| {
            let source = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            Ok((file, source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let symbols = symbols::Sources::new(&sources, crates)?;
    let native_abi = native::runtime_abi()?;
    for (file, source) in &sources {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file.strip_prefix(crates).unwrap();
        let package_root = crates.join(relative.components().next().unwrap().as_os_str());
        let cargo =
            std::fs::read_to_string(package_root.join("Cargo.toml")).map_err(|e| e.to_string())?;
        let package = package_name(&cargo)?;

        all.extend(declarations_with_receivers(
            &source,
            &relative.to_string_lossy(),
            &package,
            |body| match body.strip_prefix("@provider:") {
                Some(name) => symbols
                    .provider_body(name, file)
                    .map(|body| (body, String::new())),
                None => symbols.resolve(body, file),
            },
            &mut native_receivers,
            &native_abi,
        )?);
        if source.contains("fn_info") && source.contains("export") {
            export_sources.push((source, relative.to_string_lossy().into_owned(), package));
        }
    }
    all.extend(crate::realm_layouts::read(&runtime_src)?);
    crate::realm_layouts::attach_births(&mut all)?;
    let needed = crate::realm_exports::InfoExports::needed(&all);
    for (source, path, package) in export_sources {
        exports.read(&source, &path, &package, &needed)?;
    }
    native::resolve_receivers(&mut all, &native_receivers)?;
    derive::finish(&mut all)?;
    exports.link(&mut all)?;
    all.sort_by(|a, b| {
        a.get("name")
            .and_then(Value::string)
            .cmp(&b.get("name").and_then(Value::string))
    });
    let mut names = BTreeSet::new();
    for declaration in &all {
        let name = declaration
            .get("name")
            .and_then(Value::string)
            .ok_or("realm declaration has no name")?;
        if name.is_empty() || !names.insert(name) {
            return Err(format!("duplicate/empty realm declaration: {name}"));
        }
    }
    Ok(Value::Object(std::collections::BTreeMap::from([
        (
            "version".into(),
            Value::Number(REALM_MANIFEST_VERSION.to_string()),
        ),
        ("declarations".into(), Value::Array(all)),
    ])))
}

/// Check every entry against the declaration actually present in source.
/// Neither a valid file nor an annotation-shaped comment is sufficient: the
/// source reader must find this name at this exact declaring macro.
pub fn validate_provenance(runtime_src: &Path, manifest: &Value) -> Result<(), String> {
    let source = read(runtime_src)?;
    let expected = source
        .get("declarations")
        .and_then(Value::array)
        .ok_or("missing source declarations")?;
    let expected: std::collections::BTreeMap<_, _> = expected
        .iter()
        .map(|d| (d.get("name").and_then(Value::string).unwrap(), d))
        .collect();
    let entries = manifest
        .get("declarations")
        .and_then(Value::array)
        .ok_or("missing manifest declarations")?;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Value::string)
            .ok_or("missing name")?;
        if !seen.insert(name) {
            return Err(format!("duplicate provenance entry: {name}"));
        }
        let declaration = expected
            .get(name)
            .ok_or_else(|| format!("no source declaration for {name}"))?;
        assert_origin(entry, declaration)?;
    }
    if seen.len() != expected.len() {
        return Err("manifest omits source declarations".into());
    }
    Ok(())
}

fn assert_origin(entry: &Value, declaration: &Value) -> Result<(), String> {
    for field in ["source", "line"] {
        if entry.get(field).is_none() || entry.get(field) != declaration.get(field) {
            return Err(format!(
                "realm provenance disagrees with declaring macro: {:?} ({field})",
                entry.get("name").and_then(Value::string)
            ));
        }
    }
    Ok(())
}

/// Emit runtime build artifact and compiler-only table. Nothing includes this
/// table in the runtime, and WRT3a does not emit any realm data into programs.
pub fn generate(runtime_src: &Path, out: &Path) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let manifest = read(runtime_src)?;
    let entries = manifest
        .get("declarations")
        .and_then(Value::array)
        .ok_or("missing realm declarations")?;
    for root in ["Object", "Function", "Array"] {
        if !entries
            .iter()
            .any(|d| d.get("name").and_then(Value::string) == Some(root))
        {
            return Err(format!("missing realm root {root}"));
        }
    }
    validate_provenance(runtime_src, &manifest)?;
    projection::generate(runtime_src, out)?;
    std::fs::write(out.join("realm-v1.json"), manifest.encode()).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("realm-build-time-ns"),
        start.elapsed().as_nanos().to_string(),
    )
    .map_err(|e| e.to_string())?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inline(source: &str) -> Result<Vec<Value>, String> {
        declarations(source, "array.rs", "perry-runtime", |body| {
            Ok((body.into(), format!("source:{body}")))
        })
    }
    #[test]
    fn bare_constructors_and_reference_captures_keep_one_instance_property() {
        let d = &inline("fn_info!(#[realm(Example,kind=constructor,provider=closure)] body,1;with_declared(1));").unwrap()[0];
        assert_eq!(
            d.get("own_key_order").unwrap().encode(),
            r#"["length","name","prototype"]"#
        );
        let d = &inline("fn_info!(#[realm(Example.method,constructible=true,capture=[ref(Example.prototype)])] body,0);").unwrap()[0];
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&Value::Number("0".into()))
        );
        assert_eq!(
            d.get("births").unwrap().encode(),
            r#"["Example.prototype","Function.prototype"]"#
        );
        assert_eq!(
            d.get("captures").unwrap().array().unwrap()[0]
                .get("kind")
                .unwrap()
                .string(),
            Some("reference")
        );
    }

    #[test]
    fn ffi_body_declarations_share_the_typed_info_and_local_projection_contract() {
        let source = r#"js_function_info!(#[realm("Example.method",spec={length=1},emit=metadata)] body,2;with_flags(FN_BUILTIN);bind info,metadata);"#;
        let ds = inline(source).unwrap();
        assert_eq!(ds[0].get("length"), Some(&Value::Number("1".into())));
        assert_eq!(
            ds[0].get("function_info").unwrap().get("params"),
            Some(&Value::Number("2".into()))
        );
        assert_eq!(ds[0].get("line"), Some(&Value::Number("1".into())));
    }

    #[test]
    fn public_metadata_is_separate_from_abi_and_requires_a_runtime_projection() {
        let source = "fn_info!(#[realm(Foo, spec={length=0}, emit=metadata)] body,3;with_declared(3);bind info, metadata);";
        let declarations = inline(source).unwrap();
        assert_eq!(
            declarations[0].get("length"),
            Some(&Value::Number("0".into()))
        );
        assert_eq!(
            declarations[0].get("function_info").unwrap().get("params"),
            Some(&Value::Number("3".into()))
        );
        assert!(inline(&source.replace(";bind info, metadata", "")).is_err());
        assert!(inline(&source.replace("length=0", "length=3")).is_err());
    }

    #[test]
    fn callable_members_derive_edges_and_can_state_nondefault_intrinsic_order() {
        let source = r#"fn_info!(#[realm(Example,kind=constructor,members={from; prototype; [Symbol.species]=accessor(get="Example.species.<get>"); custom=Symbol.for("example");})] body,1);"#;
        let ds = inline(source).unwrap();
        let d = &ds[0];
        assert_eq!(
            d.get("own_key_order").unwrap().encode(),
            r#"["length","name","from","prototype","custom"]"#
        );
        let properties = d.get("properties").unwrap().array().unwrap();
        let prototype = properties
            .iter()
            .find(|p| p.get("key").and_then(Value::string) == Some("prototype"))
            .unwrap();
        assert_eq!(
            prototype.get("attributes"),
            Some(&Value::Number("0".into()))
        );
        assert_eq!(
            d.get("births").unwrap().encode(),
            r#"["Example.from","Example.prototype","Example.species.<get>","Function.prototype"]"#
        );
        assert!(
            inline("fn_info!(#[realm(Example,kind=constructor,members={length=1;})] body,1);")
                .is_err()
        );
        assert!(inline(
            "fn_info!(#[realm(Example,kind=constructor,members={prototype;})] body,1);"
        )
        .is_err());
    }

    #[test]
    fn callable_alias_keeps_the_birth_name_and_canonicalizes_cell_identity() {
        let ds = inline("fn_info!(#[realm(String.prototype.trimStart,alias=String.prototype.trimLeft)] body,0);").unwrap();
        assert_eq!(ds.len(), 1);
        let d = &ds[0];
        assert_eq!(
            d.get("name").and_then(Value::string),
            Some("String.prototype.trimLeft")
        );
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("trimStart")
        );
        assert_eq!(d.get("line"), Some(&Value::Number("1".into())));
        assert!(inline("fn_info!(#[realm(A.x,alias=A.x)] body,0);").is_err());
    }

    #[test]
    fn uncaptured_bare_closure_derives_empty_name_and_unrecorded_arity() {
        let d = &inline("fn_info!(#[realm(Example.method,provider=closure)] body,3);").unwrap()[0];
        assert_eq!(d.get("length"), Some(&Value::Number("0".into())));
        assert_eq!(d.get("captures").unwrap().array().unwrap().len(), 0);
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&Value::Number("0".into()))
        );
        assert_eq!(
            d.get("own_key_order").unwrap().encode(),
            r#"["length","name","prototype"]"#
        );
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("")
        );
    }

    #[test]
    fn allocator_birth_without_metadata_preserves_pointer_free_layout() {
        let d = &inline("fn_info!(#[realm(A.method,provider=closure_alloc)] body,1);").unwrap()[0];
        assert_eq!(d.get("length"), Some(&Value::Number("0".into())));
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&Value::Number(
                abi_constants()["GC_LAYOUT_POINTER_FREE"].to_string()
            ))
        );
        assert_eq!(d.get("captures").unwrap().array().unwrap().len(), 0);
    }

    #[test]
    fn accessor_label_can_derive_from_its_property_key() {
        let source = r#"fn_info!(#[realm("Example.connection.<get>",spec={name=property_key},emit=metadata(realm_connection))] body,0);"#;
        let d = &inline(source).unwrap()[0];
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("connection")
        );
        assert!(inline(&source.replace("connection.<get>", "connection")).is_err());
    }

    #[test]
    fn native_constructor_derives_its_instance_prototype_birth() {
        let d=&inline(r#"realm_decl! { "node:example.Thing",kind=constructor,provider=bound_method,receiver="node:example"; }"#).unwrap()[0];
        assert_eq!(
            d.get("construct").and_then(Value::string),
            Some("crate::object::js_new_function_construct")
        );
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[2]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("node:example.Thing.prototype")
        );
        assert!(d
            .get("births")
            .unwrap()
            .array()
            .unwrap()
            .contains(&Value::String("node:example.Thing.prototype".into())));
    }

    #[test]
    fn constructible_birth_derives_synthetic_key_without_a_descriptor() {
        let source = "fn_info!(#[realm(Storage.prototype.clear,constructible=true)] clear,0;with_declared(0));";
        let d = &inline(source).unwrap()[0];
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&Value::Number("0".into()))
        );
        assert_eq!(d.get("own_key_order").unwrap().array().unwrap().len(), 3);
        assert_eq!(d.get("properties").unwrap().array().unwrap().len(), 2);
        assert!(inline(&source.replace(
            "with_declared(0)",
            "with_flags(crate::closure::FN_NON_CONSTRUCTOR)"
        ))
        .is_err());
        assert!(inline(&source.replace("constructible=true", "constructible=false")).is_err());
    }

    #[test]
    fn builtin_constructor_birth_uses_the_declared_family_identity() {
        let d = &inline(
            "fn_info!(#[realm(Storage,kind=constructor,provider=builtin_constructor)] body,0);",
        )
        .unwrap()[0];
        let identity = d.get("constructor").unwrap().get("identity").unwrap();
        assert_eq!(
            d.get("captures").unwrap().array().unwrap()[0].get("value"),
            Some(identity)
        );
        let Value::Number(bits) = d.get("header").unwrap().get("capture_flags").unwrap() else {
            panic!("numeric capture flags")
        };
        let bits = bits.parse::<u64>().unwrap();
        let constants = abi_constants();
        assert_ne!(bits & constants["CLOSURE_NO_THIS_REBIND_FLAG"], 0);
        assert_eq!(
            Value::Number(
                ((bits & constants["CLOSURE_CONSTRUCT_MASK"])
                    >> constants["CLOSURE_CONSTRUCT_SHIFT"])
                    .to_string()
            ),
            *identity
        );
        assert!(inline(
            "fn_info!(#[realm(Unknown,kind=constructor,provider=builtin_constructor)] body,0);"
        )
        .is_err());
    }

    #[test]
    fn property_key_capture_derives_utf8_bytes_and_length_without_accessor_prefix() {
        let d = &inline(
            "fn_info!(#[realm(\"Example.prototype.é.<get>\",capture=property_key)] body,0);",
        )
        .unwrap()[0];
        let captures = d.get("captures").unwrap().array().unwrap();
        assert_eq!(captures[0].get("value").and_then(Value::string), Some("é"));
        assert_eq!(
            captures[1].get("value").and_then(Value::string),
            Some("0x2")
        );
        assert_eq!(
            d.get("header").unwrap().get("capture_flags"),
            Some(&Value::Number("0".into()))
        );
    }

    #[test]
    fn owned_info_block_preserves_body_abi_and_reports_the_block_line() {
        let source="realm_decl! { info static EMIT:JsFunctionInfo=JsFunctionInfo::of_native_args(emit,1); realm { Stream.emit,provider=closure,receiver=undefined; Events.emit,provider=closure,receiver=undefined; } }";
        let ds = inline(source).unwrap();
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0].get("line"), Some(&Value::Number("1".into())));
        assert_eq!(ds[0].get("body").and_then(Value::string), Some("emit"));
        assert_eq!(
            ds[0].get("header").unwrap().get("reserved"),
            Some(&Value::Number(
                abi_constants()["GC_LAYOUT_POINTER_FREE"].to_string()
            ))
        );
        assert_eq!(ds[0].get("function_info"), ds[1].get("function_info"));
        assert_eq!(
            ds[0].get("captures").unwrap().array().unwrap()[0]
                .get("value")
                .and_then(Value::string),
            Some("0x7ffc000000000001")
        );
    }

    #[test]
    fn provenance_rejects_wrong_file_wrong_line_and_missing_location() {
        let declared =
            json::parse(r#"{"name":"Atomics.add","source":"install_static.rs","line":1000}"#)
                .unwrap();
        for wrong in [
            r#"{"name":"Atomics.add","source":"atomics.rs","line":1000}"#,
            r#"{"name":"Atomics.add","source":"install_static.rs","line":825}"#,
            r#"{"name":"Atomics.add","source":"install_static.rs"}"#,
        ] {
            assert!(assert_origin(&json::parse(wrong).unwrap(), &declared).is_err());
        }
        assert!(assert_origin(&declared, &declared).is_ok());
    }

    #[test]
    fn structured_facts_derive_abi_and_report_the_actual_fn_info_line() {
        let ds = inline("fn_info!(\n#[realm(Atomics.add)]\njs_atomics_add,3; with_declared(3), with_flags(crate::closure::FN_BUILTIN));").unwrap();
        let d = &ds[0];
        assert_eq!(d.get("line"), Some(&Value::Number("1".into())));
        assert_eq!(d.get("length"), Some(&Value::Number("3".into())));
        assert_eq!(
            d.get("info_symbol").and_then(Value::string),
            Some("perry_realm_info_ce5097e0ac446db8a939")
        );
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&Value::Number("128".into()))
        );
        let changed = inline("fn_info!(#[realm(Atomics.add)]js_atomics_add,4; with_declared(4), with_flags(crate::closure::FN_BUILTIN));").unwrap();
        assert_eq!(changed[0].get("length"), Some(&Value::Number("4".into())));
        assert_ne!(changed[0].get("info_symbol"), d.get("info_symbol"));
    }

    #[test]
    fn shared_body_block_declares_each_physical_cell_at_one_body() {
        let ds =
            inline("fn_info!(#[realm(realm_decl! { A.next; B.next, births=[Array]; })] next,0);")
                .unwrap();
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0].get("body"), ds[1].get("body"));
        assert_eq!(ds[1].get("births").unwrap().array().unwrap().len(), 2);
        assert_eq!(ds[0].get("line"), ds[1].get("line"));
    }

    #[test]
    fn body_is_owned_by_fn_info_and_json_declarations_are_rejected() {
        let source = "crate::fn_info!(#[realm(Array.slice)] slice_thunk,2);";
        assert_eq!(
            inline(source).unwrap()[0]
                .get("body")
                .and_then(Value::string),
            Some("slice_thunk")
        );
        let json = r##"crate::fn_info!(#[realm(r#"{"name":"Array.slice","kind":"method"}"#)] slice_thunk,2);"##;
        assert!(inline(json).is_err());
    }
    #[test]
    fn rust_lexing_skips_comments_and_raw_strings_and_rejects_json_selectors() {
        let source = "// realm_decl!(\"fake\")\n/* /* realm_decl! */ */ let x=r###\"realm_decl!(fake)\"###;\nfn_info!(#[ realm (Example.slice) ] slice,2);";
        let ds = inline(source).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].get("line"), Some(&Value::Number("3".into())));
        assert!(inline("fn_info!(slice,2); realm_decl!(\"array.realm.json#*\");").is_err());
        assert!(inline("fn_info!(#[realm(\"array.realm.json#slice\")] slice,2);").is_err());
    }
}
