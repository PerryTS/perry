use super::{COMPILE_ENV_EXCLUSIONS, COMPILE_ENV_VARS};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// A small lexical scanner: skip comments and treat strings as single tokens,
// so examples in documentation and embedded Rust/TypeScript are not env reads.
// Keep punctuation separate to accept spaces/newlines in paths and macro calls.
fn tokens(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b'\'' && bytes.get(i + 1) == Some(&b'\\') {
            // Escaped character literals (including b'\"') must not start a string.
            let mut end = i + 3;
            while end < bytes.len() && bytes[end] != b'\'' && end - i < 16 {
                end += 1;
            }
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
            } else {
                out.push("'".to_owned());
                i += 1;
            }
        } else if bytes[i] == b'\'' && i + 1 < bytes.len() {
            let end = i + 1 + source[i + 1..].chars().next().unwrap().len_utf8();
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
            } else {
                // A lifetime, such as 'static, is punctuation plus a word.
                out.push("'".to_owned());
                i += 1;
            }
        } else {
            let start = i;
            // Raw strings, including arbitrary hash delimiters.
            let mut quote = i;
            if bytes[i] == b'r' {
                quote += 1;
                while quote < bytes.len() && bytes[quote] == b'#' {
                    quote += 1;
                }
            }
            if bytes[i] == b'r' && bytes.get(quote) == Some(&b'"') {
                let hashes = quote - start - 1;
                i = quote + 1;
                let content = i;
                while i < bytes.len() {
                    if bytes[i] == b'"'
                        && bytes.get(i + 1..i + 1 + hashes) == Some(&bytes[start + 1..quote])
                    {
                        break;
                    }
                    i += 1;
                }
                out.push(format!("\"{}\"", &source[content..i]));
                i = (i + 1 + hashes).min(bytes.len());
            } else if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                out.push(source[start..i].to_owned());
            } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(source[start..i].to_owned());
            } else {
                // Non-ASCII punctuation is irrelevant to the ASCII env names.
                let ch = source[i..].chars().next().unwrap();
                out.push(ch.to_string());
                i += ch.len_utf8();
            }
        }
    }
    out
}

fn env_reads(source: &str) -> Vec<String> {
    let tokens = tokens(source);
    let mut found = Vec::new();
    // Resolve simple named string constants used as env-read arguments too.
    let mut constants = BTreeMap::new();
    for (i, token) in tokens.iter().enumerate() {
        if token != "const" {
            continue;
        }
        let Some(name) = tokens.get(i + 1) else {
            continue;
        };
        let mut j = i + 2;
        while j < tokens.len() && !matches!(tokens[j].as_str(), "=" | ";" | "{") {
            j += 1;
        }
        if tokens.get(j).map(String::as_str) == Some("=") {
            if let Some(value) = tokens.get(j + 1).filter(|v| v.starts_with('"')) {
                constants.insert(name.as_str(), value.as_str());
            }
        }
    }
    for (i, token) in tokens.iter().enumerate() {
        let mut j = i + 1;
        match token.as_str() {
            "var" | "var_os" | "env_flag" => {}
            "env" | "option_env" if tokens.get(j).map(String::as_str) == Some("!") => {
                j += 1;
            }
            _ => continue,
        }
        if !matches!(tokens.get(j).map(String::as_str), Some("(" | "{" | "[")) {
            continue;
        }
        let Some(name) = tokens
            .get(j + 1)
            .map(|s| constants.get(s.as_str()).copied().unwrap_or(s.as_str()))
            .and_then(|s| s.strip_prefix('"'))
            .and_then(|s| s.strip_suffix('"'))
        else {
            continue;
        };
        if name.starts_with("PERRY_")
            && name.len() > 6
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            found.push(name.to_owned());
        }
    }
    found
}

// Discover every workspace dependency the compiler can link, including optional,
// target and build dependencies. A newly linked crate joins the audit automatically.
fn compiler_crates(crates: &Path) -> Vec<PathBuf> {
    let mut manifests = BTreeMap::new();
    for entry in std::fs::read_dir(crates).expect("workspace crates") {
        let dir = entry.unwrap().path();
        let path = dir.join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let manifest = toml::Value::Table(
            std::fs::read_to_string(path)
                .unwrap()
                .parse::<toml::Table>()
                .unwrap(),
        );
        let name = manifest["package"]["name"].as_str().unwrap().to_owned();
        manifests.insert(name, (dir, manifest));
    }
    let mut todo = vec!["perry".to_owned()];
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let (dir, manifest) = &manifests[&name];
        roots.push(dir.clone());
        let mut tables = vec![manifest];
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            tables.extend(targets.values());
        }
        for table in tables {
            for kind in ["dependencies", "build-dependencies"] {
                if let Some(deps) = table.get(kind).and_then(toml::Value::as_table) {
                    for (alias, dep) in deps {
                        let name = dep
                            .get("package")
                            .and_then(toml::Value::as_str)
                            .unwrap_or(alias);
                        if manifests.contains_key(name) {
                            todo.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    roots
}

#[test]
fn env_read_scanner_handles_all_read_forms_without_scanning_examples() {
    let source = r####"
        std::env :: var (
            "PERRY_ONE");
        env::var_os( r#"PERRY_TWO"# );
        option_env! ("PERRY_THREE"); env!("PERRY_FOUR");
        var("PERRY_FIVE"); env_flag("PERRY_SIX");
        const NAMED: &str = "PERRY_SEVEN"; env::var_os(NAMED);
        env! { "PERRY_EIGHT" }; option_env!["PERRY_NINE"];
        // env::var("PERRY_COMMENT")
        /* /* nested */ env::var_os("PERRY_COMMENT") */
        let example = r###"env::var("PERRY_EXAMPLE")"###;
        let ordinary = "env::var(\"PERRY_EXAMPLE\")";
        let quote = '"'; let escaped = '\''; let byte_quote = b'"';
        env::var("OTHER_NAME"); env::var("PERRY_TEN");
    "####;
    assert_eq!(
        env_reads(source),
        [
            "PERRY_ONE",
            "PERRY_TWO",
            "PERRY_THREE",
            "PERRY_FOUR",
            "PERRY_FIVE",
            "PERRY_SIX",
            "PERRY_SEVEN",
            "PERRY_EIGHT",
            "PERRY_NINE",
            "PERRY_TEN"
        ]
    );
}

#[test]
fn codegen_env_vars_are_build_cache_inputs() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let roots = compiler_crates(&crates);
    assert!(
        roots.len() >= 24,
        "compiler dependency scan shrank unexpectedly"
    );
    let mut found = BTreeSet::new();
    let mut reads = 0;
    for root in roots {
        let mut stack = vec![root.join("src"), root.join("build.rs")];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(path).unwrap().map(|e| e.unwrap().path()));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") && path.is_file() {
                let names = env_reads(&std::fs::read_to_string(&path).unwrap());
                reads += names.len();
                found.extend(names);
            }
        }
    }
    // Audited inventory: 483 literal/constant read sites / 329 distinct names in this
    // change. A broken matcher or missing source roots must never pass vacuously.
    assert!(
        reads > 450 && found.len() > 310,
        "env audit found only {reads} reads / {} names",
        found.len()
    );
    assert!(
        COMPILE_ENV_VARS.windows(2).all(|pair| pair[0] < pair[1]),
        "shared list must be sorted and unique"
    );
    let exclusions: BTreeSet<_> = COMPILE_ENV_EXCLUSIONS
        .iter()
        .map(|(name, reason)| {
            assert!(
                !reason.trim().is_empty() && !reason.contains('\n'),
                "missing one-line reason for {name}"
            );
            assert!(
                !COMPILE_ENV_VARS.contains(name),
                "{name} is both keyed and excluded"
            );
            assert!(found.contains(*name), "stale exclusion: {name}");
            *name
        })
        .collect();
    assert_eq!(
        exclusions.len(),
        COMPILE_ENV_EXCLUSIONS.len(),
        "duplicate exclusion"
    );
    let missing: Vec<_> = found
        .iter()
        .filter(|name| {
            !COMPILE_ENV_VARS.contains(&name.as_str()) && !exclusions.contains(name.as_str())
        })
        .collect();
    assert!(missing.is_empty(), "unregistered compiler env reads: {missing:?}; add to compile_env::COMPILE_ENV_VARS or COMPILE_ENV_EXCLUSIONS with a reason (#12582)");
    eprintln!("compile env audit: {reads} reads, {} names", found.len());
}
