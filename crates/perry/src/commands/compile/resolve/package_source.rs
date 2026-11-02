//! Where a package's readable source is: source maps, hybrid CJS emit and the TypeScript source entry.

use super::*;

/// Append `.map` to a path's file name (`index.js` → `index.js.map`,
/// `index.d.ts` → `index.d.ts.map`). `Path::with_extension` cannot be used —
/// it would rewrite the trailing component instead (`index.d.ts` →
/// `index.d.map`).
pub(super) fn append_map_extension(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".map");
    PathBuf::from(name)
}

/// Parse a source/declaration map and return the canonical path of its single
/// original source, when that source exists on disk and is a real TypeScript
/// source (`.ts`/`.tsx`/`.mts`/`.cts`, never a `.d.ts`).
///
/// Only a 1:1 emit is honored — a map carrying exactly one `sources` entry.
/// A bundler that folds many inputs into one output lists several `sources`,
/// and compiling any single one in place of the bundle would silently drop the
/// rest, so those are left to the emitted-file path.
pub(super) fn original_source_from_map_file(map_path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(map_path).ok()?;
    let map: serde_json::Value = serde_json::from_str(&content).ok()?;
    let sources = map.get("sources")?.as_array()?;
    if sources.len() != 1 {
        return None;
    }
    let source = sources[0].as_str()?;
    let source_root = map
        .get("sourceRoot")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // `sources` / `sourceRoot` are resolved relative to the map file's own
    // directory (the tsc / Node source-map convention). An absolute `source`
    // (or absolute `sourceRoot`) makes `join` discard the base, as intended.
    let map_dir = map_path.parent()?;
    let relative = if source_root.is_empty() {
        PathBuf::from(source)
    } else {
        Path::new(source_root).join(source)
    };
    // canonicalize both proves the file exists and normalizes the `../` hops a
    // `dist/ → src/` map almost always contains. A `webpack://`-style or other
    // non-filesystem `source` simply fails to canonicalize and is skipped.
    let canonical = map_dir.join(relative).canonicalize().ok()?;
    if is_declaration_file(&canonical) {
        return None;
    }
    let is_ts_source = matches!(
        canonical.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    );
    is_ts_source.then_some(canonical)
}

/// Recover the ORIGINAL TypeScript source that a compiled package entry was
/// emitted from by reading the declaration map (`*.d.ts.map`) or source map
/// (`*.js.map`) written next to it. This is the authoritative pointer to the
/// source — it survives non-`src/` layouts the name-based conventions in
/// [`resolve_package_source_entry`] cannot guess. (Issue #2569 step 5: prefer
/// original TS sources via source maps or declaration maps.)
///
/// The declaration map is consulted first: it exists specifically to link a
/// `.d.ts` back to the `.ts` it describes, so its `sources` are the cleanest
/// path to the original source. The JS source map is the fallback.
pub(super) fn original_source_via_map(entry: &Path) -> Option<PathBuf> {
    if let Some(declaration) = declaration_sidecar_for_implementation(entry) {
        if let Some(source) = original_source_from_map_file(&append_map_extension(&declaration)) {
            return Some(source);
        }
    }
    original_source_from_map_file(&append_map_extension(entry))
}

/// A published CommonJS package can ship a TypeScript input that is not
/// directly executable as a Perry module, in which case Perry should keep
/// the package on its compiled JS emit instead of the raw source (matching
/// what Node actually runs) rather than following a source map / `src/`
/// convention to that source. Two known shapes trigger this, both narrow and
/// evidence-driven rather than a general "prefer JS" default:
///
/// - **ESM-plus-CJS-epilogue hybrid** (#6586): normal ESM declarations for
///   TypeScript plus a top-level `module.exports = ...` interop epilogue.
///   ESM classification leaves `module` unbound, while CJS wrapping would
///   move its `export` declarations inside an IIFE — neither executes.
/// - **Namespace/function declaration merging via `export =`** (#10662):
///   `namespace X { export class Y extends Z {} }` merged onto a same-named
///   `function X() {}` and exported with `export = X` — the shape
///   `agent-base` (an `axios` → `https-proxy-agent` transitive dependency)
///   uses. Perry's HIR lowers the namespace's exported members as static
///   fields against a synthetic class entity that is not the SAME runtime
///   value `export =` ends up exporting, so e.g. `pkg.Agent` reads back as
///   `undefined` and a downstream `class X extends pkg.Agent` throws "Class
///   extends value is not a constructor". Node can't run this non-erasable
///   TS syntax directly either (`--experimental-strip-types` rejects
///   `namespace`/`export =`), so such a package is never executed from its
///   raw `.ts` source in practice — only via its compiled emit.
pub(super) fn is_hybrid_cjs_emit_input(path: &Path) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();

    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().expect("hybrid source cache").get(&path) {
        return *cached;
    }

    let Ok(source) = fs::read_to_string(&path) else {
        return false;
    };
    let stripped = super::super::cjs_wrap::detect::strip_comments_and_strings(&source);
    let hybrid = (super::super::cjs_wrap::detect::has_top_level_esm(&stripped)
        && super::super::cjs_wrap::detect::has_top_level_module_exports_assignment(&stripped))
        || (super::super::cjs_wrap::detect::has_top_level_namespace_or_module_block(&stripped)
            && super::super::cjs_wrap::detect::has_top_level_export_equals(&stripped));
    cache
        .lock()
        .expect("hybrid source cache")
        .insert(path, hybrid);
    hybrid
}

/// If a package's root TypeScript source is a hybrid CJS-emit input, keep the
/// entire package graph on its published JavaScript. Mixing a `dist` root with
/// source-mapped TypeScript subpaths creates duplicate module identities and
/// reintroduces source-only semantics that Node never executes (Ajv's
/// `dist/ajv.js` plus `lib/compile/codegen/index.ts` was the #6585/#6586
/// combination).
pub(super) fn package_entry_requires_cjs_emit(package_dir: &Path) -> bool {
    if let Some(entry) = resolve_package_entry(package_dir, None) {
        if is_js_file(&entry) {
            if original_source_via_map(&entry).is_some_and(|src| is_hybrid_cjs_emit_input(&src)) {
                return true;
            }
        } else if is_hybrid_cjs_emit_input(&entry) {
            return true;
        }
    }

    let src_index = package_dir.join("src").join("index");
    resolve_with_extensions(&src_index)
        .is_some_and(|src| !is_js_file(&src) && is_hybrid_cjs_emit_input(&src))
}

/// A package's TypeScript sources stand in for its published JavaScript
/// only when they are self-contained: every import of every source reachable
/// (by relative imports) from the sources its manifest's JavaScript targets
/// map to resolves, relatively, to a package installed beside it, or to a
/// built-in. Sources written against their monorepo do not run as shipped:
/// `tsconfig` path aliases (`@posthog/core`'s `@/utils`) and unpublished
/// workspace packages the published build bundles (`@supabase/supabase-js`'s
/// `@supabase/tracing`) left those imports unbound and the link failed on
/// symbols named after their bindings. Node runs the JavaScript; so does
/// Perry then, for the whole package (one identity per module, #6585).
/// Type-only statements are erased and not counted.
pub(super) fn package_sources_self_contained(package_dir: &Path) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    let Ok(dir) = fs::canonicalize(package_dir) else {
        return true;
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().expect("source closure cache").get(&dir) {
        return *cached;
    }
    let verdict = sources_resolve(&dir);
    cache
        .lock()
        .expect("source closure cache")
        .insert(dir, verdict);
    verdict
}

pub(super) fn sources_resolve(package_dir: &Path) -> bool {
    let Some(manifest) = fs::read_to_string(package_dir.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return true;
    };
    let mut targets = Vec::new();
    for field in ["main", "module"] {
        if let Some(t) = manifest.get(field).and_then(|v| v.as_str()) {
            targets.push(t.to_string());
        }
    }
    fn leaves(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| leaves(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| leaves(x, out)),
            _ => {}
        }
    }
    if let Some(exports) = manifest.get("exports") {
        leaves(exports, &mut targets);
    }
    let mut queue: Vec<PathBuf> = targets
        .iter()
        .filter(|t| !t.contains('*'))
        .filter_map(|t| resolve_with_extensions(&package_dir.join(t.trim_start_matches("./"))))
        .filter(|entry| is_js_file(entry))
        .filter_map(|entry| original_source_via_map(&entry))
        .collect();
    queue.extend(resolve_with_extensions(
        &package_dir.join("src").join("index"),
    ));
    queue.retain(|source| !is_js_file(source) && !is_declaration_file(source));
    let mut seen: HashSet<PathBuf> = queue.iter().cloned().collect();
    while let Some(file) = queue.pop() {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for specifier in value_import_specifiers(&text) {
            if is_relative_specifier(&specifier) {
                match resolve_relative_import_path(&specifier, &file) {
                    Some(next) => {
                        if !is_js_file(&next)
                            && !is_declaration_file(&next)
                            && seen.insert(next.clone())
                        {
                            queue.push(next);
                        }
                    }
                    None => return false,
                }
                continue;
            }
            let bare = specifier.strip_prefix("node:").unwrap_or(&specifier);
            if specifier.starts_with("node:")
                || perry_hir::is_node_builtin_module(bare)
                || perry_hir::is_bare_prefix_only_builtin(&specifier)
                || specifier == "bun"
                || specifier.starts_with("bun:")
                || specifier.starts_with("perry/")
            {
                continue;
            }
            let (package, _) = parse_package_specifier(&specifier);
            let installed = file.ancestors().any(|dir| {
                package
                    .split('/')
                    .fold(dir.join("node_modules"), |path, part| path.join(part))
                    .join("package.json")
                    .is_file()
            });
            if !installed {
                return false;
            }
        }
    }
    true
}

/// The specifiers of a module's value imports and re-exports (`import ..
/// from`, `export .. from`, `import "x"`, `import("x")`), not its `import
/// type` / `export type` statements.
pub(super) fn value_import_specifiers(text: &str) -> Vec<String> {
    static FROM: OnceLock<Option<perry_perex::tooling::Regex>> = OnceLock::new();
    static BARE: OnceLock<Option<perry_perex::tooling::Regex>> = OnceLock::new();
    let from = FROM.get_or_init(|| {
        perry_perex::tooling::Regex::new(
            r#"\b(?:import|export)(\s+type\b)?[^'";]*?\bfrom\s*['"]([^'"\n]+)['"]"#,
        )
        .ok()
    });
    let bare = BARE.get_or_init(|| {
        perry_perex::tooling::Regex::new(r#"\bimport\s*\(?\s*['"]([^'"\n]+)['"]"#).ok()
    });
    let mut out = Vec::new();
    if let Some(re) = from {
        for cap in re.captures_iter(text) {
            if cap.get(1).is_none() {
                if let Some(m) = cap.get(2) {
                    out.push(m.as_str().to_string());
                }
            }
        }
    }
    if let Some(re) = bare {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                out.push(m.as_str().to_string());
            }
        }
    }
    out
}

/// Resolve package entry preferring TypeScript source over compiled JS output.
/// Used for compile_packages where we want to compile from TS source, not bundled JS.
pub(in crate::commands::compile) fn resolve_package_source_entry(
    package_dir: &Path,
    subpath: Option<&str>,
) -> Option<PathBuf> {
    if package_entry_requires_cjs_emit(package_dir) || !package_sources_self_contained(package_dir)
    {
        return None;
    }

    let normal_entry = resolve_package_entry(package_dir, subpath);

    // #2569 step 5: the most authoritative pointer to a package's original
    // TypeScript source is the declaration/source map its build wrote next to
    // the emitted entry — it records the exact source path even when the
    // layout does not follow the `src/` ⇄ `dist/` naming the heuristics below
    // assume. Consult it for the actual resolved entry first; fall through to
    // the name-based conventions when no usable map is present.
    if let Some(entry) = normal_entry.as_ref() {
        if is_js_file(entry) {
            if let Some(original) = original_source_via_map(entry) {
                if !is_hybrid_cjs_emit_input(&original) {
                    return Some(original);
                }
            }
        }
    }

    // For subpaths, a package.json "exports" entry that names the subpath is
    // authoritative, as in Node and Bun: when it points at TypeScript source,
    // or at an emit whose source mirror exists, that file is the module.
    // `@opencode/tui` maps `./mini/tool` to `src/mini/tool.public.ts` next to
    // an unrelated `src/mini/tool.ts`; guessing `src/<subpath>` first bound
    // importers to the wrong file. The `src/<subpath>` shorthand is only a
    // fallback for a subpath whose exports target is JS with no source
    // mirror, or that exports does not define. Falling back to src/index.ts
    // for a subpath misroutes imports like `@tanstack/router-core/isServer`
    // to the root barrel and leaves callers linked against symbols that the
    // root never exports.
    if let Some(sub) = subpath {
        if package_exports_define(package_dir, &format!("./{sub}")) {
            if let Some(source) = normal_entry
                .clone()
                .and_then(|entry| prefer_ts_source_for_package_entry(package_dir, entry))
            {
                return Some(source);
            }
        }
        let src_path = package_dir.join("src").join(sub);
        if let Some(resolved) = resolve_with_extensions(&src_path) {
            if !is_js_file(&resolved) && !is_hybrid_cjs_emit_input(&resolved) {
                return Some(resolved);
            }
        }

        let normal_entry = normal_entry?;
        return prefer_ts_source_for_package_entry(package_dir, normal_entry);
    }

    // Try src/index.ts (most common TS source entry)
    let src_index = package_dir.join("src").join("index");
    if let Some(resolved) = resolve_with_extensions(&src_index) {
        if !is_js_file(&resolved) && !is_hybrid_cjs_emit_input(&resolved) {
            return Some(resolved);
        }
    }

    // Try using normal entry resolution but prefer TS over JS
    let normal_entry = normal_entry?;
    prefer_ts_source_for_package_entry(package_dir, normal_entry)
}

/// True when the package.json `exports` field of `package_dir` is a subpath
/// map (keys start with `.`) with an entry for `export_key`, exact or by
/// pattern. The sugar forms (a string, an array or a condition object) only
/// define the package root `.`.
pub(super) fn package_exports_define(package_dir: &Path, export_key: &str) -> bool {
    let Ok(content) = fs::read_to_string(package_dir.join("package.json")) else {
        return false;
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(exports) = pkg.get("exports") else {
        return false;
    };
    let is_subpath_map = exports
        .as_object()
        .is_some_and(|map| map.keys().all(|key| key.starts_with('.')));
    is_subpath_map && !resolve_exports_candidates(exports, export_key).is_empty()
}

pub(super) fn prefer_ts_source_for_package_entry(
    package_dir: &Path,
    normal_entry: PathBuf,
) -> Option<PathBuf> {
    if !is_js_file(&normal_entry) {
        return Some(normal_entry);
    }

    // Try native TypeScript equivalents of the JS entry first, in the
    // same preference order used by resolve_with_extensions.
    for ext in ["ts", "tsx", "mts"] {
        let ts_path = normal_entry.with_extension(ext);
        if ts_path.is_file() && !is_hybrid_cjs_emit_input(&ts_path) {
            return Some(ts_path);
        }
    }
    // Check src/ directory mirror of lib/ or dist/ path
    if let Ok(rel) = normal_entry.strip_prefix(package_dir) {
        let rel_str = rel.to_string_lossy();
        if rel_str.starts_with("lib") || rel_str.starts_with("dist") {
            let stripped = if rel_str.starts_with("lib") {
                rel.strip_prefix("lib")
            } else {
                rel.strip_prefix("dist")
            };
            if let Ok(rest) = stripped {
                for ext in ["ts", "tsx", "mts"] {
                    let src_equiv = package_dir.join("src").join(rest).with_extension(ext);
                    if src_equiv.is_file() && !is_hybrid_cjs_emit_input(&src_equiv) {
                        return Some(src_equiv);
                    }
                }
            }
        }
    }

    None
}
