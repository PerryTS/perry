use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use perry_perex::tooling::Regex;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use super::parse_package_specifier;
use crate::commands::compile::cjs_wrap::detect::strip_comments_and_strings;

#[cfg(test)]
fn transform_static_literal_requires(
    source: &str,
    compile_packages: &HashSet<String>,
    module_dir: &Path,
) -> String {
    transform_static_literal_requires_with_bunfs(
        source,
        compile_packages,
        module_dir,
        &module_dir.join("index.js"),
        None,
        false,
    )
}

/// `replace_json_requires`: the module is one the CommonJS wrapper does not
/// rewrite (its `require` is the ambient one, with no per-module shim).
pub(super) fn transform_static_literal_requires_with_bunfs(
    source: &str,
    compile_packages: &HashSet<String>,
    module_dir: &Path,
    source_path: &Path,
    bunfs_root: Option<&Path>,
    replace_json_requires: bool,
) -> String {
    let create_require_aliases = collect_create_require_aliases(source);
    let mut require_aliases =
        collect_create_require_aliases_from_decls(source, &create_require_aliases);
    // `createRequire(import.meta.url)("pkg")` with no binding in between: the
    // call expression itself is the module-relative require.
    for create_alias in &create_require_aliases {
        for call in create_require_call_re(create_alias).find_iter(source) {
            require_aliases.insert(call.as_str().to_string());
        }
    }
    // #9202: `const require = createRequire(import.meta.url)` binds a
    // module-relative alias, not the ambient global — it must not take the
    // bare-global JSON replacement below (whose whole point is that a global
    // `require` has NO module-relative base to resolve against).
    let require_is_create_require_bound = require_aliases.contains("require");
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
            if !is_bare_call(&masked_source, full.start(), full.end()) {
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
    let mut discovered_side_effects = HashSet::new();
    let mut commonjs_targets = HashMap::new();
    let mut replacements = Vec::new();
    let mut next_id = 0usize;
    // One static edge function per target (`__perry_require_static_N`).
    let mut edges: HashMap<std::path::PathBuf, String> = HashMap::new();
    let mut edge_fns = String::new();
    for alias in require_aliases {
        for cap in literal_require_resolve_call_re(&alias).captures_iter(source) {
            let Some(full) = cap.name("call") else {
                continue;
            };
            if !is_bare_call(&masked_source, full.start(), full.end()) {
                continue;
            }
            let specifier = cap.name("spec").map(|m| m.as_str()).unwrap_or_default();
            if let Some(target) = resolve_static_require(module_dir, specifier, bunfs_root) {
                if discovered_side_effects.insert(target.clone()) {
                    let binding = unique_lazy_require_name(source, &mut next_id);
                    imports.push(format!(
                        "import {binding} from {:?};",
                        target.to_string_lossy()
                    ));
                }
            }
        }
        let call_re = literal_require_call_re(&alias);
        for cap in call_re.captures_iter(source) {
            let Some(full) = cap.name("call") else {
                continue;
            };
            if !is_bare_call(&masked_source, full.start(), full.end()) {
                continue;
            }
            let specifier = cap.name("spec").map(|m| m.as_str()).unwrap_or_default();
            // A package Perry implements natively is one module for every
            // loader: the resolver sends the specifier to the native bucket,
            // and the call binds the value a bare `require` literal lowers to
            // (`register_require_namespace_binding`). That is the module's
            // CommonJS default namespace when it has one (`import x from`
            // binds the same view), else its namespace. The package's npm
            // files are not part of the program.
            if is_native_package_specifier(specifier, compile_packages) {
                let temp = imported_specs
                    .entry(specifier.to_string())
                    .or_insert_with(|| {
                        let temp = unique_temp_name(source, &mut next_id);
                        imports.push(if perry_dispatch::has_cjs_default_namespace(specifier) {
                            format!("import {temp} from {:?};", specifier)
                        } else {
                            format!("import * as {temp} from {:?};", specifier)
                        });
                        temp
                    })
                    .clone();
                replacements.push((full.start(), full.end(), temp));
                continue;
            }
            let require_target = resolve_static_require(module_dir, specifier, bunfs_root);
            if should_leave_runtime_require(specifier, compile_packages) {
                if let Some(target) = require_target.as_ref() {
                    if discovered_side_effects.insert(target.clone()) {
                        let binding = unique_lazy_require_name(source, &mut next_id);
                        imports.push(format!(
                            "import {binding} from {:?};",
                            target.to_string_lossy()
                        ));
                    }
                }
                continue;
            }
            // A missing file or an exports-blocked package subpath is a runtime
            // createRequire error, never a hard AOT import. This also covers
            // callbacks declared outside the `try` that eventually invokes
            // them; lexical try detection alone cannot see that control flow.
            if alias != "require"
                && require_target.is_none()
                && (is_relative_or_absolute_specifier(specifier)
                    || package_subpath_is_blocked(module_dir, specifier))
            {
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
            if optional_specs.get(specifier).copied().unwrap_or(false) && require_target.is_none() {
                continue;
            }
            // CJS, JSON, native addons, and custom extensions need the runtime
            // `require` path: it owns Node's cache/record/extension-hook
            // semantics. The side-effect import only makes the statically-known
            // module part of the AOT graph.
            if let Some(target) = require_target.as_ref() {
                let is_native_addon =
                    target.extension().and_then(|extension| extension.to_str()) == Some("node");
                // #10758: an ESM-shaped TypeScript source can still contain a
                // literal `require("./data.json")` (mongodb reads its own
                // package.json this way). Unlike a CJS-wrapped source it has
                // no per-module `require` shim, so leaving the call in place
                // resolves it relative to the process entry instead of the
                // importing module. The JSON loader's default export is the
                // same cached object a static require returns, so use that
                // binding directly for unwrapped ESM modules.
                // #9202: only the AMBIENT global takes this shortcut. A
                // `createRequire`-derived alias already carries the
                // module-relative base into the runtime `require` closure, so
                // its JSON call must go through the runtime loader — the sole
                // owner of `require.cache` (`module_require.rs`'s
                // `cache_exports`/`require_path`). Replacing it with a static
                // import left `require.cache[resolved]` undefined and made the
                // fixture's own `cache!.exports` read throw.
                if replace_json_requires
                    && alias == "require"
                    && !require_is_create_require_bound
                    && target.extension().and_then(|extension| extension.to_str()) == Some("json")
                {
                    let binding = unique_temp_name(source, &mut next_id);
                    imports.push(format!(
                        "import {binding} from {:?};",
                        target.to_string_lossy()
                    ));
                    replacements.push((full.start(), full.end(), binding));
                    continue;
                }
                if !matches!(
                    target.extension().and_then(|e| e.to_str()),
                    Some("ts" | "tsx" | "mts" | "cts" | "js" | "mjs")
                ) || *commonjs_targets.entry(target.clone()).or_insert_with(|| {
                    // A .js/.ts CommonJS target also needs the lazy loader:
                    // an ESM namespace is not its module.exports value, and
                    // hoisting the load would execute conditional requires.
                    std::fs::read_to_string(target).is_ok_and(|source| {
                        crate::commands::compile::cjs_wrap::is_commonjs(&source)
                    })
                }) {
                    if !is_native_addon && discovered_side_effects.insert(target.clone()) {
                        let binding = unique_lazy_require_name(source, &mut next_id);
                        imports.push(format!(
                            "import {binding} from {:?};",
                            target.to_string_lossy()
                        ));
                    }
                    // In a module the wrapper does not rewrite, the ambient
                    // `require` of a compiled CommonJS module is the same
                    // static edge the wrapper writes (design CJS_LINK §2.2):
                    // the census counts a literal require as exact, so in a
                    // closed program nothing publishes into the loader's path
                    // registry and the call must read the target's record.
                    // A program that is not closed keeps the loader call.
                    if replace_json_requires
                        && alias == "require"
                        && !require_is_create_require_bound
                    {
                        if let Some(edge_target) =
                            crate::commands::compile::cjs_wrap::static_edge_target(
                                specifier,
                                source_path,
                                Some(compile_packages),
                                None,
                            )
                        {
                            let edge = edges
                                .entry(edge_target.clone())
                                .or_insert_with(|| {
                                    let (record, edge) = unique_static_edge_names(source, &mut next_id);
                                    imports.push(format!(
                                        "import {{ {} as {record} }} from {:?};",
                                        perry_hir::CJS_RECORD_EXPORT,
                                        edge_target.to_string_lossy()
                                    ));
                                    edge_fns.push_str(&format!(
                                        "\nfunction {edge}() {{ if (__perry_cjs_open()) {{ return require({}); }} else {{ return __perry_cjs_init({record}).exports; }} }}",
                                        serde_json::to_string(specifier).expect("specifier is JSON encodable")
                                    ));
                                    edge
                                })
                                .clone();
                            replacements.push((full.start(), full.end(), format!("{edge}()")));
                        }
                    }
                    continue;
                }
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
    // Function declarations hoist: appended, they shift no source line.
    transformed.push_str(&edge_fns);
    prepend_imports_preserving_shebang(&transformed, &imports)
}

fn is_bare_call(masked_source: &str, start: usize, end: usize) -> bool {
    // The regex excludes an adjacent dot, but whitespace/comments can separate
    // it from the method name. Keep `import.meta . require(...)` and ordinary
    // object methods intact; only identifier calls belong to this transform.
    !masked_source[..start].trim_end().ends_with('.')
        && !masked_source[start..end]
            .bytes()
            .all(|b| b.is_ascii_whitespace())
}

pub(in crate::commands::compile) fn resolve_static_require(
    module_dir: &Path,
    specifier: &str,
    bunfs_root: Option<&Path>,
) -> Option<std::path::PathBuf> {
    if specifier.starts_with(crate::commands::compile::resolve::BUNFS_ROOT_PREFIX) {
        return bunfs_root.and_then(|root| {
            crate::commands::compile::resolve::resolve_bunfs_import_path(specifier, root)
        });
    }
    if is_relative_or_absolute_specifier(specifier) {
        let base = if std::path::Path::new(specifier).is_absolute() {
            std::path::PathBuf::from(specifier)
        } else {
            module_dir.join(specifier)
        };
        return resolve_require_path(&base);
    }

    let (package, subpath) = super::parse_package_specifier(specifier);
    for node_modules in crate::commands::compile::resolve::ancestor_node_modules_dirs(module_dir) {
        let package_dir = node_modules.join(&package);
        if !package_dir.is_dir() {
            continue;
        }
        let package_json = std::fs::read_to_string(package_dir.join("package.json")).ok();
        if let Some(text) = package_json {
            let manifest: PackageManifest = serde_json::from_str(&text).ok()?;
            if let Some(exports) = manifest.exports.as_ref() {
                let key = subpath
                    .as_deref()
                    .map(|s| format!("./{s}"))
                    .unwrap_or_else(|| ".".to_string());
                let target = resolve_require_exports(exports, &key)?;
                return resolve_require_path(&package_dir.join(target));
            }
            if let Some(subpath) = subpath.as_deref() {
                return resolve_require_path(&package_dir.join(subpath));
            }
            if let Some(main) = manifest.main.as_ref().and_then(serde_json::Value::as_str) {
                if let Some(path) = resolve_require_path(&package_dir.join(main)) {
                    return Some(path);
                }
            }
        }
        return resolve_require_path(&package_dir);
    }
    None
}

fn package_subpath_is_blocked(module_dir: &Path, specifier: &str) -> bool {
    if is_relative_or_absolute_specifier(specifier) {
        return false;
    }
    let (package, subpath) = super::parse_package_specifier(specifier);
    for node_modules in crate::commands::compile::resolve::ancestor_node_modules_dirs(module_dir) {
        let package_dir = node_modules.join(&package);
        if !package_dir.is_dir() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(package_dir.join("package.json")) else {
            return false;
        };
        let Ok(manifest) = serde_json::from_str::<PackageManifest>(&text) else {
            return false;
        };
        let Some(exports) = manifest.exports.as_ref() else {
            return false;
        };
        let key = subpath
            .as_deref()
            .map(|path| format!("./{path}"))
            .unwrap_or_else(|| ".".to_string());
        return resolve_require_exports(exports, &key).is_none();
    }
    false
}

fn resolve_require_path(path: &Path) -> Option<std::path::PathBuf> {
    if path.is_file() {
        return path.canonicalize().ok();
    }
    for ext in ["ts", "tsx", "mts", "cts", "js", "json", "node", "cjs"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(".");
        candidate.push(ext);
        let candidate = std::path::PathBuf::from(candidate);
        if candidate.is_file() {
            return candidate.canonicalize().ok();
        }
    }
    if path.is_dir() {
        if let Ok(text) = std::fs::read_to_string(path.join("package.json")) {
            if let Ok(manifest) = serde_json::from_str::<PackageManifest>(&text) {
                if let Some(main) = manifest.main.as_ref().and_then(serde_json::Value::as_str) {
                    if let Some(found) = resolve_require_path(&path.join(main)) {
                        return Some(found);
                    }
                }
            }
        }
        for ext in ["ts", "tsx", "mts", "cts", "js", "json", "node", "cjs"] {
            let candidate = path.join(format!("index.{ext}"));
            if candidate.is_file() {
                return candidate.canonicalize().ok();
            }
        }
    }
    None
}

#[derive(Deserialize)]
struct PackageManifest {
    main: Option<serde_json::Value>,
    exports: Option<OrderedJson>,
}

enum OrderedJson {
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
    Other,
}

impl<'de> Deserialize<'de> for OrderedJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OrderedJsonVisitor;

        impl<'de> Visitor<'de> for OrderedJsonVisitor {
            type Value = OrderedJson;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(OrderedJson::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(OrderedJson::String(value))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(OrderedJson::Array(values))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = map.next_entry()? {
                    values.push(value);
                }
                Ok(OrderedJson::Object(values))
            }

            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(OrderedJson::Other)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(OrderedJson::Other)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(OrderedJson::Other)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(OrderedJson::Other)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(OrderedJson::Other)
            }
        }

        deserializer.deserialize_any(OrderedJsonVisitor)
    }
}

fn resolve_require_exports(exports: &OrderedJson, key: &str) -> Option<String> {
    match exports {
        OrderedJson::String(target) => Some(target.clone()),
        OrderedJson::Array(items) => items
            .iter()
            .find_map(|item| resolve_require_exports(item, key)),
        OrderedJson::Object(map) => {
            if let Some((_, target)) = map.iter().find(|(name, _)| name == key) {
                return resolve_require_exports(target, key);
            }
            for (condition, target) in map {
                if matches!(condition.as_str(), "node" | "require" | "default") {
                    if let Some(resolved) = resolve_require_exports(target, key) {
                        return Some(resolved);
                    }
                }
            }
            None
        }
        _ => None,
    }
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

/// Every name a `createRequire` from `"module"` is reachable under: ESM named,
/// default and namespace imports, CommonJS destructuring and bindings of
/// `require("module")`, and the inline `require("module").createRequire`.
/// A literal require through any of them is the same module-relative require
/// (one specifier, one resolution), so each one must be recognised.
fn collect_create_require_aliases(source: &str) -> HashSet<String> {
    // Only a `createRequire` that is named somewhere yields an alias: skip the
    // regex scans over a source that never names it.
    if !source.contains("createRequire") {
        return HashSet::new();
    }
    const MODULE: &str = r#"['"](?:node:)?module['"]"#;
    const ID: &str = r"[A-Za-z_$][A-Za-z0-9_$]*";
    static IMPORT_RE: OnceLock<Regex> = OnceLock::new();
    let import_re = IMPORT_RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?m)^\s*import\s+(?:(?P<def>{ID})\s*,?\s*)?(?:\*\s*as\s+(?P<ns>{ID})|\{{(?P<specs>[^}}]*)\}})?\s*from\s*{MODULE}"
        ))
        .expect("module import regex")
    });
    static DESTRUCTURE_RE: OnceLock<Regex> = OnceLock::new();
    let destructure_re = DESTRUCTURE_RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?m)^\s*(?:const|let|var)\s*\{{(?P<specs>[^}}]*)\}}\s*=\s*require\s*\(\s*{MODULE}\s*\)"
        ))
        .expect("module destructuring regex")
    });
    static BINDING_RE: OnceLock<Regex> = OnceLock::new();
    let binding_re = BINDING_RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?m)^\s*(?:const|let|var)\s+(?P<ns>{ID})\s*=\s*require\s*\(\s*{MODULE}\s*\)\s*;?\s*$"
        ))
        .expect("module binding regex")
    });
    static INLINE_RE: OnceLock<Regex> = OnceLock::new();
    let inline_re = INLINE_RE.get_or_init(|| {
        Regex::new(&format!(
            r"require\s*\(\s*{MODULE}\s*\)\s*\.\s*createRequire"
        ))
        .expect("inline module createRequire regex")
    });

    let mut aliases = HashSet::new();
    let add_specs = |specs: &str, separator: &str, aliases: &mut HashSet<String>| {
        for part in specs.split(',') {
            let part = part.trim();
            if part == "createRequire" {
                aliases.insert("createRequire".to_string());
                continue;
            }
            if let Some(rest) = part
                .strip_prefix("createRequire")
                .and_then(|rest| rest.trim_start().strip_prefix(separator))
            {
                let alias = rest.trim();
                if is_identifier(alias) {
                    aliases.insert(alias.to_string());
                }
            }
        }
    };
    for cap in import_re.captures_iter(source) {
        // `import type { createRequire } from "module"` binds no value.
        if let Some(default) = cap.name("def").filter(|name| name.as_str() != "type") {
            aliases.insert(format!("{}.createRequire", default.as_str()));
        }
        if let Some(namespace) = cap.name("ns") {
            aliases.insert(format!("{}.createRequire", namespace.as_str()));
        }
        if let Some(specs) = cap.name("specs") {
            add_specs(specs.as_str(), "as ", &mut aliases);
        }
    }
    for cap in destructure_re.captures_iter(source) {
        if let Some(specs) = cap.name("specs") {
            add_specs(specs.as_str(), ":", &mut aliases);
        }
    }
    for cap in binding_re.captures_iter(source) {
        if let Some(namespace) = cap.name("ns") {
            aliases.insert(format!("{}.createRequire", namespace.as_str()));
        }
    }
    for inline in inline_re.find_iter(source) {
        aliases.insert(inline.as_str().to_string());
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

/// The `createRequire` arguments that name this module's own location, so the
/// require they make resolves exactly like this module's literal `require`:
/// its URL or path, or its directory as a URL (`new URL(".", import.meta.url)`).
/// A directory path without a trailing separator (`import.meta.dirname`,
/// `__dirname`) names a file in the parent directory to Node and is not one.
const THIS_MODULE_BASE: &str = r#"(?:import\.meta\.url|import\.meta\.filename|__filename|(?:[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*)?fileURLToPath\s*\(\s*import\.meta\.url\s*\)|new\s+URL\s*\(\s*(?:['"]\.\/?['"]\s*,\s*)?import\.meta\.url\s*\))"#;

/// `<createRequire alias>(<this module>)`.
fn create_require_call_re(create_alias: &str) -> Regex {
    Regex::new(&format!(
        r"{}\s*\(\s*{THIS_MODULE_BASE}\s*\)",
        perry_perex::tooling::escape(create_alias)
    ))
    .expect("createRequire call regex")
}

fn create_require_decl_re(create_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)^\s*(?:const|let|var)\s+(?P<alias>[A-Za-z_$][A-Za-z0-9_$]*)(?:\s*:\s*[^=;]+)?\s*=\s*{}\s*\(\s*{THIS_MODULE_BASE}\s*\)\s*;?"#,
        perry_perex::tooling::escape(create_alias)
    ))
    .expect("createRequire declaration regex")
}

fn literal_require_call_re(require_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)(?:^|[^A-Za-z0-9_$\.])(?P<call>{}\s*\(\s*['"](?P<spec>[^'"]+)['"]\s*\))"#,
        perry_perex::tooling::escape(require_alias)
    ))
    .expect("static require literal call regex")
}

fn literal_require_resolve_call_re(require_alias: &str) -> Regex {
    Regex::new(&format!(
        r#"(?m)(?:^|[^A-Za-z0-9_$\.])(?P<call>{}\.resolve\s*\(\s*['\"](?P<spec>[^'\"]+)['\"]\s*\))"#,
        perry_perex::tooling::escape(require_alias)
    ))
    .expect("static require.resolve literal call regex")
}

/// A package (not a Node builtin) that Perry implements natively and the
/// project did not opt into `perry.compilePackages`. The static resolver
/// sends such a specifier to the native bucket (`resolve.rs`). Builtins stay
/// runtime requires: the runtime loader resolves them by name.
fn is_native_package_specifier(specifier: &str, compile_packages: &HashSet<String>) -> bool {
    if specifier.starts_with("node:") || perry_hir::is_node_builtin_module(specifier) {
        return false;
    }
    let (package_name, _) = parse_package_specifier(specifier);
    perry_hir::is_native_module_specifier(specifier) && !compile_packages.contains(&package_name)
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

/// The record binding (`__perry_cjs_rec_N`, the name the collector and the
/// init order read as an edge) and the edge function of one static edge.
fn unique_static_edge_names(source: &str, next_id: &mut usize) -> (String, String) {
    loop {
        let (record, edge) = (
            format!("__perry_cjs_rec_{}", *next_id),
            format!("__perry_require_static_{}", *next_id),
        );
        *next_id += 1;
        if !source.contains(&record) && !source.contains(&edge) {
            return (record, edge);
        }
    }
}

fn unique_lazy_require_name(source: &str, next_id: &mut usize) -> String {
    loop {
        let name = format!("_lazyreq_static_{}", *next_id);
        *next_id += 1;
        if !source.contains(&name) {
            return name;
        }
    }
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
    fn member_calls_with_whitespace_are_not_bare_requires() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dep.js"), "export const answer = 42;").unwrap();
        for source in [
            "const dep = import.meta . require('./dep.js');",
            "const dep = import.meta. /* gap */ require('./dep.js');",
            "const dep = object . require('./dep.js');",
            "const dep = object . require.resolve('./dep.js');",
            "// require('./dep.js')\nexport {};",
        ] {
            assert_eq!(
                transform_static_literal_requires(source, &HashSet::new(), dir.path()),
                source
            );
        }
    }

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
    fn replaces_resolved_json_require_in_unwrapped_esm_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        let module_dir = dir.path().join("src/cmap/handshake");
        std::fs::create_dir_all(&module_dir).expect("create nested module dir");
        let package_json = dir.path().join("package.json");
        std::fs::write(&package_json, r#"{"version":"7.5.0"}"#).expect("write package.json");
        let source = r#"
import { marker } from "./marker";
export const version = require("../../../package.json").version + marker;
"#;

        let got = transform_static_literal_requires_with_bunfs(
            source,
            &HashSet::new(),
            &module_dir,
            &module_dir.join("index.js"),
            None,
            true,
        );

        assert!(
            got.contains(&format!(
                "import __perry_static_require_0 from {:?};",
                package_json
                    .canonicalize()
                    .expect("canonicalize package.json")
                    .to_string_lossy()
            )),
            "got:\n{got}"
        );
        assert!(got.contains("export const version = __perry_static_require_0.version + marker;"));
        assert!(!got.contains(r#"require("../../../package.json")"#));
    }

    /// #9202: the SAME resolved JSON require, reached through a
    /// `createRequire`-derived alias, must keep its runtime call. The alias's
    /// closure already resolves module-relative, and the runtime loader is the
    /// only owner of `require.cache` — replacing the call with a static import
    /// left `require.cache[resolved]` undefined, so the fixture's own
    /// `cache!.exports` read threw `TypeError`.
    #[test]
    fn leaves_json_require_on_a_create_require_alias_for_the_runtime_loader() {
        let dir = tempfile::tempdir().expect("tempdir");
        let module_dir = dir.path().join("src/cmap/handshake");
        std::fs::create_dir_all(&module_dir).expect("create nested module dir");
        let package_json = dir.path().join("package.json");
        std::fs::write(&package_json, r#"{"version":"7.5.0"}"#).expect("write package.json");
        let source = r#"
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const cached = req.cache[req.resolve("../../../package.json")];
console.log(cached.exports.version, req("../../../package.json").version);
"#;

        let got = transform_static_literal_requires_with_bunfs(
            source,
            &HashSet::new(),
            &module_dir,
            &module_dir.join("index.js"),
            None,
            true,
        );

        assert!(
            got.contains(r#"req("../../../package.json")"#),
            "a createRequire alias must keep its runtime require call, got:\n{got}"
        );
        assert!(
            got.contains(&format!(
                "from {:?};",
                package_json
                    .canonicalize()
                    .expect("canonicalize package.json")
                    .to_string_lossy()
            )),
            "the JSON must still join the AOT graph via a side-effect import, got:\n{got}"
        );
        assert!(
            !got.contains("__perry_static_require_"),
            "no static JSON import may replace the alias call, got:\n{got}"
        );
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
    fn supports_namespace_and_immediate_create_require() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("node_modules/demo-js");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"demo-js","main":"index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            "module.exports = { answer: 42 };\n",
        )
        .unwrap();
        let source = r#"import * as NodeModule from "node:module";
const requireDemo = NodeModule.createRequire(import.meta.url);
const a = requireDemo("demo-js");
const { answer } = NodeModule.createRequire(import.meta.url)("demo-js");
"#;
        let compile_packages: HashSet<String> = ["demo-js".to_string()].into_iter().collect();
        let got = transform_static_literal_requires_with_bunfs(
            source,
            &compile_packages,
            dir.path(),
            &dir.path().join("main.mjs"),
            None,
            false,
        );
        // Both forms make the package part of the AOT graph.
        assert!(got.contains("import "), "got:\n{got}");
        assert!(
            got.contains("demo-js") && got.matches("index.js").count() >= 1,
            "got:\n{got}"
        );
    }

    /// Every way a module obtains `createRequire` from `"module"`, and every
    /// argument that names the module itself, makes a literal require through
    /// it resolve like the module's own literal `require`: the package joins
    /// the program.
    #[test]
    fn every_create_require_form_of_this_module_is_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("node_modules/plain-cjs");
        std::fs::create_dir_all(package.join("lib")).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"plain-cjs","main":"lib/index.js"}"#,
        )
        .unwrap();
        std::fs::write(package.join("lib/index.js"), "exports.answer = 42;\n").unwrap();
        let forms = [
            "import Module from \"node:module\";\nconst req = Module.createRequire(import.meta.url);\nreq(\"plain-cjs\");\n",
            "import Module, { createRequire } from \"module\";\nModule.createRequire(import.meta.url)(\"plain-cjs\");\n",
            "import { createRequire } from \"node:module\";\nconst req = createRequire(new URL(\".\", import.meta.url));\nreq(\"plain-cjs\");\n",
            "import { createRequire } from \"node:module\";\nimport { fileURLToPath } from \"node:url\";\nconst req = createRequire(fileURLToPath(import.meta.url));\nreq(\"plain-cjs\");\n",
            "import { createRequire } from \"node:module\";\nconst req = createRequire(import.meta.filename);\nreq(\"plain-cjs\");\n",
            "const { createRequire } = require(\"module\");\nconst req = createRequire(__filename);\nreq(\"plain-cjs\");\n",
            "const { createRequire: make } = require(\"node:module\");\nmake(__filename)(\"plain-cjs\");\n",
            "const NodeModule = require(\"node:module\");\nconst req = NodeModule.createRequire(__filename);\nreq(\"plain-cjs\");\n",
            "const req = require(\"node:module\").createRequire(__filename);\nreq(\"plain-cjs\");\n",
        ];
        for source in forms {
            let got = transform_static_literal_requires(source, &HashSet::new(), dir.path());
            assert!(
                got.contains("plain-cjs/lib/index.js"),
                "the package must join the program for:\n{source}\ngot:\n{got}"
            );
        }
    }

    /// A `createRequire` whose base is not this module (a directory path
    /// without a trailing separator names a file in its parent to Node)
    /// resolves elsewhere and is left to the runtime.
    #[test]
    fn create_require_of_another_base_is_not_this_module() {
        let dir = tempfile::tempdir().unwrap();
        let source = "import { createRequire } from \"node:module\";\nconst req = createRequire(import.meta.dirname);\nreq(\"plain-cjs\");\n";
        assert_eq!(
            transform_static_literal_requires(source, &HashSet::new(), dir.path()),
            source
        );
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

    /// A `createRequire` call of a package Perry implements natively binds the
    /// same namespace a static import gets; the package's npm files on disk
    /// must not join the program.
    #[test]
    fn create_require_of_native_package_binds_the_native_namespace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let package = dir.path().join("node_modules/node-pty");
        std::fs::create_dir_all(&package).expect("create stub package");
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"node-pty","version":"1.0.0","main":"index.js"}"#,
        )
        .expect("write stub manifest");
        std::fs::write(
            package.join("index.js"),
            "exports.spawn = function () {};\n",
        )
        .expect("write stub entry");
        let source = r#"
import * as NodeModule from "node:module";
const requirePty = NodeModule.createRequire(import.meta.url);
const pty = requirePty("node-pty");
const again = requirePty("node-pty").spawn;
const third = NodeModule.createRequire(import.meta.url)("node-pty");
"#;
        let got = transform_static_literal_requires(source, &HashSet::new(), dir.path());
        assert!(
            got.contains(r#"import __perry_static_require_0 from "node-pty";"#),
            "the call must bind the CommonJS default namespace, got:\n{got}"
        );
        assert!(got.contains("const pty = __perry_static_require_0;"));
        assert!(got.contains("const again = __perry_static_require_0.spawn;"));
        assert!(got.contains("const third = __perry_static_require_0;"));
        assert!(
            !got.contains("node_modules"),
            "the stub package files must not be imported, got:\n{got}"
        );

        // A native package without a distinct CommonJS default namespace
        // binds its namespace.
        let source = r#"
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const bcrypt = req("bcrypt");
"#;
        let got = transform_static_literal_requires(source, &HashSet::new(), dir.path());
        assert!(
            got.contains(r#"import * as __perry_static_require_0 from "bcrypt";"#),
            "got:\n{got}"
        );
        assert!(got.contains("const bcrypt = __perry_static_require_0;"));
    }

    /// Opting the package into `perry.compilePackages` keeps its npm files
    /// (and a Node-API addon they load) as the module.
    #[test]
    fn create_require_of_compiled_native_package_keeps_its_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let package = dir.path().join("node_modules/node-pty");
        std::fs::create_dir_all(&package).expect("create stub package");
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"node-pty","version":"1.0.0","main":"index.js"}"#,
        )
        .expect("write stub manifest");
        std::fs::write(
            package.join("index.js"),
            "exports.spawn = function () {};\n",
        )
        .expect("write stub entry");
        let source = r#"
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const pty = req("node-pty");
"#;
        let mut compile_packages = HashSet::new();
        compile_packages.insert("node-pty".to_string());
        let got = transform_static_literal_requires(source, &compile_packages, dir.path());
        assert!(
            !got.contains(r#"from "node-pty";"#),
            "a compiled package is not the native module, got:\n{got}"
        );
        assert!(
            got.contains("node_modules/node-pty/index.js"),
            "the compiled package's entry joins the program, got:\n{got}"
        );
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
    /// In a module the CommonJS wrapper does not rewrite, a literal `require`
    /// of a compiled CommonJS module is the wrapper's static edge: the record
    /// import and an edge function whose closed arm runs the target's init
    /// and returns its exports. (worker_deferred_require: the call stayed a
    /// loader call, the census counted it exact, the program was closed, the
    /// target never published, and the require threw MODULE_NOT_FOUND.)
    #[test]
    fn a_literal_require_of_a_compiled_commonjs_module_is_a_static_edge() {
        let dir = tempfile::tempdir().unwrap();
        let dom = dir.path().join("node_modules").join("fakedom");
        std::fs::create_dir_all(dom.join("lib")).unwrap();
        std::fs::write(
            dom.join("package.json"),
            r#"{ "name": "fakedom", "main": "./lib" }"#,
        )
        .unwrap();
        std::fs::write(
            dom.join("lib").join("index.js"),
            "exports.parse = (s) => s;",
        )
        .unwrap();
        let source = r#"
function createParser() { var Dom = require("fakedom"); return Dom.parse; }
function again() { return require("fakedom"); }
export default createParser();
"#;
        let mut packages = HashSet::new();
        packages.insert("fakedom".to_string());
        let parser = dir.path().join("parser.es.js");
        let got = transform_static_literal_requires_with_bunfs(
            source,
            &packages,
            dir.path(),
            &parser,
            None,
            true,
        );
        let target = dom.join("lib").join("index.js");
        assert!(
            got.contains(&format!(
                "import {{ __perry_cjs_record as __perry_cjs_rec_1 }} from {:?};",
                target.to_string_lossy()
            )),
            "{got}"
        );
        assert!(
            got.contains("var Dom = __perry_require_static_1();"),
            "{got}"
        );
        assert!(
            got.contains("return __perry_require_static_1(); }"),
            "{got}"
        );
        assert_eq!(
            got.matches("function __perry_require_static_1()").count(),
            1,
            "{got}"
        );
        assert!(
            got.contains(r#"if (__perry_cjs_open()) { return require("fakedom"); } else { return __perry_cjs_init(__perry_cjs_rec_1).exports; }"#),
            "{got}"
        );
        // A wrapped module keeps its call: the wrapper wrote its edge.
        let wrapped = transform_static_literal_requires_with_bunfs(
            source,
            &packages,
            dir.path(),
            &parser,
            None,
            false,
        );
        assert!(!wrapped.contains("__perry_require_static_"), "{wrapped}");
    }

    /// Only a computed specifier is a loader use in a module the wrapper does
    /// not rewrite: the transform leaves the call and the census counts it,
    /// so the program is not closed (as with `createRequire`).
    #[test]
    fn a_require_left_to_the_runtime_loader_is_counted() {
        let source = r#"
function load(name) { return require(name); }
export default load("fakedom");
"#;
        let got = transform_static_literal_requires_with_bunfs(
            source,
            &HashSet::new(),
            Path::new("/nonexistent-test-dir"),
            Path::new("/nonexistent-test-dir/parser.es.js"),
            None,
            true,
        );
        assert_eq!(got, source);
        let census = crate::commands::compile::cjs_wrap::family_syntax::analyze(source);
        assert_eq!(census.module_capability_uses, 1);
        let literal = crate::commands::compile::cjs_wrap::family_syntax::analyze(
            "function load() { return require(\"fakedom\"); }\nexport default load();\n",
        );
        assert_eq!(literal.module_capability_uses, 0);
    }
}
