//! Shared build producer. Both builds read declarations beside runtime bodies;
//! the compiler never loads or links runtime machine code to obtain these facts.
use crate::realm_json::{self as json, Value};
pub const REALM_MANIFEST_VERSION: u32 = 1;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    string: Option<String>,
}

// Small Rust token reader, not a search for annotation-shaped text. Comments
// (including nested block comments), raw strings and quoted strings are opaque.
fn tokens(source: &str) -> Result<Vec<Token<'_>>, String> {
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
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
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

fn declarations(
    source: &str,
    path: &str,
    mut load: impl FnMut(&str) -> Result<String, String>,
) -> Result<Vec<Value>, String> {
    if !source.contains("realm") {
        return Ok(Vec::new());
    }
    let ts = tokens(source)?;
    let mut out = Vec::new();
    for (index, token) in ts.iter().enumerate() {
        let annotation = token.text == "realm"
            && index >= 2
            && ts[index - 1].text == "["
            && ts[index - 2].text == "#";
        let object = token.text == "realm_decl" && ts.get(index + 1).is_some_and(|t| t.text == "!");
        if !annotation && !object {
            continue;
        }
        let open = index + if object { 2 } else { 1 };
        if ts.get(open).is_none_or(|t| t.text != "(") {
            return Err(format!("{path}: malformed realm declaration"));
        }
        if ts.get(open + 1).is_some_and(|t| t.text == "$") {
            continue;
        }
        let literal = ts
            .get(open + 1)
            .and_then(|t| t.string.as_ref())
            .ok_or_else(|| format!("{path}: realm needs a literal selector"))?;
        if ts.get(open + 2).is_none_or(|t| t.text != ")") {
            return Err(format!("{path}: realm needs one literal selector"));
        }
        let data = if literal.trim_start().starts_with('[') || literal.trim_start().starts_with('{')
        {
            json::parse(literal)?
        } else {
            let (file, group) = literal
                .split_once('#')
                .ok_or_else(|| format!("{path}: expected file.realm.json#group"))?;
            let data = json::parse(&load(file)?)?;
            if data.get("version") != Some(&Value::Number(REALM_MANIFEST_VERSION.to_string())) {
                return Err(format!("{path}: unsupported realm group version"));
            }
            data.get("groups")
                .and_then(|g| g.get(group))
                .cloned()
                .ok_or_else(|| format!("{path}: missing realm group {literal}"))?
        };
        let mut ds = match data {
            Value::Array(ds) => ds,
            Value::Object(_) => vec![data],
            _ => return Err(format!("{path}: realm declaration is not an object/array")),
        };
        let mut body = None;
        if annotation {
            let mut after = open + 3;
            if ts.get(after).is_none_or(|t| t.text != "]") {
                return Err(format!("{path}: malformed realm attribute"));
            }
            after += 1;
            if ts
                .get(after)
                .is_some_and(|t| matches!(t.text, "native_args" | "unwind" | "unwind_in_tests"))
            {
                after += 1;
            }
            let end = ts[after..]
                .iter()
                .position(|t| t.text == ",")
                .ok_or("missing fn_info body")?
                + after;
            body = Some(ts[after..end].iter().map(|t| t.text).collect::<String>());
        }
        for d in &mut ds {
            let Value::Object(d) = d else {
                return Err(format!("{path}: realm declaration is not an object"));
            };
            let name = d
                .get("name")
                .and_then(Value::string)
                .ok_or("realm declaration has no name")?
                .to_owned();
            d.insert("source".into(), Value::String(path.replace('\\', "/")));
            d.insert(
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
            if let Some(body) = &body {
                if d.get("body")
                    .and_then(Value::string)
                    .is_some_and(|b| b != body && b.rsplit("::").next() != body.rsplit("::").next())
                {
                    return Err(format!("{path}: {name} disagrees with fn_info body {body}"));
                }
                d.entry("body".into())
                    .or_insert_with(|| Value::String(body.clone()));
            }
        }
        // Retain token end in the scanner so malformed strings cannot leak
        // into the following annotation; source offsets are UTF-8 boundaries.
        debug_assert!(token.end <= source.len());
        out.extend(ds);
    }
    Ok(out)
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
    let mut selectors = BTreeSet::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let source = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let relative = file.strip_prefix(crates).unwrap();
        all.extend(declarations(
            &source,
            &relative.to_string_lossy(),
            |selector| {
                let path = file.parent().unwrap().join(selector);
                if path.extension().is_none_or(|e| e != "json") {
                    return Err("realm selector must name a JSON source".into());
                }
                selectors.insert(path.clone());
                println!("cargo:rerun-if-changed={}", path.display());
                std::fs::read_to_string(path).map_err(|e| e.to_string())
            },
        )?);
    }
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
        declarations(source, "array.rs", |_| Err("unexpected file".into()))
    }
    #[test]
    fn body_is_owned_by_fn_info_and_drift_fails() {
        let source = r##"crate::fn_info!(#[realm(r#"{"name":"Array.slice","kind":"method"}"#)] slice_thunk,2);"##;
        assert_eq!(
            inline(source).unwrap()[0]
                .get("body")
                .and_then(Value::string),
            Some("slice_thunk")
        );
        assert!(inline(&source.replace(
            "\"kind\":\"method\"",
            "\"kind\":\"method\",\"body\":\"other\""
        ))
        .is_err());
    }
    #[test]
    fn rust_lexing_and_external_groups() {
        let source="// realm_decl!(\"fake\")\n/* /* realm_decl! */ */ let x=r###\"realm_decl!(fake)\"###;\nfn_info!(#[ realm (\"array.realm.json#slice\") ] native_args slice,2);";
        let ds=declarations(source,"array.rs",|file|{assert_eq!(file,"array.realm.json");Ok(r#"{"version":1,"groups":{"slice":[{"name":"Array.slice","kind":"method","body":"crate::array::slice"}]}}"#.into())}).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].get("line"), Some(&Value::Number("3".into())));
    }
}
