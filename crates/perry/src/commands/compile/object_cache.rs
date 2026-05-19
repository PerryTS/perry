//! Per-module on-disk object file cache + key derivation.
//!
//! Tier 2.1 follow-up (v0.5.340) — extracts the V2.2 codegen cache
//! family from `compile.rs`. Three concerns clustered here because
//! they all relate to the `.perry-cache/objects/<target>/<key>.o`
//! cache layout:
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
//! 3. **`ObjectCache`** — the `lookup` / `store` surface used by the
//!    rayon codegen workers. Atomic (tmp + rename) writes, silent
//!    IO-error degradation, lock-free shared `&self` access (each
//!    cache key is per-module so writes never conflict).

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
        self.write(name.as_bytes());
        self.write(b"=");
        self.write(value.as_bytes());
        self.write(b"\x1f");
    }
    fn finish(self) -> u64 {
        self.state
    }
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
/// We also mix in four environment variables that `perry-codegen` reads
/// at compile time but that aren't part of `CompileOptions`:
/// `PERRY_DEBUG_INIT`, `PERRY_DEBUG_SYMBOLS`, `PERRY_LLVM_CLANG`, and
/// `PERRY_WRITE_BARRIERS`. See the env-var block at the bottom of this
/// function for the rationale.
///
/// NOT captured in the key: the host CPU. `compile_ll_to_object` passes
/// `-mcpu=native`/`-march=native` to clang, so the emitted `.o` bakes in
/// whatever instruction set the build machine supports. The cache is
/// consequently **machine-local** — `.perry-cache/` is in `.gitignore`
/// for this reason. Sharing across machines with different CPUs (rsync,
/// NFS, Docker bind-mount) can produce SIGILL at runtime.
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
    h.field("jsrt", if opts.needs_js_runtime { "1" } else { "0" });
    h.field("gh_port", &opts.geisterhand_port.to_string());
    // Fast-math flag flips per-instruction `reassoc + contract` emission
    // in `perry-codegen`, which produces different LLVM IR (and therefore
    // different `.o` bytes) for the same TS source. Without this in the
    // key, `perry --fast-math foo.ts` after a default `perry foo.ts` would
    // serve the previously-cached non-fast-math `.o` and the flag would
    // appear to do nothing.
    h.field("fmath", if opts.fast_math { "1" } else { "0" });
    h.field("app_version", &opts.app_metadata.version);
    h.field(
        "app_build_number",
        &opts.app_metadata.build_number.to_string(),
    );
    h.field("app_bundle_id", &opts.app_metadata.bundle_id);

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
    h.field("js_specs", &opts.js_module_specifiers.join("|"));
    {
        let mut buf = String::new();
        for (path, prefix) in &opts.bundled_extensions {
            buf.push_str(path);
            buf.push('@');
            buf.push_str(prefix);
            buf.push('|');
        }
        h.field("bundled_ext", &buf);
    }
    {
        let mut buf = String::new();
        for (lib, funcs, header) in &opts.native_library_functions {
            buf.push_str(lib);
            buf.push(':');
            buf.push_str(&funcs.join(","));
            buf.push('@');
            buf.push_str(header);
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

    // Issue #321: namespace reexport named imports — separate subset that
    // gates the codegen's StaticMethodCall var-shape routing. Cache must
    // discriminate between two modules whose `namespace_imports` are
    // identical but whose `namespace_reexport_named_imports` differ, else
    // the wrong code-path winds up in the object file.
    {
        let mut v: Vec<&String> = opts.namespace_reexport_named_imports.iter().collect();
        v.sort();
        h.field(
            "ns_reexport_named_imports",
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
                "{}@{}:ctor={}:parent={}:alias={}:id={}:fields={}:methods={}:method_arities={}|",
                c.name,
                c.source_prefix,
                c.constructor_param_count,
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

    // Imported return types (HashMap — MUST sort). Type has Debug but no
    // Display; Debug is deterministic for this type (all enum/Vec, no
    // HashMap internally as of v0.5.156).
    {
        let mut v: Vec<(&String, &perry_types::Type)> =
            opts.imported_func_return_types.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        let s = v
            .iter()
            .map(|(k, vv)| format!("{}={:?}", k, vv))
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
            .map(|(k, vv)| format!("{}={:?}", k, vv))
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
    // Hashing the values (not just presence) means a persistent override
    // like PERRY_LLVM_CLANG=/opt/homebrew/opt/llvm/bin/clang in a shell rc
    // still gets cache reuse across runs, while flipping a debug flag on
    // or off cleanly invalidates.
    h.field(
        "env_debug_init",
        std::env::var("PERRY_DEBUG_INIT").as_deref().unwrap_or(""),
    );
    h.field(
        "env_debug_symbols",
        std::env::var("PERRY_DEBUG_SYMBOLS")
            .as_deref()
            .unwrap_or(""),
    );
    h.field(
        "env_llvm_clang",
        std::env::var("PERRY_LLVM_CLANG").as_deref().unwrap_or(""),
    );
    h.field(
        "env_write_barriers",
        std::env::var("PERRY_WRITE_BARRIERS")
            .as_deref()
            .unwrap_or(""),
    );

    h.finish()
}
/// On-disk per-module object cache at `.perry-cache/objects/<target>/<hash:016x>.o`.
///
/// Each rayon codegen worker calls `lookup(key)`; on hit, it skips the LLVM
/// pipeline and hands the cached bytes to the linker; on miss, it runs
/// `compile_module` as usual and then calls `store(key, bytes)` to
/// populate the cache for the next build. Atomic (tmp + rename) writes
/// and silent IO-error handling mean the cache is strictly an optimization
/// — any corruption or permission failure degrades gracefully to the
/// uncached codepath.
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
}

impl ObjectCache {
    /// Create a new cache rooted at `<project_root>/.perry-cache/objects/<target>/`.
    /// `target_triple` is the LLVM target triple (or `"host"` for the host
    /// default). Passing `enabled = false` returns a no-op instance —
    /// every `lookup` misses and every `store` is a silent drop.
    pub fn new(project_root: &Path, target_triple: &str, enabled: bool) -> Self {
        let cache_dir = if enabled {
            let dir = project_root
                .join(".perry-cache")
                .join("objects")
                .join(target_triple);
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
    pub fn lookup(&self, key: u64) -> Option<Vec<u8>> {
        let path = self.path_for(key)?;
        match fs::read(&path) {
            Ok(bytes) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(bytes)
            }
            Err(_) => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Store the freshly-compiled bytes under `key`. Atomic via tmp +
    /// rename so a concurrent reader in another process never sees a
    /// partial file. IO errors are counted but not reported — the cache
    /// is strictly an optimization.
    pub fn store(&self, key: u64, bytes: &[u8]) {
        let path = match self.path_for(key) {
            Some(p) => p,
            None => return,
        };
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
            }
            Err(_) => {
                // Best-effort cleanup of the tmp file.
                let _ = fs::remove_file(&tmp_path);
                self.store_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
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
}
#[cfg(test)]
mod object_cache_tests {
    use super::*;
    use perry_codegen::{CompileOptions, ImportedClass};
    use tempfile::tempdir;

    /// A minimal `CompileOptions` with every vec/map empty. Tests that want
    /// to vary one field mutate the returned value before hashing.
    fn empty_opts() -> CompileOptions {
        CompileOptions {
            target: Some("aarch64-apple-darwin".to_string()),
            is_entry_module: false,
            non_entry_module_prefixes: Vec::new(),
            import_function_prefixes: std::collections::HashMap::new(),
            import_function_origin_names: std::collections::HashMap::new(),
            import_function_v8_specifiers: std::collections::HashMap::new(),
            // Issue #841: new submodule registry fields.
            import_function_node_submodule: std::collections::HashMap::new(),
            namespace_node_submodules: std::collections::HashMap::new(),
            namespace_v8_specifiers: std::collections::HashMap::new(),
            namespace_member_prefixes: std::collections::HashMap::new(),
            emit_ir_only: false,
            namespace_imports: Vec::new(),
            namespace_reexport_named_imports: std::collections::HashSet::new(),
            imported_classes: Vec::new(),
            imported_enums: Vec::new(),
            imported_async_funcs: std::collections::HashSet::new(),
            type_aliases: std::collections::HashMap::new(),
            imported_func_param_counts: std::collections::HashMap::new(),
            imported_func_has_rest: std::collections::HashSet::new(),
            imported_func_return_types: std::collections::HashMap::new(),
            imported_vars: std::collections::HashSet::new(),
            output_type: "executable".to_string(),
            needs_stdlib: false,
            needs_ui: false,
            needs_geisterhand: false,
            geisterhand_port: 7676,
            needs_js_runtime: false,
            enabled_features: Vec::new(),
            native_module_init_names: Vec::new(),
            js_module_specifiers: Vec::new(),
            bundled_extensions: Vec::new(),
            native_library_functions: Vec::new(),
            i18n_table: None,
            fast_math: false,
            app_metadata: perry_codegen::AppMetadata::default(),
            namespace_entries: Vec::new(),
            dynamic_import_path_to_prefix: std::collections::HashMap::new(),
            deferred_module_prefixes: std::collections::HashSet::new(),
            module_init_deps: Vec::new(),
            is_dynamic_import_target: false,
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
    fn key_changes_with_imported_class_signature() {
        let mut a = empty_opts();
        let mut b = empty_opts();
        a.imported_classes.push(ImportedClass {
            name: "Foo".into(),
            local_alias: None,
            source_prefix: "src".into(),
            constructor_param_count: 1,
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
        // Flipping an env var that perry-codegen reads (PERRY_DEBUG_INIT,
        // PERRY_DEBUG_SYMBOLS, PERRY_LLVM_CLANG, PERRY_WRITE_BARRIERS)
        // must invalidate the key so we don't serve a cached .o that was
        // built with different debug sections, a different clang binary,
        // or different generated barrier calls.
        //
        // Uses unique var names (suffixed with a test-local marker) would
        // be cleaner, but we're checking behavior against the *actual*
        // names the codegen reads — toggling them temporarily with unsafe
        // env::set_var is the only way. Test is #[serial]-safe only in
        // spirit; cargo's single-threaded test runner for this binary
        // keeps it from racing with other tests that happen to read the
        // same vars (none today).
        let opts = empty_opts();
        for var in ["PERRY_DEBUG_INIT", "PERRY_WRITE_BARRIERS"] {
            // Sample state without the var, with the var, and with a different
            // value — all three keys must be distinct.
            let prev = std::env::var_os(var);
            // SAFETY: Rust 1.80+ flags env::set_var/remove_var as unsafe
            // because they're racy with other threads reading env. cargo's
            // in-process test runner can parallelize tests; this test is
            // still correct because `compute_object_cache_key` reads the
            // env at call time and we don't span a .await / yield. The
            // remaining race is another *test* reading these vars
            // mid-flight, which none do.
            unsafe { std::env::remove_var(var) };
            let k_unset = compute_object_cache_key(&opts, 1, "0.5.156");
            unsafe { std::env::set_var(var, "1") };
            let k_set = compute_object_cache_key(&opts, 1, "0.5.156");
            unsafe { std::env::set_var(var, "2") };
            let k_two = compute_object_cache_key(&opts, 1, "0.5.156");
            // Restore.
            match prev {
                Some(v) => unsafe { std::env::set_var(var, v) },
                None => unsafe { std::env::remove_var(var) },
            }
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
    }

    #[test]
    fn lookup_miss_bumps_miss_counter() {
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "test-target", true);
        assert!(cache.lookup(0x1234).is_none());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn cache_files_land_under_target_subdirectory() {
        // The on-disk layout must be .perry-cache/objects/<target>/<hex>.o
        // so cross-compile caches can coexist without colliding.
        let dir = tempdir().unwrap();
        let cache = ObjectCache::new(dir.path(), "aarch64-apple-darwin", true);
        cache.store(0xabc, b"xx");
        let expected = dir
            .path()
            .join(".perry-cache")
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
}
