use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::parse_package_specifier;
use crate::commands::compile::cjs_wrap::detect::strip_comments_and_strings;

pub(super) fn transform_static_literal_requires(
    source: &str,
    compile_packages: &HashSet<String>,
    module_dir: &Path,
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

    // #6873: classify each specifier as OPTIONAL — every one of its call sites
    // sits inside a `try` block. One unguarded use makes it load-bearing, so
    // the whole specifier stays mandatory.
    let mut optional_specs: HashMap<String, bool> = HashMap::new();
    for alias in &require_aliases {
        for cap in literal_require_call_re(alias).captures_iter(source) {
            let Some(full) = cap.name("call") else {
                continue;
            };
            // Skip matches that only exist inside a comment or string literal
            // — their span is blank in the masked copy. Same filter the
            // hoisting loop below applies. Without it a phantom mention like
            // `// fallback: require("./generated")` outside a `try` would
            // flip a genuinely optional specifier back to mandatory and
            // reintroduce the #6873 hard error.
            if masked_source[full.start()..full.end()]
                .bytes()
                .all(|b| b == b' ' || b == b'\t' || b == b'\r' || b == b'\n')
            {
                continue;
            }
            let specifier = cap.name("spec").map(|m| m.as_str()).unwrap_or_default();
            let in_try = is_inside_try_block(&masked_source, full.start());
            optional_specs
                .entry(specifier.to_string())
                .and_modify(|all_in_try| *all_in_try &= in_try)
                .or_insert(in_try);
        }
    }

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
            // #6873: hoisting `try { x = require("./gen") } catch {}` to a
            // top-level `import * as` discards both the guard and the catch,
            // and an unresolvable namespace import is a hard build error
            // (#629). Node and Bun instead throw at the call and let the catch
            // swallow it. When an optional specifier does not resolve on disk,
            // leave the call as a runtime `require` — that path already
            // reproduces Node exactly (it is what bare package specifiers do).
            //
            // Resolvable optional requires keep being hoisted, so a module
            // that IS present still gets compiled in and loads.
            if optional_specs.get(specifier).copied().unwrap_or(false)
                && !relative_specifier_resolves(module_dir, specifier)
            {
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

/// Does a relative/absolute `specifier` name a file that exists next to the
/// module being compiled? Mirrors the extension set the module resolver tries.
/// Non-relative specifiers return `true` so they keep today's hoisting.
fn relative_specifier_resolves(module_dir: &Path, specifier: &str) -> bool {
    if !is_relative_or_absolute_specifier(specifier) {
        return true;
    }
    const EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
    let base = module_dir.join(specifier);
    if base.is_file() {
        return true;
    }
    for ext in EXTENSIONS {
        if base.with_extension(ext).is_file() {
            return true;
        }
        if base.join(format!("index.{ext}")).is_file() {
            return true;
        }
    }
    false
}

/// Is byte offset `at` lexically inside the block of a `try { ... }`?
///
/// `masked` is the comment/string-blanked copy of the source (identical byte
/// offsets), so braces and the `try` keyword match textually without tripping
/// over literals.
///
/// A miss either way is safe: a false negative keeps today's hoisting, and a
/// false positive only downgrades an unresolvable module to a runtime require —
/// which is what Node does anyway.
fn is_inside_try_block(masked: &str, at: usize) -> bool {
    let bytes = masked.as_bytes();
    // One entry per open brace, recording whether it opened a `try` block.
    let mut stack: Vec<bool> = Vec::new();
    for (i, &b) in bytes.iter().enumerate().take(at.min(bytes.len())) {
        match b {
            b'{' => stack.push(brace_opens_try_block(masked, i)),
            b'}' => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.iter().any(|&is_try| is_try)
}

/// Does the `{` at `brace_idx` open a `try` block — is the immediately
/// preceding token the `try` keyword?
fn brace_opens_try_block(masked: &str, brace_idx: usize) -> bool {
    let bytes = masked.as_bytes();
    let mut i = brace_idx;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    let Some(start) = i.checked_sub(3) else {
        return false;
    };
    if !masked.is_char_boundary(start) || &masked[start..i] != "try" {
        return false;
    }
    // `retry {` / `o.try {` must not match — require a token boundary before.
    match start.checked_sub(1).map(|p| bytes[p]) {
        None => true,
        Some(prev) => {
            !(prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'$' || prev == b'.')
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
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert!(got.contains(r#"import * as __perry_static_require_0 from "./local";"#));
        assert!(got.contains(r#"import * as __perry_static_require_1 from "../client";"#));
        assert!(got.contains("const local = __perry_static_require_0;"));
        assert!(got.contains("const { Client } = __perry_static_require_1;"));
    }

    // #6873 ------------------------------------------------------------------

    /// A try-wrapped require of a module that is NOT on disk must stay a
    /// runtime `require`. Hoisting it would drop the guard and the catch, and
    /// an unresolvable namespace import is a hard build error (#629) — so a
    /// gitignored/generated optional file made the whole build fail.
    #[test]
    fn leaves_optional_try_wrapped_require_of_missing_module() {
        let source = r#"
export const X = 1;
let B = null;
try { B = require("./absent-generated").DATA; } catch {}
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert!(
            !got.contains("import * as"),
            "optional missing require must not be hoisted, got:\n{got}"
        );
        assert!(got.contains(r#"require("./absent-generated")"#));
    }

    /// The same shape, but the module EXISTS: keep hoisting so it is compiled
    /// in and actually loads.
    #[test]
    fn hoists_optional_try_wrapped_require_when_module_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("present.ts"), "export const DATA = 1;\n")
            .expect("write present.ts");
        let source = r#"
let B = null;
try { B = require("./present").DATA; } catch {}
"#;
        let got = transform_static_literal_requires(source, &HashSet::new(), dir.path());
        assert!(
            got.contains(r#"import * as __perry_static_require_0 from "./present";"#),
            "resolvable optional require must still be hoisted, got:\n{got}"
        );
    }

    /// One unguarded call site makes the specifier load-bearing: it is not
    /// optional, so it keeps being hoisted even though another site is inside
    /// a `try`.
    #[test]
    fn require_used_outside_try_is_not_optional() {
        let source = r#"
const eager = require("./shared");
try { const lazy = require("./shared"); } catch {}
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert!(
            got.contains(r#"import * as __perry_static_require_0 from "./shared";"#),
            "specifier with an unguarded use must stay hoisted, got:\n{got}"
        );
    }

    /// A `require(...)` mention that exists only inside a comment or a string
    /// is not a call site, so it must not drag a genuinely optional specifier
    /// back to mandatory. Before the masked-span filter in the classification
    /// loop, the commented mention below (outside any `try`) flipped
    /// `./absent-generated` to non-optional and the hoist reintroduced the
    /// exact #6873 hard error this change removes.
    #[test]
    fn comment_and_string_mentions_do_not_defeat_optional_classification() {
        let source = r#"
// fallback: require("./absent-generated")
const doc = 'see require("./absent-generated") for details';
let B = null;
try { B = require("./absent-generated").DATA; } catch {}
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert!(
            !got.contains("import * as"),
            "comment/string mentions must not make the specifier mandatory, got:\n{got}"
        );
    }

    /// `try` must be matched as a keyword, not a suffix — `retry { ... }` does
    /// not make an enclosed require optional.
    #[test]
    fn identifier_ending_in_try_does_not_open_a_try_block() {
        let source = r#"
retry { const m = require("./absent-generated"); }
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert!(
            got.contains(r#"import * as __perry_static_require_0 from "./absent-generated";"#),
            "`retry {{` must not count as a try block, got:\n{got}"
        );
    }

    #[test]
    fn hoists_inline_member_literal_require() {
        let source = r#"
console.log(require("./local").value);
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
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
        let got = transform_static_literal_requires(
            source,
            &compile_packages,
            Path::new("/nonexistent-test-dir"),
        );
        assert!(got.contains(r#"import * as __perry_static_require_0 from "discord.js";"#));
        assert!(got.contains("const Discord = __perry_static_require_0;"));
    }

    #[test]
    fn leaves_disallowed_package_and_builtin_requires() {
        let source = r#"
const Discord = require("discord.js");
const path = require("node:path");
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
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
        let got = transform_static_literal_requires(
            source,
            &compile_packages,
            Path::new("/nonexistent-test-dir"),
        );
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
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert_eq!(got, source);
    }

    #[test]
    fn ignores_require_mentions_in_comments_and_strings() {
        let source = r#"
// const local = require("./local");
const text = 'require("./local")';
"#;
        let got = transform_static_literal_requires(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
        );
        assert_eq!(got, source);
    }
}
