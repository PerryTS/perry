use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use super::parse_package_specifier;

pub(super) fn transform_create_require_literal_requires(
    source: &str,
    compile_packages: &HashSet<String>,
) -> String {
    let create_require_aliases = collect_create_require_aliases(source);
    if create_require_aliases.is_empty() {
        return source.to_string();
    }

    let require_aliases = collect_require_aliases(source, &create_require_aliases);
    if require_aliases.is_empty() {
        return source.to_string();
    }

    let mut transformed = source.to_string();
    let mut imports = Vec::new();
    let mut next_id = 0usize;
    for alias in require_aliases {
        let call_re = require_assignment_re(&alias);
        let mut replacements = Vec::new();
        for cap in call_re.captures_iter(&transformed) {
            let specifier = cap.name("spec").map(|m| m.as_str()).unwrap_or_default();
            if should_leave_runtime_require(specifier, compile_packages) {
                continue;
            }
            let Some(full) = cap.get(0) else {
                continue;
            };
            let temp = unique_temp_name(&transformed, &mut next_id);
            imports.push(format!("import * as {temp} from {:?};", specifier));
            let indent = cap.name("indent").map(|m| m.as_str()).unwrap_or("");
            let kind = cap.name("kind").map(|m| m.as_str()).unwrap_or("const");
            let lhs = cap.name("lhs").map(|m| m.as_str()).unwrap_or("").trim_end();
            let tail = cap.name("tail").map(|m| m.as_str()).unwrap_or("");
            replacements.push((
                full.start(),
                full.end(),
                format!("{indent}{kind} {lhs} = {temp};{tail}"),
            ));
        }

        if replacements.is_empty() {
            continue;
        }
        for (start, end, replacement) in replacements.into_iter().rev() {
            transformed.replace_range(start..end, &replacement);
        }
    }

    if imports.is_empty() {
        return source.to_string();
    }

    let mut prefix = imports.join("\n");
    prefix.push('\n');
    if transformed.starts_with("#!") {
        if let Some(line_end) = transformed.find('\n') {
            let mut out = String::new();
            out.push_str(&transformed[..=line_end]);
            out.push_str(&prefix);
            out.push_str(&transformed[line_end + 1..]);
            out
        } else {
            format!("{transformed}\n{prefix}")
        }
    } else {
        prefix.push_str(&transformed);
        prefix
    }
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

fn collect_require_aliases(source: &str, create_require_aliases: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for create_alias in create_require_aliases {
        let decl_re = create_require_decl_re(create_alias);
        for cap in decl_re.captures_iter(source) {
            let Some(alias) = cap.name("alias").map(|m| m.as_str()) else {
                continue;
            };
            if !out.iter().any(|existing| existing == alias) {
                out.push(alias.to_string());
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

fn require_assignment_re(require_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)^(?P<indent>[ \t]*)(?P<kind>const|let|var)\s+(?P<lhs>[^=\n;]+?)\s*=\s*{}\s*\(\s*['"](?P<spec>[^'"]+)['"]\s*\)\s*;?(?P<tail>[ \t]*(?://[^\n]*)?)$"#,
        regex::escape(require_alias)
    ))
    .expect("createRequire literal call regex")
}

fn should_leave_runtime_require(specifier: &str, compile_packages: &HashSet<String>) -> bool {
    let (package_name, _) = parse_package_specifier(specifier);
    perry_hir::is_native_module(specifier) && !compile_packages.contains(&package_name)
}

fn unique_temp_name(source: &str, next_id: &mut usize) -> String {
    loop {
        let name = format!("__perry_create_require_{}", *next_id);
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
    fn hoists_non_builtin_literal_requires_from_create_require() {
        let source = r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const Discord = require("discord.js");
const local: any = require("./local");
const path = require("node:path");
"#;
        let got = transform_create_require_literal_requires(source, &HashSet::new());
        assert!(got.contains(r#"import * as __perry_create_require_0 from "discord.js";"#));
        assert!(got.contains(r#"import * as __perry_create_require_1 from "./local";"#));
        assert!(got.contains("const Discord = __perry_create_require_0;"));
        assert!(got.contains("const local: any = __perry_create_require_1;"));
        assert!(got.contains(r#"const path = require("node:path");"#));
    }

    #[test]
    fn supports_renamed_create_require_and_destructuring() {
        let source = r#"
import { createRequire as makeRequire } from "module";
const req = makeRequire(import.meta.url);
const { Client } = req("mini");
"#;
        let got = transform_create_require_literal_requires(source, &HashSet::new());
        assert!(got.contains(r#"import * as __perry_create_require_0 from "mini";"#));
        assert!(got.contains("const { Client } = __perry_create_require_0;"));
    }
}
