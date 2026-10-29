//! Module source preparation, and running it ahead of the module walk.
//!
//! Preparing a module's source (read, CommonJS wrap, static-require
//! transform and the scans over the result) is a pure function of the file's
//! path, its bytes on disk and settings fixed before the walk starts. It is
//! most of the front end's time on CommonJS-heavy programs, yet the walk
//! needs each result only when it reaches that module. So worker threads
//! prepare the modules the walk has discovered (and modules they guess it
//! will discover) while the walk lowers earlier ones, and the walk consumes
//! the results strictly in its own order.
//!
//! Output does not depend on the workers: the walk thread and a worker call
//! the same [`prepare_plain`] with the same inputs, and the walk checks that a
//! worker's result was prepared for the classification the walk itself
//! computes for that module, recomputing inline otherwise. Workers never
//! touch the compilation context. Every effect on it, every error and every
//! diagnostic line is carried in the result and applied or printed by the
//! walk when it consumes the module, so they keep the walk's order.
//!
//! A compile walks the module graph twice (the second walk lowers against
//! the class field types the first one found), and the second walk prepares
//! the same files with the same settings. So the first walk returns the
//! sources it prepared, as [`PreparedSources`], and the second takes them as
//! an input ([`WalkSources`]) and uses each one prepared for the settings
//! and classification it computes itself, preparing the rest as usual.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use anyhow::{anyhow, Result};

use super::super::cjs_wrap::{
    extract_require_specifiers, is_commonjs, wrap_commonjs_with_addon_paths,
};
use super::super::{
    is_declaration_file, is_in_compile_package, is_in_compile_package_dir,
    is_in_perry_native_package, is_js_file, is_recognized_text_asset, CompilationContext,
};
use super::import_helpers::collect_js_module_imports;
use super::import_meta_require::import_meta_require_may_apply;
use super::json_module::synthesize_json_module;
use super::node_specifier_literals;
use super::static_require_transform::transform_static_literal_requires_with_bunfs;
use super::wasm_asset::is_wasm_asset;

/// Classification inputs that importers can extend while the walk runs.
pub(super) struct RouteInputs<'a> {
    pub(super) bunfs_root: Option<&'a Path>,
    pub(super) aot_discovered_modules: &'a HashSet<PathBuf>,
    pub(super) compile_packages: &'a HashSet<String>,
    pub(super) compile_package_dirs: &'a BTreeSet<PathBuf>,
}

impl<'a> RouteInputs<'a> {
    pub(super) fn of(ctx: &'a CompilationContext) -> Self {
        Self {
            bunfs_root: ctx.bunfs_root.as_deref(),
            aot_discovered_modules: &ctx.aot_discovered_modules,
            compile_packages: &ctx.compile_packages,
            compile_package_dirs: &ctx.compile_package_dirs,
        }
    }
}

/// How the walk routes a module.
pub(super) struct Route {
    pub(super) is_in_node_modules: bool,
    pub(super) is_perry_native: bool,
    pub(super) is_in_compiled_pkg: bool,
    pub(super) should_use_js_runtime: bool,
}

pub(super) fn route(canonical: &Path, inputs: &RouteInputs) -> Route {
    // Match a real `node_modules/` directory COMPONENT, not a substring: a
    // file whose NAME contains "node_modules" (e.g. turbopack's bundled chunks
    // `.next/server/chunks/ssr/node_modules_next_dist_…._.js`) is NOT in
    // node_modules and must compile natively, not get force-routed to the
    // (removed) JS runtime. (Next.js wall 54.)
    let is_in_node_modules = canonical
        .components()
        .any(|c| c.as_os_str() == "node_modules");
    let is_perry_native = is_in_node_modules && is_in_perry_native_package(canonical);
    // `--bunfs-root` describes source extracted from a self-contained Bun
    // executable. Compile every module below that opted-in tree natively,
    // including paths with a `node_modules` component; otherwise Perry's
    // ordinary dependency classification can route those mapped modules to
    // the removed JS fallback despite the resolver selecting NativeCompiled.
    let is_in_bunfs_root = inputs
        .bunfs_root
        .is_some_and(|root| canonical.starts_with(root));
    let is_in_compiled_pkg = is_in_bunfs_root
        || inputs.aot_discovered_modules.contains(canonical)
        || (is_in_node_modules && is_in_compile_package(canonical, inputs.compile_packages))
        // Excludes nested node_modules/ inside the compiled package (e.g.,
        // @solana/web3.js/node_modules/bs58/ is NOT part of @solana/web3.js).
        || is_in_compile_package_dir(canonical, inputs.compile_package_dirs)
        // A file whose canonical path resolves to inside a perry.nativeLibrary package
        // but is NOT under any node_modules/ component (i.e., reached via a file: dep
        // that places the package inside the project root, as in #209 "file:./vendor/bloom/")
        // must still be compiled natively, not handed to the JS runtime.
        // Guard with !is_in_node_modules so this branch never fires for the standard
        // node_modules/ethers, node_modules/ws etc. paths that already have their
        // own handling (is_perry_native above).
        || (!is_in_node_modules && is_in_perry_native_package(canonical));
    // #668 / node-core (#800): a *user* `.js`/`.cjs`/`.mjs` file (entry or a
    // project source, i.e. NOT under node_modules) is fed through the native
    // AOT pipeline like a `.ts` file rather than treated as a JS module. The
    // extension is not the signal — content is: most plain `.js` is valid
    // TypeScript-subset, and CommonJS shapes (`require(...)`, `module.exports`)
    // are rewritten to ESM by `cjs_wrap`, the same translation already trusted
    // for `compilePackages` targets. node_modules `.js` keeps the JS-module
    // classification (post-#1696 there is no V8 fallback, so it surfaces as an
    // unsupported-module error rather than silently running).
    let should_use_js_runtime =
        (is_js_file(canonical) && !is_in_compiled_pkg && is_in_node_modules)
            || is_declaration_file(canonical);
    Route {
        is_in_node_modules,
        is_perry_native,
        is_in_compiled_pkg,
        should_use_js_runtime,
    }
}

/// The settings source preparation reads. All are fixed before the walk.
pub(super) struct PrepInputs<'a> {
    pub(super) target: Option<&'a str>,
    pub(super) compile_packages: &'a HashSet<String>,
    pub(super) native_addon_paths: &'a BTreeMap<PathBuf, String>,
    pub(super) bunfs_root: Option<&'a Path>,
    pub(super) debug_symbols: bool,
    pub(super) cjs_demanded_exports: &'a BTreeMap<PathBuf, BTreeSet<String>>,
}

impl<'a> PrepInputs<'a> {
    pub(super) fn of(ctx: &'a CompilationContext, target: Option<&'a str>) -> Self {
        Self {
            target,
            compile_packages: &ctx.compile_packages,
            native_addon_paths: &ctx.native_addon_paths,
            bunfs_root: ctx.bunfs_root.as_deref(),
            debug_symbols: ctx.debug_symbols,
            cjs_demanded_exports: &ctx.cjs_demanded_exports,
        }
    }
}

/// Owned copy of [`PrepInputs`] for a walk that prepares modules ahead
/// (never with `--bunfs-root`), taken when the walk starts.
#[derive(PartialEq, Eq)]
struct PrepConfig {
    target: Option<String>,
    compile_packages: HashSet<String>,
    native_addon_paths: BTreeMap<PathBuf, String>,
    debug_symbols: bool,
    cjs_demanded_exports: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl PrepConfig {
    fn of(ctx: &CompilationContext, target: Option<&str>) -> Option<Self> {
        if ctx.bunfs_root.is_some() {
            return None;
        }
        Some(Self {
            target: target.map(str::to_string),
            compile_packages: ctx.compile_packages.clone(),
            native_addon_paths: ctx.native_addon_paths.clone(),
            debug_symbols: ctx.debug_symbols,
            cjs_demanded_exports: ctx.cjs_demanded_exports.clone(),
        })
    }

    /// Same settings except, possibly, the demanded CommonJS exports, which
    /// change only the modules they name.
    fn same_except_demand(&self, other: &Self) -> bool {
        self.target == other.target
            && self.compile_packages == other.compile_packages
            && self.native_addon_paths == other.native_addon_paths
            && self.debug_symbols == other.debug_symbols
    }

    fn inputs(&self) -> PrepInputs<'_> {
        PrepInputs {
            target: self.target.as_deref(),
            compile_packages: &self.compile_packages,
            native_addon_paths: &self.native_addon_paths,
            bunfs_root: None,
            debug_symbols: self.debug_symbols,
            cjs_demanded_exports: &self.cjs_demanded_exports,
        }
    }
}

/// The part of a module's classification its prepared source depends on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct PrepClass {
    /// CommonJS source is wrapped as ESM (`compilePackages` or user source).
    pub(super) wrap_eligible: bool,
    /// The file is synthesized into a module exporting its parsed JSON.
    pub(super) is_json: bool,
    /// The file is synthesized into a module exporting its text.
    pub(super) is_text: bool,
    /// The module is the compile-time entry (`require.main`).
    pub(super) is_entry: bool,
}

/// A module's source ready for parsing, with the context effects the walk
/// applies and the diagnostics it prints.
#[derive(Clone)]
pub(super) struct PreparedSource {
    pub(super) class_family_syntax: Option<perry_hir::ir::ClassFamilySyntax>,
    pub(super) source: String,
    pub(super) was_cjs_wrapped: bool,
    /// Recoverable parse warnings from the CommonJS wrap, printed by the walk.
    pub(super) messages: Vec<String>,
    /// Computed-`require` warnings from the CommonJS wrap, printed by the
    /// walk after `messages` when `CompilationContext::cjs_require_diagnostics`
    /// is on (the second walk turns it off). Always collected, so the
    /// prepared source does not depend on that setting.
    pub(super) computed_require_warnings: Vec<String>,
    /// The wrapped source under `PERRY_DEBUG_CJS_WRAP`, printed by the walk
    /// last.
    pub(super) wrap_dump: Option<String>,
    /// Built-in modules named by literal `require()` or `getBuiltinModule`
    /// sites that need the stdlib, in source order (`node:` stripped).
    pub(super) stdlib_modules: Vec<String>,
    /// Lines before the original body in the wrapped source, for debug
    /// locations (`--debug-symbols` on a wrapped module only).
    pub(super) debug_prefix_line_count: Option<u32>,
}

impl PreparedSource {
    fn bytes(&self) -> usize {
        self.source.len()
            + self
                .messages
                .iter()
                .chain(&self.computed_require_warnings)
                .chain(&self.wrap_dump)
                .map(String::len)
                .sum::<usize>()
    }
}

/// Prepare an already-read module source: JSON/text synthesis, CommonJS wrap,
/// static-require transform and the scans over the result.
pub(super) fn prepare_from_raw(
    raw_source: String,
    canonical: &Path,
    class: PrepClass,
    inputs: &PrepInputs,
) -> Result<PreparedSource> {
    // JSON module import: turn the data file into a native ESM module whose
    // default export is the parsed value.
    let raw_source = if class.is_json {
        synthesize_json_module(&raw_source, canonical)?
    } else if class.is_text {
        // #5223: text-asset import. The file's contents are exposed verbatim as
        // the module's default export (a JS string). We never TS-parse the raw
        // text — instead we synthesize `export default "<escaped-contents>";`.
        // `serde_json::to_string` of a string produces a valid double-quoted JS
        // string literal with all required escaping (newlines, quotes, control
        // chars, unicode), so the contents round-trip byte-for-byte.
        let literal = serde_json::to_string(&raw_source).map_err(|e| {
            anyhow!(
                "Failed to encode text asset {} as a string literal: {}",
                canonical.display(),
                e
            )
        })?;
        format!("export default {};\n", literal)
    } else {
        raw_source
    };
    // Issue #348: when a `compilePackages` target ships CommonJS (e.g. React
    // 18's `module.exports = require('./cjs/react.production.min.js')`),
    // rewrite the source as ESM before SWC parses it. Fire for
    // `compilePackages` targets (the original #348 case) AND for any user file
    // outside node_modules (#668 / #800): a user `.js` or `.ts` written in
    // CommonJS — `require(...)` / `module.exports` with no top-level ESM — is
    // rewritten to ESM here so `require("literal")` lands as a static namespace
    // import and flows through the normal resolution + init-order + codegen
    // path. A file that already has top-level `import`/`export` is not
    // CommonJS (`is_commonjs` returns false) and is left untouched.
    // The explicit CommonJS entry format supplies its wrapper bindings even
    // when the body only reads them and has no heuristic CJS source tokens.
    let is_cjs_entry = class.is_entry
        && canonical
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("cjs");
    let was_cjs_wrapped = class.wrap_eligible && (is_cjs_entry || is_commonjs(&raw_source));
    let class_family_syntax = Some(super::super::cjs_wrap::family_syntax::analyze(&raw_source));
    let mut messages = Vec::new();
    let mut computed_require_warnings = Vec::new();
    let mut wrap_dump = None;
    // #5247 / #7036: when source locations are requested, capture where the
    // original module body lands inside the wrapped output so debug frames and
    // opt reports can map a wrapped-coordinate byte offset back to an
    // original-source line. The default build does no extra work.
    let mut body_prefix_lines: Option<u32> = None;
    let source = if was_cjs_wrapped {
        let (wrapped, body_off) = wrap_commonjs_with_addon_paths(
            &raw_source,
            canonical,
            inputs.target,
            class.is_entry,
            Some(inputs.compile_packages),
            Some(inputs.native_addon_paths),
            inputs.cjs_demanded_exports.get(canonical),
            &mut messages,
            Some(&mut computed_require_warnings),
        );
        if std::env::var("PERRY_DEBUG_CJS_WRAP").is_ok() {
            wrap_dump = Some(format!(
                "=== CJS WRAP for {} ===\n{}\n=== END ===",
                canonical.display(),
                wrapped
            ));
        }
        if inputs.debug_symbols {
            // Preserve original-source locations after injecting the wrapper.
            body_prefix_lines = body_off.map(|off| {
                wrapped.as_bytes()[..off]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count() as u32
            });
        }
        wrapped
    } else {
        raw_source
    };
    // #5247: the static-require transform may prepend `import * as` lines,
    // shifting BOTH the prefix and the body down by the same number of lines.
    // Capture the wrapped line count, run the transform, then add the line
    // delta to the prefix so the wrapped-line → original-line subtraction is
    // computed against the FINAL parsed source.
    let lines_before_transform = source.bytes().filter(|&b| b == b'\n').count();
    let source = transform_static_literal_requires_with_bunfs(
        &source,
        inputs.compile_packages,
        canonical.parent().unwrap_or_else(|| Path::new(".")),
        inputs.bunfs_root,
        !was_cjs_wrapped,
    );
    // #8547: a builtin reached through `require("http")` never appears in the
    // ESM import walk, so `needs_stdlib` stayed false, the link came out
    // runtime-only, `perry-stdlib`'s dispatch init never ran, and every
    // stdlib-backed export returned `undefined` (`http.createServer` was the
    // reported case; the bug is not http-specific). The lowered body cannot
    // answer this — the CJS shim resolves the name through a generated runtime
    // switch that contains EVERY builtin — so recover it from the literal call
    // sites, which is what `extract_require_specifiers` already finds for CJS
    // wrapping. Dynamic `require(someVar)` stays undetectable and unchanged.
    let mut stdlib_modules = Vec::new();
    for spec in extract_require_specifiers(&source) {
        if perry_hir::requires_stdlib(&spec) {
            stdlib_modules.push(spec.strip_prefix("node:").unwrap_or(&spec).to_string());
        }
    }
    // #11616: the same for `process.getBuiltinModule(id)` — a module asked for
    // by name was never imported, so auto-optimize stripped it and its methods
    // returned `undefined`. The id is a runtime string; take every quoted
    // `node:` literal in a module that mentions `getBuiltinModule`.
    if source.contains("getBuiltinModule") {
        for spec in node_specifier_literals(&source) {
            if perry_hir::requires_stdlib(spec) {
                stdlib_modules.push(spec.strip_prefix("node:").unwrap_or(spec).to_string());
            }
        }
    }
    let debug_prefix_line_count = match body_prefix_lines {
        Some(prefix_lines) if was_cjs_wrapped && inputs.debug_symbols => {
            let lines_after_transform = source.bytes().filter(|&b| b == b'\n').count();
            let added_lines = lines_after_transform.saturating_sub(lines_before_transform) as u32;
            Some(prefix_lines + added_lines)
        }
        _ => None,
    };
    Ok(PreparedSource {
        class_family_syntax,
        source,
        was_cjs_wrapped,
        messages,
        computed_require_warnings,
        wrap_dump,
        stdlib_modules,
        debug_prefix_line_count,
    })
}

/// A plain source file (not an addon, Wasm or file-loader asset) read and,
/// when nothing in it needs the walk, prepared.
pub(super) enum ReadSource {
    /// The source names both `import.meta` and `require`: the
    /// `import.meta.require` rewrite records addons in the context, so the
    /// walk prepares this module itself from the bytes read here.
    NeedsWalk(String),
    Prepared {
        /// `extract_require_specifiers` of the raw source, in order.
        require_specs: Vec<String>,
        /// The relative `require` targets with a `.node` extension, in order.
        node_targets: Vec<PathBuf>,
        prepared: Result<PreparedSource>,
    },
}

/// Read and prepare a plain source file. Only valid without `--bunfs-root`:
/// with it the walk registers literal asset paths from the raw source.
pub(super) fn prepare_plain(
    canonical: &Path,
    class: PrepClass,
    inputs: &PrepInputs,
) -> Result<ReadSource> {
    debug_assert!(inputs.bunfs_root.is_none());
    let raw_source = std::fs::read_to_string(canonical)
        .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;
    if import_meta_require_may_apply(&raw_source) {
        return Ok(ReadSource::NeedsWalk(raw_source));
    }
    // CJS wrapping consumes literal `require()` sites and replaces them with
    // generated loader calls. The walk queues native targets before that
    // rewrite so the graph still authenticates and packages the selected
    // `.node` binary.
    let require_specs = extract_require_specifiers(&raw_source);
    let node_targets = require_specs
        .iter()
        .filter_map(|specifier| {
            super::super::resolve::resolve_relative_import_path(specifier, canonical)
        })
        .filter(|target| {
            target.extension().and_then(|extension| extension.to_str()) == Some("node")
        })
        .collect();
    let prepared = prepare_from_raw(raw_source, canonical, class, inputs);
    Ok(ReadSource::Prepared {
        require_specs,
        node_targets,
        prepared,
    })
}

/// A worker's result for one module.
pub(super) struct Prefetched {
    /// The classification the source was prepared for.
    pub(super) class: PrepClass,
    pub(super) read: Result<ReadSource>,
}

impl Prefetched {
    fn bytes(&self) -> usize {
        match &self.read {
            Ok(ReadSource::NeedsWalk(raw)) => raw.len(),
            Ok(ReadSource::Prepared { prepared, .. }) => {
                prepared.as_ref().map_or(0, PreparedSource::bytes)
            }
            Err(_) => 0,
        }
    }
}

/// Unconsumed prepared bytes above which workers stop starting new modules.
const READY_BYTES_LIMIT: usize = 512 * 1024 * 1024;
const MAX_WORKERS: usize = 4;

/// Worker threads for a walk: `CompilationContext::source_prep_workers`, or
/// one less than the available parallelism (the walk thread is busy too),
/// capped at 4. 0 prepares every module on the walk thread. On Next.js four
/// workers already leave the walk no module to wait for, and more only
/// compete with the walk thread for cores.
pub(super) fn worker_count(ctx: &CompilationContext) -> usize {
    ctx.source_prep_workers.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .saturating_sub(1)
            .min(MAX_WORKERS)
    })
}

/// Owned copy of the settings workers read, taken when a walk starts.
pub(super) struct PrepSettings {
    config: PrepConfig,
    entry_canonical: Option<PathBuf>,
    aot_discovered_modules: HashSet<PathBuf>,
    compile_package_dirs: BTreeSet<PathBuf>,
    /// Modules an earlier walk prepared, which this walk takes from there.
    earlier: HashSet<PathBuf>,
}

impl PrepSettings {
    /// `None` when modules cannot be prepared ahead (`--bunfs-root`).
    pub(super) fn of(
        ctx: &CompilationContext,
        target: Option<&str>,
        sources: &WalkSources,
    ) -> Option<Self> {
        Some(Self {
            config: PrepConfig::of(ctx, target)?,
            entry_canonical: ctx.entry_canonical.clone(),
            aot_discovered_modules: ctx.aot_discovered_modules.clone(),
            compile_package_dirs: ctx.compile_package_dirs.clone(),
            earlier: match sources {
                WalkSources::Reuse(earlier) => earlier.modules.keys().cloned().collect(),
                WalkSources::Keep(_) => HashSet::new(),
            },
        })
    }

    /// Prepare `canonical` the way the walk would if it routes the module to
    /// a plain native read; `None` for modules the walk handles otherwise.
    fn prefetch(&self, canonical: &Path) -> Option<Prefetched> {
        if canonical.extension().and_then(|ext| ext.to_str()) == Some("node")
            || is_wasm_asset(canonical)
            || self.earlier.contains(canonical)
        {
            return None;
        }
        let route = route(
            canonical,
            &RouteInputs {
                bunfs_root: None,
                aot_discovered_modules: &self.aot_discovered_modules,
                compile_packages: &self.config.compile_packages,
                compile_package_dirs: &self.compile_package_dirs,
            },
        );
        if route.should_use_js_runtime {
            return None;
        }
        let class = PrepClass {
            wrap_eligible: route.is_in_compiled_pkg || !route.is_in_node_modules,
            is_json: canonical.extension().and_then(|e| e.to_str()) == Some("json"),
            is_text: is_recognized_text_asset(canonical),
            is_entry: self.entry_canonical.as_deref() == Some(canonical),
        };
        Some(Prefetched {
            class,
            read: prepare_plain(canonical, class, &self.config.inputs()),
        })
    }
}

/// A plain module's prepared source as a walk consumed it.
struct KeptSource {
    class: PrepClass,
    read: KeptRead,
    prepared: PreparedSource,
}

enum KeptRead {
    /// Prepared straight from the file ([`prepare_plain`]).
    Plain {
        require_specs: Vec<String>,
        node_targets: Vec<PathBuf>,
    },
    /// Prepared by the walk from this source, the file after the
    /// `import.meta.require` rewrite. The rewrite records addons in the
    /// context, so every walk runs it; its output decides reuse.
    AfterRewrite { raw_source: String },
}

/// The plain module sources one walk prepared, keyed by canonical path, with
/// the settings they were prepared for. The compile's first walk returns
/// them and its second walk takes them as an input; nothing else holds them.
#[derive(Default)]
pub(crate) struct PreparedSources {
    config: Option<PrepConfig>,
    modules: HashMap<PathBuf, KeptSource>,
}

impl PreparedSources {
    /// Modules held.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.modules.len()
    }

    /// Record `read`, prepared for `class` with the settings the walk
    /// started with (see [`WalkSources::start`]).
    pub(super) fn keep(&mut self, canonical: &Path, class: PrepClass, read: &ReadSource) {
        if self.config.is_none() {
            return;
        }
        if let ReadSource::Prepared {
            require_specs,
            node_targets,
            prepared: Ok(prepared),
        } = read
        {
            self.modules.insert(
                canonical.to_path_buf(),
                KeptSource {
                    class,
                    read: KeptRead::Plain {
                        require_specs: require_specs.clone(),
                        node_targets: node_targets.clone(),
                    },
                    prepared: prepared.clone(),
                },
            );
        }
    }

    /// Record `prepared`, prepared by the walk for `class` from `raw_source`
    /// (the file after the `import.meta.require` rewrite).
    pub(super) fn keep_after_rewrite(
        &mut self,
        canonical: &Path,
        class: PrepClass,
        raw_source: String,
        prepared: &PreparedSource,
    ) {
        if self.config.is_none() {
            return;
        }
        self.modules.insert(
            canonical.to_path_buf(),
            KeptSource {
                class,
                read: KeptRead::AfterRewrite { raw_source },
                prepared: prepared.clone(),
            },
        );
    }

    /// Take the source prepared straight from the file for `canonical`, if
    /// it was prepared for `class` (the settings were checked when the walk
    /// started). A source prepared after the rewrite stays for
    /// [`Self::take_after_rewrite`].
    pub(super) fn take(&mut self, canonical: &Path, class: PrepClass) -> Option<ReadSource> {
        if matches!(
            self.modules.get(canonical)?.read,
            KeptRead::AfterRewrite { .. }
        ) {
            return None;
        }
        let kept = self.modules.remove(canonical)?;
        let KeptRead::Plain {
            require_specs,
            node_targets,
        } = kept.read
        else {
            unreachable!("checked above");
        };
        (kept.class == class).then(|| ReadSource::Prepared {
            require_specs,
            node_targets,
            prepared: Ok(kept.prepared),
        })
    }

    /// Take the source the earlier walk prepared for `canonical` after the
    /// `import.meta.require` rewrite, if it was prepared for `class` from
    /// exactly `raw_source`, this walk's rewrite output.
    pub(super) fn take_after_rewrite(
        &mut self,
        canonical: &Path,
        class: PrepClass,
        raw_source: &str,
    ) -> Option<PreparedSource> {
        let kept = self.modules.remove(canonical)?;
        match kept.read {
            KeptRead::AfterRewrite {
                raw_source: kept_raw,
            } if kept.class == class && kept_raw == raw_source => Some(kept.prepared),
            _ => None,
        }
    }
}

/// What a walk does with prepared module sources besides using them.
pub(crate) enum WalkSources<'a> {
    /// Keep every plain module source this walk prepares, for a later walk.
    Keep(&'a mut PreparedSources),
    /// Use the sources an earlier walk prepared, where they were prepared for
    /// this walk's settings and classification.
    Reuse(&'a mut PreparedSources),
}

impl WalkSources<'_> {
    /// Called when a walk starts, before anything is prepared. Sources are
    /// kept for one set of settings and reused only under the same settings;
    /// the walk-thread path reads the same settings from the context, and
    /// nothing changes them while a walk runs.
    pub(super) fn start(&mut self, ctx: &CompilationContext, target: Option<&str>) {
        match self {
            WalkSources::Keep(sources) => {
                let config = PrepConfig::of(ctx, target);
                if sources.config != config {
                    sources.modules.clear();
                    sources.config = config;
                }
            }
            WalkSources::Reuse(sources) => {
                let config = PrepConfig::of(ctx, target);
                match (&config, &sources.config) {
                    (Some(new), Some(old)) if new.same_except_demand(old) => {
                        // Re-prepare only the modules whose demanded exports
                        // changed; every other kept source still applies.
                        sources.modules.retain(|path, _| {
                            new.cjs_demanded_exports.get(path) == old.cjs_demanded_exports.get(path)
                        });
                    }
                    _ => sources.modules.clear(),
                }
                sources.config = config;
            }
        }
    }
}

enum Slot {
    InFlight {
        released: bool,
    },
    /// A worker's result (`None`: not preparable ahead, or the worker
    /// panicked) and its byte count.
    Ready(Option<Box<Prefetched>>, usize),
    /// The walk claimed or released the module.
    Taken,
}

#[derive(Default)]
struct State {
    /// Paths the walk pushed; the last pushed is the walk's next pop.
    walk_queue: Vec<PathBuf>,
    /// Paths guessed from prepared sources; taken only when `walk_queue` is
    /// empty, newest first.
    guess_queue: Vec<PathBuf>,
    /// By canonical path.
    slots: HashMap<PathBuf, Slot>,
    ready_bytes: usize,
    finished: bool,
}

/// Modules prepared ahead of one walk. Lives for that walk only.
#[derive(Default)]
pub(super) struct SourcePrefetch {
    state: Mutex<State>,
    changed: Condvar,
}

impl SourcePrefetch {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Queue paths the walk just pushed, given in the walk's push order (the
    /// last one is popped first).
    pub(super) fn submit<'p>(&self, pushed: impl IntoIterator<Item = &'p PathBuf>) {
        let mut state = self.lock();
        let before = state.walk_queue.len();
        state.walk_queue.extend(pushed.into_iter().cloned());
        if state.walk_queue.len() != before {
            self.changed.notify_all();
        }
    }

    /// The walk is collecting `canonical`: its prepared value if a worker
    /// prepared it (waiting for one in flight), else `None` and the walk
    /// prepares it inline.
    pub(super) fn claim(&self, canonical: &Path) -> Option<Prefetched> {
        let mut state = self.lock();
        loop {
            match state.slots.get(canonical) {
                Some(Slot::InFlight { .. }) => {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poison| poison.into_inner());
                }
                Some(Slot::Ready(..)) => {
                    let Some(Slot::Ready(value, bytes)) =
                        state.slots.insert(canonical.to_path_buf(), Slot::Taken)
                    else {
                        unreachable!("slot checked above");
                    };
                    state.ready_bytes -= bytes;
                    self.changed.notify_all();
                    return value.map(|value| *value);
                }
                Some(Slot::Taken) => return None,
                None => {
                    state.slots.insert(canonical.to_path_buf(), Slot::Taken);
                    return None;
                }
            }
        }
    }

    /// The walk is done with `canonical` (collected it, or will never need a
    /// prepared value for it): drop anything prepared for it.
    pub(super) fn release(&self, canonical: &Path) {
        let mut state = self.lock();
        match state.slots.get_mut(canonical) {
            Some(Slot::InFlight { released }) => *released = true,
            Some(Slot::Ready(_, bytes)) => {
                let bytes = *bytes;
                state.ready_bytes -= bytes;
                state.slots.insert(canonical.to_path_buf(), Slot::Taken);
                self.changed.notify_all();
            }
            Some(Slot::Taken) => {}
            None => {
                state.slots.insert(canonical.to_path_buf(), Slot::Taken);
            }
        }
    }

    /// Stop the workers once their current module is done.
    pub(super) fn finish(&self) {
        self.lock().finished = true;
        self.changed.notify_all();
    }

    pub(super) fn work(&self, settings: &PrepSettings) {
        loop {
            let path = {
                let mut state = self.lock();
                loop {
                    if state.finished {
                        return;
                    }
                    if state.ready_bytes <= READY_BYTES_LIMIT {
                        if let Some(path) = state.walk_queue.pop() {
                            break path;
                        }
                        if let Some(path) = state.guess_queue.pop() {
                            break path;
                        }
                    }
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poison| poison.into_inner());
                }
            };
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            {
                let mut state = self.lock();
                if state.slots.contains_key(&canonical) {
                    continue;
                }
                state
                    .slots
                    .insert(canonical.clone(), Slot::InFlight { released: false });
            }
            // A panic here is the walk's to report: it prepares the module
            // inline, the same function on the same input.
            let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                settings.prefetch(&canonical)
            }))
            .ok()
            .flatten();
            // The module's own relative imports are likely the walk's next
            // modules. A wrong guess costs only work: results are keyed by
            // canonical path and are pure functions of it.
            let guesses = match &value {
                Some(Prefetched {
                    read:
                        Ok(ReadSource::Prepared {
                            prepared: Ok(prepared),
                            ..
                        }),
                    ..
                }) => collect_js_module_imports(&canonical, &prepared.source),
                _ => Vec::new(),
            };
            let mut state = self.lock();
            if let Some(slot) = state.slots.get_mut(&canonical) {
                if matches!(slot, Slot::InFlight { released: true }) {
                    *slot = Slot::Taken;
                } else {
                    let bytes = value.as_ref().map_or(0, Prefetched::bytes);
                    *slot = Slot::Ready(value.map(Box::new), bytes);
                    state.ready_bytes += bytes;
                }
            }
            state.guess_queue.extend(guesses.into_iter().rev());
            self.changed.notify_all();
        }
    }
}
