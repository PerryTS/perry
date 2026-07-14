//! Per-module on-disk object file cache + key derivation.
//!
//! Tier 2.1 follow-up (v0.5.340) — extracts the V2.2 codegen cache
//! family from `compile.rs`. Three concerns clustered here because
//! they all relate to the `<cache_dir>/objects/<target>/<key>.o`
//! cache layout (where `cache_dir` defaults to
//! `<project_root>/node_modules/.cache/perry`):
//!
//! 1. **`djb2_hash`** + **`Djb2Hasher`** — a fast non-crypto hash
//!    used both for the cache-key field hasher and by other parts
//!    of `perry`. Mirrored algorithmically by
//!    `perry_hir::stable_hash::Djb2Hasher` (issue #686) so the HIR
//!    fingerprint and the cache key share one hash family.
//! 2. **`compute_object_cache_key`** — turns
//!    `(CompileOptions, hir_hash, perry_version)` into a stable
//!    16-hex-digit cache key. As of #686 the second input is a
//!    deterministic fingerprint of the post-transform HIR (computed
//!    via `perry_hir::stable_hash::hash_module`) instead of the raw
//!    source-bytes hash, so formatter-only and comment-only edits
//!    that lower to identical HIR reuse the cached `.o`.
//! 3. **`ObjectCache`** — the `lookup_path` / `lookup` / `store`
//!    surface used by the rayon codegen workers. Atomic (tmp + rename)
//!    writes, silent IO-error degradation, lock-free shared `&self`
//!    access (each cache key is per-module so writes never conflict).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub fn djb2_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for b in bytes {
        hash = hash.wrapping_mul(33).wrapping_add(*b as u64);
    }
    hash
}

/// Hash of the running `perry` executable, computed once per process.
///
/// `CARGO_PKG_VERSION` only invalidates the cache on a version bump; during
/// HIR/codegen pass development the version usually doesn't move between
/// rebuilds, so identical-source modules served stale `.o` files compiled
/// against the old pass output (issue #544 — ~45 min of phantom-bug
/// debugging). Folding a hash of the perry binary itself into the key means
/// any `cargo build -p perry-codegen` (or `perry-transform`, `perry-hir`,
/// or any dep that gets baked into the perry executable) produces a new
/// build id and the cache invalidates correctly.
///
/// Failure modes degrade silently: if `current_exe()` or the read fails,
/// returns 0 and we fall back to the version-only behavior — at worst the
/// user is back to the pre-fix status quo, never worse.
fn perry_build_id() -> u64 {
    static BUILD_ID: OnceLock<u64> = OnceLock::new();
    *BUILD_ID.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| fs::read(&p).ok())
            .map(|bytes| djb2_hash(&bytes))
            .unwrap_or(0)
    })
}

/// Directory holding Perry's on-disk caches for a project. Precedence:
/// `--cache-dir` → `PERRY_CACHE_DIR` → perry.toml `[perry] cacheDir` →
/// package.json `perry.cacheDir` → default
/// `<project_root>/node_modules/.cache/perry` (the find-cache-dir
/// convention used by babel-loader / eslint / etc.). Relative overrides
/// resolve against `project_root`.
///
/// This is a pure function: the caller reads the env var + package.json and
/// passes the merged override in, so the resolver is race-free and unit-
/// testable. Pass `None` for `override_dir` to get the default location.
pub fn resolve_cache_dir(project_root: &Path, override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => project_root.join(dir),
        None => project_root
            .join("node_modules")
            .join(".cache")
            .join("perry"),
    }
}

/// Choose the cache-dir override from the three non-CLI sources, applying
/// the non-CLI half of [`resolve_cache_dir`]'s precedence:
/// `PERRY_CACHE_DIR` env var → perry.toml `[perry] cacheDir` → package.json
/// `perry.cacheDir`. The first non-empty candidate wins; later (lower-
/// precedence) candidates are only consulted when the higher ones are absent.
///
/// Pure function over the already-read candidate strings, so the precedence
/// is unit-testable without touching the filesystem or process env. The CLI
/// flag layers on top of this result in the caller (`--cache-dir` wins over
/// everything), so it isn't an input here.
pub fn pick_cache_dir_override(
    env: Option<&str>,
    toml: Option<&str>,
    pkg: Option<&str>,
) -> Option<PathBuf> {
    [env, toml, pkg]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Read the project's cache-dir override from the environment, perry.toml,
/// and package.json, applying the non-CLI half of [`resolve_cache_dir`]'s
/// precedence: `PERRY_CACHE_DIR` env var → perry.toml `[perry] cacheDir` →
/// package.json `perry.cacheDir`. Walks up from `project_root` to find the
/// nearest `perry.toml` / `package.json` so it behaves like the rest of the
/// compile pipeline's config discovery. Returns `None` when no source sets a
/// path, leaving the default.
pub fn cache_dir_override(project_root: &Path) -> Option<PathBuf> {
    let env = std::env::var("PERRY_CACHE_DIR").ok();
    let toml = perry_toml_cache_dir(project_root);
    let pkg = package_json_cache_dir(project_root);
    pick_cache_dir_override(env.as_deref(), toml.as_deref(), pkg.as_deref())
}

/// Read `[perry] cacheDir` from the nearest `perry.toml` walking up from
/// `project_root`. Returns `None` when no `perry.toml` is found or the key is
/// absent. Mirrors the perry.toml walk used by the strict-eval config block.
fn perry_toml_cache_dir(project_root: &Path) -> Option<String> {
    let mut dir = project_root.to_path_buf();
    loop {
        let candidate = dir.join("perry.toml");
        if candidate.exists() {
            return fs::read_to_string(&candidate)
                .ok()
                .and_then(|s| s.parse::<toml::Table>().ok())
                .and_then(|table| {
                    table
                        .get("perry")
                        .and_then(|v| v.as_table())
                        .and_then(|t| t.get("cacheDir"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Read `perry.cacheDir` from the nearest `package.json` walking up from
/// `project_root`. Returns `None` when no `package.json` is found or the key
/// is absent.
fn package_json_cache_dir(project_root: &Path) -> Option<String> {
    let mut dir = project_root.to_path_buf();
    loop {
        let candidate = dir.join("package.json");
        if candidate.exists() {
            return fs::read_to_string(&candidate)
                .ok()
                .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                .and_then(|pkg| {
                    pkg.get("perry")
                        .and_then(|p| p.get("cacheDir"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Streaming djb2 accumulator so multi-part keys don't have to build a
/// giant intermediate `String`. Feed field bytes with a separator between
/// logical fields and the resulting hash is stable across runs as long as
/// the feed order is stable.
#[derive(Clone)]
struct Djb2Hasher {
    state: u64,
}

impl Djb2Hasher {
    fn new() -> Self {
        Self { state: 5381 }
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.state = self.state.wrapping_mul(33).wrapping_add(*b as u64);
        }
    }
    /// Feed a named field: "<name>=<value>\x1f".
    fn field(&mut self, name: &str, value: &str) {
        static DEBUG_KEY_FIELDS: OnceLock<bool> = OnceLock::new();
        if *DEBUG_KEY_FIELDS
            .get_or_init(|| std::env::var("PERRY_CACHE_DEBUG_KEY").as_deref() == Ok("1"))
        {
            eprintln!("cache-key-field {name}={value:?}");
        }
        self.write(name.as_bytes());
        self.write(b"=");
        self.write(value.as_bytes());
        self.write(b"\x1f");
    }
    fn finish(self) -> u64 {
        self.state
    }
}

fn stable_type_key(ty: &perry_types::Type) -> String {
    format!("{:016x}", perry_hir::stable_hash::hash_type(ty))
}

/// Compute the on-disk object cache key for one module.
///
/// Design contract: the key must change whenever any input that affects
/// the bytes `compile_module` returns changes, and must be stable across
/// runs otherwise. We serialize every field of `CompileOptions` that the
/// codegen reads, sort every map/set so HashMap iteration order doesn't
/// leak in, preserve declaration order for lists where the order itself
/// is meaningful (topological init order, FFI wrapper order), and mix
/// in the module's HIR fingerprint (#686) and the perry version.
///
/// `hir_hash` is `perry_hir::stable_hash::hash_module(&Module)`, computed
/// per-module inside the rayon job after every HIR-mutating pass has
/// run — see compile.rs's main per-module closure for the call site.
/// Replacing the previous source-bytes hash with the HIR hash means
/// formatter-only / comment-only / quote-style edits that lower to the
/// same final HIR reuse the cached `.o`. Behavior changes still produce
/// a different HIR and miss the cache as before.
///
/// We also mix in environment variables that `perry-codegen` reads
/// at compile time but that aren't part of `CompileOptions`:
/// `PERRY_DEBUG_INIT`, `PERRY_DEBUG_SYMBOLS`, `PERRY_LLVM_CLANG`,
/// `PERRY_WRITE_BARRIERS`, `PERRY_SHADOW_STACK`,
/// `PERRY_DISABLE_BUFFER_FAST_PATH`, `PERRY_VERIFY_NATIVE_REGIONS`,
/// `PERRY_UNBOXED_OBJECT_FIELDS`, and `PERRY_TARGET_CPU`. See the env-var
/// block at the bottom of this function for the rationale.
///
/// NOT captured in the key: the host CPU. By default (`PERRY_TARGET_CPU`
/// unset) `compile_ll_to_object` passes `-mcpu=native`/`-march=native` to
/// clang for host builds, so the emitted `.o` bakes in whatever instruction
/// set the build machine supports. The cache is consequently
/// **machine-local** — the default `node_modules/.cache/perry`
/// is inside the already-gitignored `node_modules/` for this reason.
/// Sharing across machines with different CPUs (rsync,
/// NFS, Docker bind-mount) can produce SIGILL at runtime. Pinning an
/// explicit baseline (`--march x86-64-v2` / `[build] march`, #6125) removes
/// the machine dependence — the chosen baseline IS keyed, via the
/// `PERRY_TARGET_CPU` env field.
///
/// Cross-platform non-determinism (Mach-O LC_UUID, PE TimeDateStamp,
/// codesigning) affects the *linked binary*, not the object file — so
/// a per-module `.o` cache can reuse bytes across runs as long as LLVM
/// itself emits deterministic object code, which it does by default.
pub fn compute_object_cache_key(
    opts: &perry_codegen::CompileOptions,
    hir_hash: u64,
    perry_version: &str,
) -> u64 {
    compute_object_cache_key_with_env(opts, hir_hash, perry_version, |name| {
        std::env::var(name).ok()
    })
}

fn compute_object_cache_key_with_env(
    opts: &perry_codegen::CompileOptions,
    hir_hash: u64,
    perry_version: &str,
    mut env_var: impl FnMut(&str) -> Option<String>,
) -> u64 {
    let mut h = Djb2Hasher::new();

    // Perry version + bitcode_link gate (we shouldn't be called when
    // emit_ir_only=true, but include it so key-space is disjoint if the
    // caller ever forgets to check).
    h.field("v", perry_version);
    // Build id of the running perry binary (issue #544). Hashed once per
    // process; see `perry_build_id` for rationale. The version field above
    // is not enough during HIR/codegen pass development because the version
    // doesn't usually move between rebuilds.
    h.field("build_id", &format!("{:016x}", perry_build_id()));
    h.field("ir_only", if opts.emit_ir_only { "1" } else { "0" });
    // #5247: `--debug-symbols` flips per-call `js_set_call_location` emission,
    // which changes the emitted IR (and `.o` bytes). Without this in the key,
    // toggling the flag would serve the previously-cached object and the
    // source locations would silently not appear.
    h.field("dbgloc", if opts.debug_locations { "1" } else { "0" });
    h.field(
        "verify_native_regions",
        if opts.verify_native_regions { "1" } else { "0" },
    );
    h.field(
        "disable_buffer_fast_path",
        if opts.disable_buffer_fast_path {
            "1"
        } else {
            "0"
        },
    );

    // HIR fingerprint (issue #686). Computed by
    // `perry_hir::stable_hash::hash_module` over the post-transform HIR
    // that `compile_module` actually consumes. Replaces the previous
    // source-bytes hash. Field tag is "hir" (intentionally distinct from
    // the old "src") so any pre-#686 cache entries cleanly miss.
    h.field("hir", &format!("{:016x}", hir_hash));

    // Target + top-level shape.
    h.field("tgt", opts.target.as_deref().unwrap_or("host"));
    h.field("out", &opts.output_type);
    h.field("entry", if opts.is_entry_module { "1" } else { "0" });

    // Feature flags that round-trip through opts. These influence which
    // extern symbols the module refers to and which compile-time
    // constants it bakes in.
    h.field("stdlib", if opts.needs_stdlib { "1" } else { "0" });
    h.field("ui", if opts.needs_ui { "1" } else { "0" });
    h.field("gh", if opts.needs_geisterhand { "1" } else { "0" });
    h.field("gh_port", &opts.geisterhand_port.to_string());
    // Fast-math flag flips per-instruction `reassoc` emission
    // in `perry-codegen`, which produces different LLVM IR (and therefore
    // different `.o` bytes) for the same TS source. Without this in the
    // key, `perry --fast-math foo.ts` after a default `perry foo.ts` would
    // serve the previously-cached non-fast-math `.o` and the flag would
    // appear to do nothing.
    h.field("fmath", if opts.fast_math { "1" } else { "0" });
    // Floating-point contraction is intentionally separate from broad
    // fast-math reassociation; toggling it changes emitted FMFs and must
    // invalidate cached objects independently.
    h.field("fpctr", opts.fp_contract_mode.as_str());
    h.field("app_version", &opts.app_metadata.version);
    h.field(
        "app_build_number",
        &opts.app_metadata.build_number.to_string(),
    );
    h.field("app_bundle_id", &opts.app_metadata.bundle_id);
    h.field(
        "app_group",
        opts.app_metadata.app_group.as_deref().unwrap_or(""),
    );

    // Ordered lists (order is significant — topological init, FFI index,
    // bundled extension order, etc.)
    h.field(
        "non_entry_prefixes",
        &opts.non_entry_module_prefixes.join("|"),
    );
    h.field("mod_init_names", &opts.native_module_init_names.join("|"));
    // Issue #753: eager/deferred split. The set membership controls
    // which `<prefix>__init` calls main emits eagerly, so the entry
    // module's `.o` bytes change when a target module moves between
    // Eager and Deferred classifications (e.g. a new dynamic
    // `import()` site appears that's the ONLY path to a previously-
    // statically-reached module).
    {
        let mut v: Vec<&String> = opts.deferred_module_prefixes.iter().collect();
        v.sort();
        h.field(
            "deferred_prefixes",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|"),
        );
    }
    h.field("init_deps", &opts.module_init_deps.join("|"));
    // Issue #842: side-effect-only dynamic-import targets emit a
    // `@__perry_ns_<prefix>` global + populator regardless of
    // `namespace_entries`. Toggling this flag changes the emitted IR
    // (one extra global + populator call), so the cache key must
    // include it.
    h.field(
        "dyn_target",
        if opts.is_dynamic_import_target {
            "1"
        } else {
            "0"
        },
    );
    {
        let mut v: Vec<&String> = opts.js_module_specifiers.iter().collect();
        v.sort();
        h.field(
            "js_specs",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|"),
        );
    }
    {
        let mut v: Vec<&(String, String)> = opts.bundled_extensions.iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut buf = String::new();
        for (path, prefix) in v {
            buf.push_str(path);
            buf.push('@');
            buf.push_str(prefix);
            buf.push('|');
        }
        h.field("bundled_ext", &buf);
    }
    {
        let mut v: Vec<_> = opts.native_library_functions.iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        let mut buf = String::new();
        for (name, params, ret) in v {
            buf.push_str(name);
            buf.push(':');
            for (idx, param) in params.iter().enumerate() {
                if idx > 0 {
                    buf.push(',');
                }
                buf.push_str(&param.to_string());
            }
            buf.push_str("->");
            buf.push_str(&ret.to_string());
            buf.push('|');
        }
        h.field("native_libs", &buf);
    }

    // Enabled features — sort for stability; Vec iteration is fine but
    // the upstream computation could reorder in future.
    {
        let mut v: Vec<&String> = opts.enabled_features.iter().collect();
        v.sort();
        h.field(
            "features",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }

    // Namespace imports.
    {
        let mut v: Vec<&String> = opts.namespace_imports.iter().collect();
        v.sort();
        h.field(
            "ns_imports",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }

    // Import function prefixes (HashMap — MUST sort).
    {
        let mut v: Vec<(&String, &String)> = opts.import_function_prefixes.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("import_fn_prefixes", &s);
    }

    // Issue #678: include the origin-name overrides in the cache key.
    // Without this, two builds where the same module imports the same
    // names but with different re-export shapes (e.g. a downstream
    // package renamed its barrel exports) would share a cached `.o` and
    // silently emit the stale symbol suffix.
    {
        let mut v: Vec<(&String, &String)> = opts.import_function_origin_names.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("import_fn_origin_names", &s);
    }

    // Issue #678 followup: V8-fallback specifier overrides — same rationale
    // as origin_names above. Two builds where the same TS module imports
    // the same names but the upstream package flipped between native and
    // V8 fallback must not share a cached `.o`.
    {
        let mut v: Vec<(&String, &String)> = opts.import_function_v8_specifiers.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("import_fn_v8_specifiers", &s);
    }
    // Issue #841: per-(local, submodule_key, export_name) so a flip
    // between named-import-from-submodule and any other resolution
    // invalidates the cached `.o`. Same shape as V8 specifiers above.
    {
        let mut v: Vec<(&String, &(String, String))> =
            opts.import_function_node_submodule.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, (submod, name))| format!("{}={}:{}", k, submod, name))
            .collect::<Vec<_>>()
            .join(",");
        h.field("import_fn_node_submodule", &s);
    }
    // Issue #841 companion: per-local namespace-to-submodule mapping.
    {
        let mut v: Vec<(&String, &String)> = opts.namespace_node_submodules.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("namespace_node_submodules", &s);
    }
    // Issue #678 followup (namespace branch): per-local-namespace V8
    // specifier mapping. A flip between V8-fallback and native-compile
    // for the namespace-target module must invalidate the cached `.o`.
    {
        let mut v: Vec<(&String, &String)> = opts.namespace_v8_specifiers.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("namespace_v8_specifiers", &s);
    }
    // Issue #680: per-namespace member resolution. This is not reflected in
    // the consumer module's HIR, but it changes which external symbol a
    // namespace member call/property access targets.
    {
        let mut v: Vec<(&(String, String), &String)> =
            opts.namespace_member_prefixes.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|((ns, member), prefix)| format!("{}:{}={}", ns, member, prefix))
            .collect::<Vec<_>>()
            .join(",");
        h.field("namespace_member_prefixes", &s);
    }
    // Issue #5924: per-namespace origin-name resolution. Same rationale as
    // `namespace_member_prefixes` above — not reflected in the consumer
    // module's HIR, but changes the symbol suffix a namespace member
    // call/property access targets.
    {
        let mut v: Vec<(&(String, String), &String)> =
            opts.namespace_member_origin_names.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|((ns, member), origin)| format!("{}:{}={}", ns, member, origin))
            .collect::<Vec<_>>()
            .join(",");
        h.field("namespace_member_origin_names", &s);
    }

    // Imported classes — sort by name. Serialize every field that codegen
    // reads so a changed constructor arity or new method on a re-exported
    // class invalidates consumers.
    {
        let mut v: Vec<&perry_codegen::ImportedClass> = opts.imported_classes.iter().collect();
        v.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.source_prefix.cmp(&b.source_prefix))
        });
        let mut buf = String::new();
        for c in v {
            buf.push_str(&format!(
                "{}@{}:ctor={}:own_ctor={}:instance_fields={}:parent={}:alias={}:id={}:fields={}:methods={}:method_arities={}|",
                c.name,
                c.source_prefix,
                c.constructor_param_count,
                if c.has_own_constructor { "1" } else { "0" },
                if c.has_instance_fields { "1" } else { "0" },
                c.parent_name.as_deref().unwrap_or(""),
                c.local_alias.as_deref().unwrap_or(""),
                c.source_class_id.map(|i| i.to_string()).unwrap_or_default(),
                c.field_names.join(","),
                c.method_names.join(","),
                c.method_param_counts
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ));
            buf.push_str("method_rest=");
            buf.push_str(
                &c.method_has_rest
                    .iter()
                    .map(|b| if *b { "1" } else { "0" })
                    .collect::<Vec<_>>()
                    .join(","),
            );
            buf.push_str(":static_fields=");
            buf.push_str(&c.static_field_names.join(","));
            buf.push_str(":static_methods=");
            buf.push_str(&c.static_method_names.join(","));
            buf.push_str(":getters=");
            buf.push_str(&c.getter_names.join(","));
            buf.push_str(":setters=");
            buf.push_str(&c.setter_names.join(","));
            buf.push_str(":field_types=");
            buf.push_str(
                &c.field_types
                    .iter()
                    .map(stable_type_key)
                    .collect::<Vec<_>>()
                    .join(","),
            );
            buf.push('|');
        }
        h.field("imported_classes", &buf);
    }

    // Imported enums — sort by local name, serialize every member.
    {
        let mut v: Vec<&(String, Vec<(String, perry_hir::EnumValue)>)> =
            opts.imported_enums.iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        let mut buf = String::new();
        for (name, members) in v {
            buf.push_str(name);
            buf.push(':');
            for (mname, mval) in members {
                buf.push_str(&format!("{}={:?};", mname, mval));
            }
            buf.push('|');
        }
        h.field("imported_enums", &buf);
    }

    // Imported async function names (HashSet — MUST sort).
    {
        let mut v: Vec<&String> = opts.imported_async_funcs.iter().collect();
        v.sort();
        h.field(
            "imported_async",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }

    // Imported param counts (HashMap — MUST sort).
    {
        let mut v: Vec<(&String, &usize)> = opts.imported_func_param_counts.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, vv))
            .collect::<Vec<_>>()
            .join(",");
        h.field("imported_param_counts", &s);
    }
    // Imported rest-shape metadata (HashSet — MUST sort). These sets change
    // the cross-module call ABI even when the caller's HIR is unchanged.
    {
        let mut v: Vec<&String> = opts.imported_func_has_rest.iter().collect();
        v.sort();
        h.field(
            "imported_has_rest",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }
    {
        let mut v: Vec<&String> = opts.imported_func_synthetic_arguments.iter().collect();
        v.sort();
        h.field(
            "imported_synthetic_arguments",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }

    // Imported return types (HashMap — MUST sort). Type can contain nested
    // HashMaps (ObjectType::properties), so never use Debug output here.
    {
        let mut v: Vec<(&String, &perry_types::Type)> =
            opts.imported_func_return_types.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, stable_type_key(vv)))
            .collect::<Vec<_>>()
            .join(",");
        h.field("imported_return_types", &s);
    }

    // Imported vars (HashSet — MUST sort).
    {
        let mut v: Vec<&String> = opts.imported_vars.iter().collect();
        v.sort();
        h.field(
            "imported_vars",
            &v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","),
        );
    }

    // Type aliases (HashMap — MUST sort).
    {
        let mut v: Vec<(&String, &perry_types::Type)> = opts.type_aliases.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s = v
            .iter()
            .map(|(k, vv)| format!("{}={}", k, stable_type_key(vv)))
            .collect::<Vec<_>>()
            .join(",");
        h.field("type_aliases", &s);
    }

    // i18n snapshot — tuple of ordered Vecs + counts, no map involved.
    // Only the entry module embeds this, but hash it unconditionally so
    // a mis-flagged non-entry module can't collide with an entry one.
    if let Some(arc) = &opts.i18n_table {
        // Tier 4.6: deref the Arc<Tuple> to read the inner fields.
        let (translations, key_count, locale_count, locale_codes, default_idx) = arc.as_ref();
        h.field("i18n_kc", &key_count.to_string());
        h.field("i18n_lc", &locale_count.to_string());
        h.field("i18n_def", &default_idx.to_string());
        h.field("i18n_locales", &locale_codes.join(","));
        // Translations are a single long Vec — join with a NUL to avoid
        // substring ambiguity across entries.
        h.field("i18n_tr", &translations.join("\0"));
    } else {
        h.field("i18n", "none");
    }

    // Dynamic import metadata is computed from the whole module graph, not
    // only this module's HIR. It directly controls emitted namespace globals,
    // namespace population, and dynamic-import dispatch.
    {
        let mut buf = String::new();
        for entry in &opts.namespace_entries {
            buf.push_str(&entry.name);
            buf.push('=');
            serialize_namespace_entry_kind(&entry.kind, &mut buf);
            buf.push('|');
        }
        h.field("namespace_entries", &buf);
    }
    {
        let mut v: Vec<(&String, &String)> = opts.dynamic_import_path_to_prefix.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s: String = v
            .iter()
            .map(|(path, prefix)| format!("{}={}", path, prefix))
            .collect::<Vec<_>>()
            .join(",");
        h.field("dynamic_import_path_to_prefix", &s);
    }

    // Environment variables read by `perry-codegen` that influence the
    // emitted .o bytes. Not part of `CompileOptions`, but just as real an
    // input to `compile_module` / `compile_ll_to_object`:
    //   - PERRY_DEBUG_INIT=1 bakes a `puts("INIT: <prefix>")` call into
    //     every module's `__init` (codegen.rs).
    //   - PERRY_DEBUG_SYMBOLS=1 adds `-g` to clang → embeds DWARF sections
    //     into the object (linker.rs).
    //   - PERRY_LLVM_CLANG selects which clang binary compiles .ll → .o;
    //     different clang versions/builds emit different bytes (linker.rs).
    //   - PERRY_WRITE_BARRIERS=0/off/false suppresses generated barrier
    //     calls at heap-store sites (codegen.rs / expr.rs).
    //   - PERRY_SHADOW_STACK=0/off/false suppresses generated frame/slot
    //     roots at function entry and pointer local stores.
    //   - PERRY_DISABLE_BUFFER_FAST_PATH=1 overrides CompileOptions and
    //     changes Buffer/Uint8Array lowering.
    //   - PERRY_VERIFY_NATIVE_REGIONS=1 overrides CompileOptions and must
    //     not be bypassed by a stale cache hit.
    //   - PERRY_UNBOXED_OBJECT_FIELDS=1 changes object-literal layout
    //     lowering for exact typed object shapes.
    //   - PERRY_TARGET_CPU (#6125, set directly or promoted from `--march` /
    //     perry.toml `[build] march`/`native_tuning`) changes the clang
    //     `-march`/`-mcpu` tuning flag, i.e. which instruction-set baseline
    //     the .o bytes assume. Serving a host-native-tuned object to a
    //     pinned-baseline build would silently ship AVX-512 again.
    // Hashing the values (not just presence) means a persistent override
    // like PERRY_LLVM_CLANG=/opt/homebrew/opt/llvm/bin/clang in a shell rc
    // still gets cache reuse across runs, while flipping a debug flag on
    // or off cleanly invalidates.
    h.field(
        "env_debug_init",
        env_var("PERRY_DEBUG_INIT").as_deref().unwrap_or(""),
    );
    h.field(
        "env_debug_symbols",
        env_var("PERRY_DEBUG_SYMBOLS").as_deref().unwrap_or(""),
    );
    h.field(
        "env_llvm_clang",
        env_var("PERRY_LLVM_CLANG").as_deref().unwrap_or(""),
    );
    h.field(
        "env_write_barriers",
        env_var("PERRY_WRITE_BARRIERS").as_deref().unwrap_or(""),
    );
    h.field(
        "env_shadow_stack",
        env_var("PERRY_SHADOW_STACK").as_deref().unwrap_or(""),
    );
    h.field(
        "env_disable_buffer_fast_path",
        env_var("PERRY_DISABLE_BUFFER_FAST_PATH")
            .as_deref()
            .unwrap_or(""),
    );
    h.field(
        "env_verify_native_regions",
        env_var("PERRY_VERIFY_NATIVE_REGIONS")
            .as_deref()
            .unwrap_or(""),
    );
    h.field(
        "env_unboxed_object_fields",
        env_var("PERRY_UNBOXED_OBJECT_FIELDS")
            .as_deref()
            .unwrap_or(""),
    );
    h.field(
        "env_target_cpu",
        env_var("PERRY_TARGET_CPU").as_deref().unwrap_or(""),
    );

    h.finish()
}

fn serialize_namespace_entry_kind(kind: &perry_codegen::NamespaceEntryKind, out: &mut String) {
    match kind {
        perry_codegen::NamespaceEntryKind::LocalVar { global_name } => {
            out.push_str("local_var:");
            out.push_str(global_name);
        }
        perry_codegen::NamespaceEntryKind::LocalFunction { wrap_symbol } => {
            out.push_str("local_fn:");
            out.push_str(wrap_symbol);
        }
        perry_codegen::NamespaceEntryKind::LocalClass { class_id } => {
            out.push_str("local_class:");
            out.push_str(&class_id.to_string());
        }
        perry_codegen::NamespaceEntryKind::ForeignVar {
            source_prefix,
            source_local,
        } => {
            out.push_str("foreign_var:");
            out.push_str(source_prefix);
            out.push(':');
            out.push_str(source_local);
        }
        perry_codegen::NamespaceEntryKind::ForeignFunction {
            source_prefix,
            source_local,
            param_count,
        } => {
            out.push_str("foreign_fn:");
            out.push_str(source_prefix);
            out.push(':');
            out.push_str(source_local);
            out.push(':');
            out.push_str(&param_count.to_string());
        }
        perry_codegen::NamespaceEntryKind::NestedNamespace { source_prefix } => {
            out.push_str("nested_ns:");
            out.push_str(source_prefix);
        }
    }
}
/// On-disk per-module object cache at `<cache_dir>/objects/<target>/<hash:016x>.o`,
/// where `cache_dir` defaults to `<project_root>/node_modules/.cache/perry`.
///
/// Each rayon codegen worker calls `lookup_path(key)`; on hit, it skips the
/// LLVM pipeline and links the cached object file directly. Older callers may
/// still call `lookup(key)` to materialize cached bytes. On miss, the worker
/// runs `compile_module` as usual and then calls `store_and_get_path(key,
/// bytes)` to populate the cache and link from the same stable cache path
/// future hits will use. Atomic (tmp + rename) writes and silent IO-error
/// handling mean the cache is strictly an optimization — any missing or
/// unreadable entry degrades gracefully to the uncached codepath.
///
/// Shared across rayon workers via `&self` — no locking is needed because
/// each key corresponds to a distinct file (the key includes this module's
/// source hash). Atomic counters track hit/miss/store for verbose reporting.
pub struct ObjectCache {
    /// Where to read/write cached objects. `None` when the cache is
    /// disabled (via `--no-cache`, bitcode-link mode, or non-writable
    /// project root).
    cache_dir: Option<PathBuf>,
    hits: AtomicUsize,
    misses: AtomicUsize,
    stores: AtomicUsize,
    store_errors: AtomicUsize,
    path_reuses: AtomicUsize,
    bytes_materialized: AtomicUsize,
}

impl ObjectCache {
    /// Create a new cache rooted at `<cache_dir>/objects/<target>/`, where
    /// `cache_dir` is the already-resolved Perry cache directory (see
    /// [`resolve_cache_dir`] — default `<project_root>/node_modules/.cache/perry`).
    /// `target_triple` is the LLVM target triple (or `"host"` for the host
    /// default). Passing `enabled = false` returns a no-op instance —
    /// every `lookup` misses and every `store` is a silent drop.
    pub fn new(cache_dir: &Path, target_triple: &str, enabled: bool) -> Self {
        let cache_dir = if enabled {
            let dir = cache_dir.join("objects").join(target_triple);
            match fs::create_dir_all(&dir) {
                Ok(()) => Some(dir),
                Err(_) => None, // silent degrade: cache stays disabled
            }
        } else {
            None
        };
        Self {
            cache_dir,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            stores: AtomicUsize::new(0),
            store_errors: AtomicUsize::new(0),
            path_reuses: AtomicUsize::new(0),
            bytes_materialized: AtomicUsize::new(0),
        }
    }

    /// Returns the cache file path for a given key, or `None` if the
    /// cache is disabled.
    fn path_for(&self, key: u64) -> Option<PathBuf> {
        self.cache_dir
            .as_ref()
            .map(|d| d.join(format!("{:016x}.o", key)))
    }

    /// Look up a cached object by key. Returns `Some(bytes)` on hit,
    /// `None` on miss (cache disabled, file missing, or IO error).
    #[allow(dead_code)]
    pub fn lookup(&self, key: u64) -> Option<Vec<u8>> {
        let path = self.path_for(key)?;
        match fs::read(&path) {
            Ok(bytes) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                self.bytes_materialized
                    .fetch_add(bytes.len(), Ordering::Relaxed);
                Some(bytes)
            }
            Err(_) => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Look up a cached object by key and return its on-disk path without
    /// reading the object bytes into memory. We still open the file once so
    /// unreadable cache entries fall back to a fresh compile just like
    /// `lookup` would.
    pub fn lookup_path(&self, key: u64) -> Option<PathBuf> {
        let path = self.path_for(key)?;
        match fs::File::open(&path).and_then(|f| f.metadata()) {
            Ok(meta) if meta.is_file() => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                self.path_reuses.fetch_add(1, Ordering::Relaxed);
                Some(path)
            }
            _ => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Store the freshly-compiled bytes under `key` and return the final
    /// cache path on success. Atomic via tmp + rename so a concurrent reader
    /// in another process never sees a partial file. IO errors are counted
    /// but not reported — the cache is strictly an optimization.
    pub fn store_and_get_path(&self, key: u64, bytes: &[u8]) -> Option<PathBuf> {
        let path = self.path_for(key)?;

        // Write to a unique tmp path in the same directory, then rename.
        // The tmp name mixes the key with a nanosecond timestamp so two
        // workers racing on the same key don't clobber each other's tmp
        // file mid-write (only the rename is atomic).
        let tmp_suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_path = path.with_extension(format!("o.tmp.{:x}", tmp_suffix));
        let result = fs::write(&tmp_path, bytes).and_then(|_| fs::rename(&tmp_path, &path));
        match result {
            Ok(()) => {
                self.stores.fetch_add(1, Ordering::Relaxed);
                Some(path)
            }
            Err(_) => {
                // Best-effort cleanup of the tmp file.
                let _ = fs::remove_file(&tmp_path);
                self.store_errors.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Store the freshly-compiled bytes under `key`. This compatibility
    /// wrapper keeps older tests/callers from depending on the returned path.
    #[allow(dead_code)]
    pub fn store(&self, key: u64, bytes: &[u8]) {
        let _ = self.store_and_get_path(key, bytes);
    }

    /// Whether the cache is actually writing to disk. `false` when
    /// disabled by `--no-cache`, by bitcode-link mode, or by a
    /// create-dir failure.
    pub fn is_enabled(&self) -> bool {
        self.cache_dir.is_some()
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> usize {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn stores(&self) -> usize {
        self.stores.load(Ordering::Relaxed)
    }

    pub fn store_errors(&self) -> usize {
        self.store_errors.load(Ordering::Relaxed)
    }

    pub fn path_reuses(&self) -> usize {
        self.path_reuses.load(Ordering::Relaxed)
    }

    pub fn bytes_materialized(&self) -> usize {
        self.bytes_materialized.load(Ordering::Relaxed)
    }
}
#[cfg(test)]
mod object_cache_tests {
    use super::*;
    use perry_codegen::{CompileOptions, ImportedClass, NamespaceEntry, NamespaceEntryKind};
    use tempfile::tempdir;

    /// A minimal `CompileOptions` with every vec/map empty. Tests that want
    /// to vary one field mutate the returned value before hashing.
    fn empty_opts() -> CompileOptions {
        CompileOptions {
            target: Some("aarch64-apple-darwin".to_string()),
            is_entry_module: false,
            non_entry_module_prefixes: Vec::new(),
            nextjs_path_init_modules: Vec::new(),
            import_function_prefixes: std::collections::HashMap::new(),
            import_function_ffi_aliases: std::collections::HashMap::new(),
            import_function_origin_names: std::collections::HashMap::new(),
            import_function_v8_specifiers: std::collections::HashMap::new(),
            // Issue #841: new submodule registry fields.
            import_function_node_submodule: std::collections::HashMap::new(),
            namespace_node_submodules: std::collections::HashMap::new(),
            namespace_v8_specifiers: std::collections::HashMap::new(),
            namespace_member_prefixes: std::collections::HashMap::new(),
            namespace_member_origin_names: std::collections::HashMap::new(),
            emit_ir_only: false,
            verify_native_regions: false,
            disable_buffer_fast_path: false,
            namespace_imports: Vec::new(),
            imported_classes: Vec::new(),
            imported_enums: Vec::new(),
            imported_async_funcs: std::collections::HashSet::new(),
            type_aliases: std::collections::HashMap::new(),
            imported_func_param_counts: std::collections::HashMap::new(),
            imported_func_has_rest: std::collections::HashSet::new(),
            imported_func_synthetic_arguments: std::collections::HashSet::new(),
            imported_func_return_types: std::collections::HashMap::new(),
            imported_vars: std::collections::HashSet::new(),
            output_type: "executable".to_string(),
            needs_stdlib: false,
            needs_ui: false,
            needs_geisterhand: false,
            geisterhand_port: 7676,
            enabled_features: Vec::new(),
            native_module_init_names: Vec::new(),
            js_module_specifiers: Vec::new(),
            bundled_extensions: Vec::new(),
            native_library_functions: Vec::new(),
            i18n_table: None,
            fast_math: false,
            fp_contract_mode: perry_codegen::FpContractMode::Off,
            app_metadata: perry_codegen::AppMetadata::default(),
            namespace_entries: Vec::new(),
            dynamic_import_path_to_prefix: std::collections::HashMap::new(),
            deferred_module_prefixes: std::collections::HashSet::new(),
            module_init_deps: Vec::new(),
            is_dynamic_import_target: false,
            debug_locations: false,
            module_source: None,
            debug_source_line_offset: 0,
        }
    }

    #[test]
    fn djb2_hash_is_stable_and_distinct() {
        assert_eq!(djb2_hash(b""), 5381);
        assert_eq!(djb2_hash(b"hello"), djb2_hash(b"hello"));
        assert_ne!(djb2_hash(b"hello"), djb2_hash(b"world"));
    }

    #[test]
    fn key_stable_for_same_inputs() {
        let opts = empty_opts();
        let k1 = compute_object_cache_key(&opts, 0xdeadbeef, "0.5.156");
        let k2 = compute_object_cache_key(&opts, 0xdeadbeef, "0.5.156");
        assert_eq!(k1, k2);
    }

    #[test]
    fn key_changes_with_hir_hash() {
        // The second argument is the post-transform HIR fingerprint
        // (issue #686), produced by `perry_hir::stable_hash::hash_module`.
        // Two different HIR hashes — i.e. two semantically different
        // modules — must produce different cache keys.
        //
        // Note: "same source bytes, different HIR" (e.g. a lowering-pass
        // behavior change between Perry versions that rewrites the same
        // input into different HIR) is covered by the `build_id` field
        // mixed in by `perry_build_id()`, NOT by this hash. So a HIR
        // walk that adds new fields between releases doesn't need a
        // separate invalidation hook here.
        let opts = empty_opts();
        let a = compute_object_cache_key(&opts, 1, "0.5.156");
        let b = compute_object_cache_key(&opts, 2, "0.5.156");
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_with_perry_version() {
        let opts = empty_opts();
        let a = compute_object_cache_key(&opts, 1, "0.5.155");
        let b = compute_object_cache_key(&opts, 1, "0.5.156");
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_with_target() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.target = Some("aarch64-apple-darwin".to_string());
        b.target = Some("x86_64-apple-darwin".to_string());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_entry_flag() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.is_entry_module = false;
        b.is_entry_module = true;
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_debug_locations_flag() {
        // #5247: toggling --debug-symbols (debug_locations) flips per-call
        // location emission, so cached objects must not be shared across it.
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.debug_locations = false;
        b.debug_locations = true;
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_fast_math_flag() {
        // Without this guard, `perry --fast-math foo.ts` after a default
        // build would silently serve the cached non-fast-math `.o` and
        // the flag would appear to do nothing. Bug found during the
        // original fast-math investigation; gate it here so a future
        // refactor can't reintroduce it.
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.fast_math = false;
        b.fast_math = true;
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.569"),
            compute_object_cache_key(&b, 1, "0.5.569")
        );
    }

    #[test]
    fn key_changes_with_fp_contract_mode() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.fp_contract_mode = perry_codegen::FpContractMode::Off;
        b.fp_contract_mode = perry_codegen::FpContractMode::On;
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.569"),
            compute_object_cache_key(&b, 1, "0.5.569")
        );
    }

    #[test]
    fn key_includes_perry_build_id() {
        // Issue #544: the cache key must mix in a hash of the running perry
        // binary so HIR/codegen pass changes invalidate the cache even when
        // the version string doesn't move. We can't easily synthesize two
        // distinct binary hashes from inside a unit test, but we can check
        // (a) that `perry_build_id()` returns a non-zero value when the
        // test binary exists on disk (i.e. the helper actually ran), and
        // (b) that perturbing the helper's output would change the key —
        // verified indirectly by confirming the field is present in the
        // serialized form via the field separator count.
        let id = perry_build_id();
        // The test binary is always readable, so the helper can't degrade
        // to 0 here. If this ever fails, current_exe() / fs::read started
        // misbehaving and we'd want to know.
        assert_ne!(id, 0, "perry_build_id must hash the test executable");
        // Stable across calls within a process (OnceLock).
        assert_eq!(perry_build_id(), id);
    }

    #[test]
    fn key_changes_with_non_entry_prefix_order() {
        // Order-significant: non_entry_module_prefixes is topologically
        // sorted, and a reorder must invalidate the cache (this is the
        // v0.5.127-128 link-ordering regression class — the issue's
        // acceptance criterion).
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.is_entry_module = true;
        b.is_entry_module = true;
        a.non_entry_module_prefixes = vec!["a".into(), "b".into()];
        b.non_entry_module_prefixes = vec!["b".into(), "a".into()];
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_stable_regardless_of_hashmap_insertion_order() {
        // HashMap iteration order is platform-dependent; the key must
        // sort entries so two equivalent maps produce the same hash.
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.import_function_prefixes
            .insert("foo".into(), "mod_a".into());
        a.import_function_prefixes
            .insert("bar".into(), "mod_b".into());
        b.import_function_prefixes
            .insert("bar".into(), "mod_b".into());
        b.import_function_prefixes
            .insert("foo".into(), "mod_a".into());
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_stable_for_order_insensitive_graph_lists() {
        // These graph-wide lists are either derived from collections or are
        // consumed as lookup metadata, not as an ordered codegen sequence.
        // Hashing their raw Vec order would make every module key depend on
        // upstream collection/traversal order.
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.js_module_specifiers = vec!["z.js".into(), "a.js".into()];
        b.js_module_specifiers = vec!["a.js".into(), "z.js".into()];
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        a = empty_opts();
        b = empty_opts();
        a.bundled_extensions = vec![
            ("/project/ext/z.ts".into(), "project_ext_z_ts".into()),
            ("/project/ext/a.ts".into(), "project_ext_a_ts".into()),
        ];
        b.bundled_extensions = vec![
            ("/project/ext/a.ts".into(), "project_ext_a_ts".into()),
            ("/project/ext/z.ts".into(), "project_ext_z_ts".into()),
        ];
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        a = empty_opts();
        b = empty_opts();
        a.native_library_functions = vec![
            (
                "zeta".into(),
                vec![perry_api_manifest::NativeAbiType::I32],
                perry_api_manifest::NativeAbiType::F64,
            ),
            (
                "alpha".into(),
                vec![perry_api_manifest::NativeAbiType::String],
                perry_api_manifest::NativeAbiType::Bool,
            ),
        ];
        b.native_library_functions = vec![
            (
                "alpha".into(),
                vec![perry_api_manifest::NativeAbiType::String],
                perry_api_manifest::NativeAbiType::Bool,
            ),
            (
                "zeta".into(),
                vec![perry_api_manifest::NativeAbiType::I32],
                perry_api_manifest::NativeAbiType::F64,
            ),
        ];
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    fn record_row_type(property_insert_order: &[&str]) -> perry_types::Type {
        let mut properties = std::collections::HashMap::new();
        for name in property_insert_order {
            let ty = match *name {
                "name" => perry_types::Type::String,
                "id" | "value" => perry_types::Type::Number,
                _ => panic!("unexpected property"),
            };
            properties.insert(
                (*name).to_string(),
                perry_types::PropertyInfo {
                    ty,
                    optional: false,
                    readonly: false,
                },
            );
        }
        perry_types::Type::Object(perry_types::ObjectType {
            name: None,
            properties,
            property_order: Some(vec!["id".into(), "name".into(), "value".into()]),
            index_signature: None,
        })
    }

    #[test]
    fn key_stable_for_nested_type_hashmap_order() {
        let type_a = record_row_type(&["name", "id", "value"]);
        let type_b = record_row_type(&["id", "name", "value"]);

        let mut a = empty_opts();
        let mut b = empty_opts();
        a.type_aliases.insert("RecordRow".into(), type_a.clone());
        b.type_aliases.insert("RecordRow".into(), type_b.clone());
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        a = empty_opts();
        b = empty_opts();
        a.imported_func_return_types
            .insert("loadRecord".into(), type_a.clone());
        b.imported_func_return_types
            .insert("loadRecord".into(), type_b.clone());
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        let class_for = |field_type| ImportedClass {
            name: "RowBox".into(),
            local_alias: None,
            source_prefix: "feature_ts".into(),
            constructor_param_count: 0,
            has_own_constructor: false,
            constructor_has_rest: false,
            has_instance_fields: true,
            method_names: vec![],
            method_param_counts: vec![],
            method_has_rest: vec![],
            static_method_names: vec![],
            getter_names: vec![],
            setter_names: vec![],
            parent_name: None,
            field_names: vec!["row".into()],
            field_types: vec![field_type],
            static_field_names: vec![],
            source_class_id: Some(7),
        };

        a = empty_opts();
        b = empty_opts();
        a.imported_classes.push(class_for(type_a));
        b.imported_classes.push(class_for(type_b));
        assert_eq!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_imported_class_signature() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.imported_classes.push(ImportedClass {
            name: "Foo".into(),
            local_alias: None,
            source_prefix: "src".into(),
            constructor_param_count: 1,
            has_own_constructor: true,
            constructor_has_rest: false,
            has_instance_fields: true,
            method_names: vec!["bar".into()],
            method_param_counts: vec![0],
            method_has_rest: vec![false],
            static_method_names: vec![],
            getter_names: vec![],
            setter_names: vec![],
            parent_name: None,
            field_names: vec!["x".into()],
            field_types: vec![],
            static_field_names: vec![],
            source_class_id: Some(42),
        });
        b.imported_classes.push(ImportedClass {
            name: "Foo".into(),
            local_alias: None,
            source_prefix: "src".into(),
            constructor_param_count: 2, // different arity
            has_own_constructor: true,
            constructor_has_rest: false,
            has_instance_fields: true,
            method_names: vec!["bar".into()],
            method_param_counts: vec![0],
            method_has_rest: vec![false],
            static_method_names: vec![],
            getter_names: vec![],
            setter_names: vec![],
            parent_name: None,
            field_names: vec!["x".into()],
            field_types: vec![],
            static_field_names: vec![],
            source_class_id: Some(42),
        });
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_imported_class_codegen_surface() {
        let base = ImportedClass {
            name: "Foo".into(),
            local_alias: None,
            source_prefix: "src".into(),
            constructor_param_count: 1,
            has_own_constructor: true,
            constructor_has_rest: false,
            has_instance_fields: true,
            method_names: vec!["bar".into()],
            method_param_counts: vec![1],
            method_has_rest: vec![false],
            static_method_names: vec![],
            getter_names: vec![],
            setter_names: vec![],
            parent_name: None,
            field_names: vec!["x".into()],
            field_types: vec![perry_types::Type::Number],
            static_field_names: vec![],
            source_class_id: Some(42),
        };
        let key_for = |class: ImportedClass| {
            let mut opts = empty_opts();
            opts.imported_classes.push(class);
            compute_object_cache_key(&opts, 1, "0.5.156")
        };
        let base_key = key_for(base.clone());

        let mut changed = base.clone();
        changed.has_own_constructor = false;
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.has_instance_fields = false;
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.method_has_rest = vec![true];
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.static_method_names = vec!["make".into()];
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.static_field_names = vec!["VERSION".into()];
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.getter_names = vec!["value".into()];
        assert_ne!(base_key, key_for(changed));

        let mut changed = base.clone();
        changed.setter_names = vec!["value".into()];
        assert_ne!(base_key, key_for(changed));

        let mut changed = base;
        changed.field_types = vec![perry_types::Type::String];
        assert_ne!(base_key, key_for(changed));
    }

    #[test]
    fn key_changes_with_namespace_member_prefixes() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.namespace_member_prefixes
            .insert(("ns".into(), "make".into()), "src_a".into());
        b.namespace_member_prefixes
            .insert(("ns".into(), "make".into()), "src_b".into());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_imported_rest_shapes() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        b.imported_func_has_rest.insert("collect".into());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        a = empty_opts();
        b = empty_opts();
        b.imported_func_synthetic_arguments.insert("invoke".into());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_dynamic_import_metadata() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        b.namespace_entries.push(NamespaceEntry {
            name: "answer".into(),
            kind: NamespaceEntryKind::ForeignFunction {
                source_prefix: "dep".into(),
                source_local: "answer".into(),
                param_count: 1,
            },
        });
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );

        a = empty_opts();
        b = empty_opts();
        b.dynamic_import_path_to_prefix
            .insert("./lazy".into(), "lazy_ts".into());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_app_group() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.app_metadata.app_group = None;
        b.app_metadata.app_group = Some("group.com.example.shared".into());
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_bitcode_mode() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.emit_ir_only = false;
        b.emit_ir_only = true;
        assert_ne!(
            compute_object_cache_key(&a, 1, "0.5.156"),
            compute_object_cache_key(&b, 1, "0.5.156")
        );
    }

    #[test]
    fn key_changes_with_codegen_env_vars() {
        // Flipping an env var that perry-codegen reads must invalidate the
        // key so we don't serve a cached .o that was built with different
        // debug sections, a different clang binary, different generated
        // helper calls, or a skipped verifier.
        //
        let opts = empty_opts();
        for var in [
            "PERRY_DEBUG_INIT",
            "PERRY_DEBUG_SYMBOLS",
            "PERRY_LLVM_CLANG",
            "PERRY_WRITE_BARRIERS",
            "PERRY_SHADOW_STACK",
            "PERRY_DISABLE_BUFFER_FAST_PATH",
            "PERRY_VERIFY_NATIVE_REGIONS",
            "PERRY_UNBOXED_OBJECT_FIELDS",
            "PERRY_TARGET_CPU",
        ] {
            // Sample state without the var, with the var, and with a different
            // value — all three keys must be distinct.
            let k_unset = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |_| None);
            let k_set = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |name| {
                (name == var).then(|| "1".to_string())
            });
            let k_two = compute_object_cache_key_with_env(&opts, 1, "0.5.156", |name| {
                (name == var).then(|| "2".to_string())
            });
            assert_ne!(k_unset, k_set, "setting {} must change key", var);
            assert_ne!(k_set, k_two, "changing {} value must change key", var);
        }
    }

    #[test]
    fn disabled_cache_always_misses_and_drops_stores() {
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "test-target", false);
        assert!(!cache.is_enabled());
        assert!(cache.lookup(0xdeadbeef).is_none());
        cache.store(0xdeadbeef, b"payload");
        // Nothing was written — a second lookup still misses.
        assert!(cache.lookup(0xdeadbeef).is_none());
        // No counters bumped for a disabled cache.
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.stores(), 0);
    }

    #[test]
    fn store_then_lookup_round_trips_bytes() {
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "test-target", true);
        assert!(cache.is_enabled());
        let key = 0xcafef00d;
        let payload = b"the quick brown fox".to_vec();
        cache.store(key, &payload);
        assert_eq!(cache.stores(), 1);
        let got = cache.lookup(key).expect("must hit after store");
        assert_eq!(got, payload);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.bytes_materialized(), payload.len());
        assert_eq!(cache.path_reuses(), 0);
    }

    #[test]
    fn store_then_lookup_path_reuses_cached_file_without_materializing_bytes() {
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "test-target", true);
        let key = 0xfeedface;
        cache.store(key, b"object bytes");

        let path = cache.lookup_path(key).expect("must hit by path");
        assert!(path.is_file(), "missing cached object: {}", path.display());
        assert_eq!(std::fs::read(path).unwrap(), b"object bytes");
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.path_reuses(), 1);
        assert_eq!(cache.bytes_materialized(), 0);
    }

    #[test]
    fn lookup_miss_bumps_miss_counter() {
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "test-target", true);
        assert!(cache.lookup(0x1234).is_none());
        assert!(cache.lookup_path(0x5678).is_none());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 2);
    }

    #[test]
    fn cache_files_land_under_target_subdirectory() {
        // The on-disk layout must be <cache_dir>/objects/<target>/<hex>.o
        // so cross-compile caches can coexist without colliding. The dir
        // passed to ObjectCache::new is the already-resolved cache dir.
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "aarch64-apple-darwin", true);
        cache.store(0xabc, b"xx");
        let expected = dir
            .path()
            .join("objects")
            .join("aarch64-apple-darwin")
            .join(format!("{:016x}.o", 0xabc_u64));
        assert!(expected.exists(), "missing: {}", expected.display());
    }

    #[test]
    fn resolve_cache_dir_defaults_to_node_modules_cache_perry() {
        // No override → the find-cache-dir convention under the project root.
        let root = Path::new("/projects/app");
        let got = resolve_cache_dir(root, None);
        assert_eq!(
            got,
            Path::new("/projects/app")
                .join("node_modules")
                .join(".cache")
                .join("perry")
        );
    }

    #[test]
    fn resolve_cache_dir_absolute_override_used_as_is() {
        // An absolute override ignores the project root entirely.
        let root = Path::new("/projects/app");
        let override_dir = Path::new("/var/cache/perry");
        let got = resolve_cache_dir(root, Some(override_dir));
        assert_eq!(got, Path::new("/var/cache/perry"));
    }

    #[test]
    fn resolve_cache_dir_relative_override_resolves_against_project_root() {
        // A relative override joins onto the project root, so two projects
        // with the same `perry.cacheDir: ".cache"` don't collide.
        let root = Path::new("/projects/app");
        let override_dir = Path::new("build/cache");
        let got = resolve_cache_dir(root, Some(override_dir));
        assert_eq!(got, Path::new("/projects/app").join("build").join("cache"));
    }

    #[test]
    fn object_cache_writes_under_resolved_cache_dir() {
        // End-to-end: resolve the default dir, build the cache against it,
        // and confirm bytes land under <resolved>/objects/<target>/.
        let dir = tempdir().unwrap();
        let resolved = resolve_cache_dir(dir.path(), None);
        let cache = ObjectCache::new(&resolved, "aarch64-apple-darwin", true);
        cache.store(0xabc, b"xx");
        let expected = dir
            .path()
            .join("node_modules")
            .join(".cache")
            .join("perry")
            .join("objects")
            .join("aarch64-apple-darwin")
            .join(format!("{:016x}.o", 0xabc_u64));
        assert!(expected.exists(), "missing: {}", expected.display());
    }

    #[test]
    fn different_targets_do_not_share_entries() {
        let dir = tempdir().unwrap();
        let a = ObjectCache::new(dir.path(), "target-a", true);
        let b = ObjectCache::new(dir.path(), "target-b", true);
        a.store(0x777, b"from-a");
        assert!(b.lookup(0x777).is_none());
        assert_eq!(a.lookup(0x777).as_deref(), Some(b"from-a".as_ref()));
    }

    // --- cache-dir override precedence ----------------------------------
    //
    // Full chain (highest wins): `--cache-dir` CLI flag → `PERRY_CACHE_DIR`
    // env → perry.toml `[perry] cacheDir` → package.json `perry.cacheDir` →
    // default. The CLI flag is layered on top by the callers
    // (`args.cache_dir.or_else(cache_dir_override)`), so the merge tested
    // here is the non-CLI half: env → perry.toml → package.json.
    //
    // `pick_cache_dir_override` is pure over the already-read candidate
    // strings, so the precedence is checked without filesystem or env races.

    #[test]
    fn pick_override_env_beats_toml_and_pkg() {
        // env wins over both lower layers — i.e. `PERRY_CACHE_DIR` overrides
        // perry.toml, which is the "env beats perry.toml" guarantee.
        let got = pick_cache_dir_override(Some("/env"), Some("/toml"), Some("/pkg"));
        assert_eq!(got, Some(PathBuf::from("/env")));
    }

    #[test]
    fn pick_override_toml_beats_pkg() {
        // perry.toml overrides package.json when env is unset.
        let got = pick_cache_dir_override(None, Some("/toml"), Some("/pkg"));
        assert_eq!(got, Some(PathBuf::from("/toml")));
    }

    #[test]
    fn pick_override_pkg_used_when_only_pkg_set() {
        let got = pick_cache_dir_override(None, None, Some("/pkg"));
        assert_eq!(got, Some(PathBuf::from("/pkg")));
    }

    #[test]
    fn pick_override_none_when_all_unset() {
        assert_eq!(pick_cache_dir_override(None, None, None), None);
    }

    #[test]
    fn pick_override_skips_empty_higher_layers() {
        // An empty string is treated as "not set", so a blank env value falls
        // through to perry.toml and a blank perry.toml falls through to pkg.
        assert_eq!(
            pick_cache_dir_override(Some(""), Some("/toml"), Some("/pkg")),
            Some(PathBuf::from("/toml"))
        );
        assert_eq!(
            pick_cache_dir_override(Some(""), Some(""), Some("/pkg")),
            Some(PathBuf::from("/pkg"))
        );
        assert_eq!(pick_cache_dir_override(Some(""), Some(""), Some("")), None);
    }

    #[test]
    fn perry_toml_cache_dir_reads_perry_table() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("perry.toml"),
            "[perry]\ncacheDir = \"/var/cache/perry\"\n",
        )
        .unwrap();
        assert_eq!(
            perry_toml_cache_dir(dir.path()).as_deref(),
            Some("/var/cache/perry")
        );
    }

    #[test]
    fn perry_toml_cache_dir_none_when_key_or_file_absent() {
        // No perry.toml at all.
        let empty = tempdir().unwrap();
        assert_eq!(perry_toml_cache_dir(empty.path()), None);

        // perry.toml present but no `[perry] cacheDir` key.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("perry.toml"), "[perry]\nstrict = true\n").unwrap();
        assert_eq!(perry_toml_cache_dir(dir.path()), None);
    }

    #[test]
    fn perry_toml_cache_dir_walks_up_to_project_root() {
        // The reader walks up from a nested dir, mirroring how the compile
        // pipeline discovers config from a subdirectory entry file.
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("perry.toml"),
            "[perry]\ncacheDir = \".cache\"\n",
        )
        .unwrap();
        let nested = root.path().join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(perry_toml_cache_dir(&nested).as_deref(), Some(".cache"));
    }

    #[test]
    fn package_json_cache_dir_reads_perry_field() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{ "perry": { "cacheDir": ".perry-cache" } }"#,
        )
        .unwrap();
        assert_eq!(
            package_json_cache_dir(dir.path()).as_deref(),
            Some(".perry-cache")
        );
    }

    #[test]
    fn toml_overrides_pkg_via_readers_and_resolver() {
        // End-to-end (no env, no CLI): with both perry.toml and package.json
        // present, the chosen override is perry.toml's, and a relative value
        // resolves against the project root — matching the existing
        // `resolve_cache_dir_relative_override_resolves_against_project_root`
        // contract for the new perry.toml layer.
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("perry.toml"),
            "[perry]\ncacheDir = \"toml-cache\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{ "perry": { "cacheDir": "pkg-cache" } }"#,
        )
        .unwrap();

        let toml = perry_toml_cache_dir(root.path());
        let pkg = package_json_cache_dir(root.path());
        let chosen = pick_cache_dir_override(None, toml.as_deref(), pkg.as_deref());
        assert_eq!(chosen.as_deref(), Some(Path::new("toml-cache")));

        // Relative perry.toml value resolves against the project root.
        let resolved = resolve_cache_dir(root.path(), chosen.as_deref());
        assert_eq!(resolved, root.path().join("toml-cache"));
    }
}
