use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use super::parse_package_specifier;
use crate::commands::compile::cjs_wrap::detect::strip_comments_and_strings;

pub(super) fn transform_static_literal_requires(
    source: &str,
    compile_packages: &HashSet<String>,
) -> String {
    let create_require_aliases = collect_create_require_aliases(source);
    let mut require_aliases =
        collect_create_require_aliases_from_decls(source, &create_require_aliases);
    if !require_is_shadowed_by_non_create_require(source, &require_aliases) {
        require_aliases.insert("require".to_string());
    }
    if require_aliases.is_empty() {
        return source.to_string();
    }

    let masked_source = strip_comments_and_strings(source);
    let mut imported_specs = HashMap::new();
    let mut imports = Vec::new();
    let mut replacements = Vec::new();
    let mut next_id = 0usize;
    for alias in require_aliases {
        let call_re = literal_require_call_re(&alias);
        for cap in call_re.captures_iter(source) {
            let specifier = cap.name("spec").map(|m| m.as_str()).unwrap_or_default();
            if should_leave_runtime_require(specifier, compile_packages) {
                continue;
            }
            let Some(full) = cap.name("call") else {
                continue;
            };
            if masked_source[full.start()..full.end()]
                .bytes()
                .all(|b| b == b' ' || b == b'\t' || b == b'\r' || b == b'\n')
            {
                continue;
            }
            let temp = imported_specs
                .entry(specifier.to_string())
                .or_insert_with(|| {
                    let temp = unique_temp_name(source, &mut next_id);
                    imports.push(format!("import * as {temp} from {:?};", specifier));
                    temp
                })
                .clone();
            replacements.push((full.start(), full.end(), temp));
        }
    }

    if imports.is_empty() {
        return source.to_string();
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    let mut transformed = source.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        transformed.replace_range(start..end, &replacement);
    }
    prepend_imports_preserving_shebang(&transformed, &imports)
}

fn prepend_imports_preserving_shebang(source: &str, imports: &[String]) -> String {
    let mut prefix = imports.join("\n");
    prefix.push('\n');
    if source.starts_with("#!") {
        if let Some(line_end) = source.find('\n') {
            let mut out = String::new();
            out.push_str(&source[..=line_end]);
            out.push_str(&prefix);
            out.push_str(&source[line_end + 1..]);
            return out;
        }
        return format!("{source}\n{prefix}");
    }
    prefix.push_str(source);
    prefix
}

fn collect_create_require_aliases(source: &str) -> HashSet<String> {
    static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
    let import_re = IMPORT_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*import\s*\{(?P<specs>[^}]*)\}\s*from\s*['"](?:node:)?module['"]\s*;?"#,
        )
        .expect("createRequire import regex")
    });

    let mut aliases = HashSet::new();
    for cap in import_re.captures_iter(source) {
        let Some(specs) = cap.name("specs") else {
            continue;
        };
        for part in specs.as_str().split(',') {
            let part = part.trim();
            if part == "createRequire" {
                aliases.insert("createRequire".to_string());
                continue;
            }
            if let Some(rest) = part.strip_prefix("createRequire as ") {
                let alias = rest.trim();
                if is_identifier(alias) {
                    aliases.insert(alias.to_string());
                }
            }
        }
    }
    aliases
}

fn collect_create_require_aliases_from_decls(
    source: &str,
    create_require_aliases: &HashSet<String>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for create_alias in create_require_aliases {
        let decl_re = create_require_decl_re(create_alias);
        for cap in decl_re.captures_iter(source) {
            if let Some(alias) = cap.name("alias").map(|m| m.as_str()) {
                out.insert(alias.to_string());
            }
        }
    }
    out
}

fn create_require_decl_re(create_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)^\s*(?:const|let|var)\s+(?P<alias>[A-Za-z_$][A-Za-z0-9_$]*)(?:\s*:\s*[^=;]+)?\s*=\s*{}\s*\(\s*import\.meta\.url\s*\)\s*;?"#,
        regex::escape(create_alias)
    ))
    .expect("createRequire declaration regex")
}

fn literal_require_call_re(require_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)(?:^|[^A-Za-z0-9_$\.])(?P<call>{}\s*\(\s*['"](?P<spec>[^'"]+)['"]\s*\))"#,
        regex::escape(require_alias)
    ))
    .expect("static require literal call regex")
}

fn should_leave_runtime_require(specifier: &str, compile_packages: &HashSet<String>) -> bool {
    if perry_hir::is_native_module(specifier) {
        return true;
    }
    if is_relative_or_absolute_specifier(specifier) {
        return false;
    }
    let (package_name, _) = parse_package_specifier(specifier);
    !compile_packages.contains(&package_name)
}

fn is_relative_or_absolute_specifier(specifier: &str) -> bool {
    specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
        || specifier.starts_with('\\')
        || specifier.as_bytes().get(1) == Some(&b':')
}

fn require_is_shadowed_by_non_create_require(
    source: &str,
    create_require_aliases: &HashSet<String>,
) -> bool {
    if create_require_aliases.contains("require") {
        return false;
    }
    static SHADOW_RE: OnceLock<Regex> = OnceLock::new();
    let shadow_re = SHADOW_RE.get_or_init(|| {
        Regex::new(r#"(?m)^\s*(?:function\s+require\s*\(|(?:const|let|var)\s+require\b)"#)
            .expect("require shadow regex")
    });
    shadow_re.is_match(source)
}

fn unique_temp_name(source: &str, next_id: &mut usize) -> String {
    loop {
        let name = format!("__perry_static_require_{}", *next_id);
        *next_id += 1;
        if !source.contains(&name) {
            return name;
        }
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoists_direct_relative_literal_require() {
        let source = r#"
const local = require("./local");
const { Client } = require("../client");
"#;
        let got = transform_static_literal_requires(source, &HashSet::new());
        assert!(got.contains(r#"import * as __perry_static_require_0 from "./local";"#));
        assert!(got.contains(r#"import * as __perry_static_require_1 from "../client";"#));
        assert!(got.contains("const local = __perry_static_require_0;"));
        assert!(got.contains("const { Client } = __perry_static_require_1;"));
    }

    #[test]
    fn hoists_inline_member_literal_require() {
        let source = r#"
console.log(require("./local").value);
"#;
        let got = transform_static_literal_requires(source, &HashSet::new());
        assert!(got.contains(r#"import * as __perry_static_require_0 from "./local";"#));
        assert!(got.contains("console.log(__perry_static_require_0.value);"));
    }

    #[test]
    fn hoists_allowed_package_literal_require() {
        let source = r#"
const Discord = require("discord.js");
"#;
        let mut compile_packages = HashSet::new();
        compile_packages.insert("discord.js".to_string());
        let got = transform_static_literal_requires(source, &compile_packages);
        assert!(got.contains(r#"import * as __perry_static_require_0 from "discord.js";"#));
        assert!(got.contains("const Discord = __perry_static_require_0;"));
    }

    #[test]
    fn leaves_disallowed_package_and_builtin_requires() {
        let source = r#"
const Discord = require("discord.js");
const path = require("node:path");
"#;
        let got = transform_static_literal_requires(source, &HashSet::new());
        assert!(!got.contains("__perry_static_require_"));
        assert!(got.contains(r#"const Discord = require("discord.js");"#));
        assert!(got.contains(r#"const path = require("node:path");"#));
    }

    #[test]
    fn supports_create_require_aliases() {
        let source = r#"
import { createRequire as makeRequire } from "module";
const req = makeRequire(import.meta.url);
const { Client } = req("mini");
"#;
        let mut compile_packages = HashSet::new();
        compile_packages.insert("mini".to_string());
        let got = transform_static_literal_requires(source, &compile_packages);
        assert!(got.contains(r#"import * as __perry_static_require_0 from "mini";"#));
        assert!(got.contains("const { Client } = __perry_static_require_0;"));
    }

    #[test]
    fn direct_require_is_not_transformed_when_shadowed() {
        let source = r#"
function require(name) {
  return name;
}
const local = require("./local");
"#;
        let got = transform_static_literal_requires(source, &HashSet::new());
        assert_eq!(got, source);
    }

    #[test]
    fn ignores_require_mentions_in_comments_and_strings() {
        let source = r#"
// const local = require("./local");
const text = 'require("./local")';
"#;
        let got = transform_static_literal_requires(source, &HashSet::new());
        assert_eq!(got, source);
    }
}
