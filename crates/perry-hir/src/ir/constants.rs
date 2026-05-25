//! Constants, native-module config, and per-thread overrides used during HIR
//! lowering. Split out of the former monolithic `ir.rs` for readability —
//! everything here is `pub use`-re-exported from `super`.

/// TypedArray element-kind tags. Must match `crates/perry-runtime/src/typedarray.rs`.
pub const TYPED_ARRAY_KIND_INT8: u8 = 0;
pub const TYPED_ARRAY_KIND_UINT8: u8 = 1;
pub const TYPED_ARRAY_KIND_INT16: u8 = 2;
pub const TYPED_ARRAY_KIND_UINT16: u8 = 3;
pub const TYPED_ARRAY_KIND_INT32: u8 = 4;
pub const TYPED_ARRAY_KIND_UINT32: u8 = 5;
pub const TYPED_ARRAY_KIND_FLOAT32: u8 = 6;
pub const TYPED_ARRAY_KIND_FLOAT64: u8 = 7;
/// Uint8ClampedArray: 1-byte elements, stores via ToUint8Clamp (not truncate-wrap).
pub const TYPED_ARRAY_KIND_UINT8_CLAMPED: u8 = 8;

/// Map a class name (e.g. "Int32Array") to its `TYPED_ARRAY_KIND_*` tag.
pub fn typed_array_kind_for_name(name: &str) -> Option<u8> {
    match name {
        "Int8Array" => Some(TYPED_ARRAY_KIND_INT8),
        "Uint8Array" => Some(TYPED_ARRAY_KIND_UINT8),
        "Uint8ClampedArray" => Some(TYPED_ARRAY_KIND_UINT8_CLAMPED),
        "Int16Array" => Some(TYPED_ARRAY_KIND_INT16),
        "Uint16Array" => Some(TYPED_ARRAY_KIND_UINT16),
        "Int32Array" => Some(TYPED_ARRAY_KIND_INT32),
        "Uint32Array" => Some(TYPED_ARRAY_KIND_UINT32),
        "Float32Array" => Some(TYPED_ARRAY_KIND_FLOAT32),
        "Float64Array" => Some(TYPED_ARRAY_KIND_FLOAT64),
        _ => None,
    }
}

/// Known native module names that map to stdlib implementations.
/// These are npm packages that have native Rust replacements.
///
/// Source of truth lives in `perry-api-manifest::NATIVE_MODULES`
/// (#463 — the manifest is the unified source for compile-time
/// unimplemented-API checks, codegen dispatch, and docs/.d.ts emit
/// per #465). This re-export keeps existing
/// `perry_hir::NATIVE_MODULES` callers working; new code should
/// import from `perry_api_manifest` directly.
pub const NATIVE_MODULES: &[&str] = perry_api_manifest::NATIVE_MODULES;

thread_local! {
    /// Refs #665: per-thread set of packages the user opted into via
    /// `perry.compilePackages`. When non-empty, `is_native_module` returns
    /// false for any path whose package name is in this set — so HIR
    /// lowering treats the import as a regular ESM/CJS module (running
    /// cjs_wrap, registering classes as imported rather than native), and
    /// `obj.method` on a compile-package-overridden class lowers as a
    /// real PropertyGet instead of a zero-arg `NativeMethodCall` (which
    /// would have called the missing FFI getter and returned `0.0`).
    ///
    /// The compiler driver sets this thread-local before each
    /// `lower_module_full` invocation and clears it after. Rayon's
    /// thread pool gives each worker its own copy.
    static COMPILE_PACKAGES_OVERRIDE: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Set the per-thread override of which packages to treat as
/// non-native during HIR lowering. Called by the compiler driver before
/// each `lower_module_full` invocation. Refs #665.
pub fn set_compile_packages_override(set: std::collections::HashSet<String>) {
    COMPILE_PACKAGES_OVERRIDE.with(|cell| *cell.borrow_mut() = set);
}

/// Clear the per-thread override. Refs #665.
pub fn clear_compile_packages_override() {
    COMPILE_PACKAGES_OVERRIDE.with(|cell| cell.borrow_mut().clear());
}

// ---- #503 dynamic-stdlib-dispatch refusal config ----

thread_local! {
    /// #503: when true, HIR lowering refuses compile-time `obj[expr]()` /
    /// `obj[expr]` on known stdlib namespace receivers (`process`, `fs`,
    /// `crypto`, `child_process`, `net`, `os`, `path`, `http`, `https`,
    /// `http2`, `stream`, `url`, `util`, `events`, `dns`, `tls`,
    /// `querystring`, `zlib`, `async_hooks`, `readline`, `string_decoder`,
    /// `tty`, `worker_threads`) unless the index is a string literal or
    /// compile-time-foldable string. (`buffer` is intentionally excluded —
    /// `Buffer` is a constructor, not a namespace object.) Catches the
    /// dispatch-by-string class of supply-chain evasion. The canonical
    /// list lives in `lower/expr_member.rs::STDLIB_NAMESPACE_NAMES`. Set
    /// to false by `perry.allowDynamicStdlibDispatch: true` or
    /// `PERRY_ALLOW_DYNAMIC_STDLIB=1`.
    static REFUSE_DYNAMIC_STDLIB_DISPATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };

    /// #503: per-thread set of npm package names that opted out of the
    /// dynamic-stdlib-dispatch refusal (`perry.allowDynamicStdlibDispatch:
    /// ["@scope/pkg", ...]`). When the currently-lowering source file
    /// belongs to one of these packages, the check is skipped.
    static ALLOW_DYNAMIC_STDLIB_PACKAGES: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    /// #503: source text of the module currently being lowered. Used by
    /// the dynamic-dispatch check to look up `// @perry-allow-dynamic`
    /// line annotations adjacent to violation sites. Set once per
    /// `lower_module_full` invocation by the compiler driver.
    static CURRENT_MODULE_SOURCE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// #503: enable (true) or disable (false) the dynamic-stdlib-dispatch
/// refusal pass. Default is true (refusal active). The compile driver
/// calls this once with the resolved configuration before kicking off
/// HIR lowering for the build.
pub fn set_refuse_dynamic_stdlib_dispatch(refuse: bool) {
    REFUSE_DYNAMIC_STDLIB_DISPATCH.with(|c| c.set(refuse));
}

/// #503: is the dynamic-stdlib-dispatch refusal pass currently enabled
/// for this thread?
pub fn refuse_dynamic_stdlib_dispatch_enabled() -> bool {
    REFUSE_DYNAMIC_STDLIB_DISPATCH.with(|c| c.get())
}

/// #503: install the per-thread allow-list of package names whose
/// modules may legitimately use dynamic dispatch on stdlib namespaces.
pub fn set_allow_dynamic_stdlib_packages(set: std::collections::HashSet<String>) {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| *c.borrow_mut() = set);
}

/// #503: clear the per-thread allow-list.
pub fn clear_allow_dynamic_stdlib_packages() {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| c.borrow_mut().clear());
}

/// #503: is the given package name on the allow-list? `package_name_of`
/// is the canonical extractor (scope-aware).
pub fn dynamic_stdlib_allowed_for_package(pkg: &str) -> bool {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| c.borrow().contains(pkg))
}

/// #503: install the source text of the module about to be lowered.
/// The dynamic-dispatch check reads this to look up site annotations
/// without re-reading the file from disk.
pub fn set_current_module_source(src: String) {
    CURRENT_MODULE_SOURCE.with(|c| *c.borrow_mut() = Some(src));
}

/// #503: clear the source-text thread-local.
pub fn clear_current_module_source() {
    CURRENT_MODULE_SOURCE.with(|c| *c.borrow_mut() = None);
}

/// #1678: resolve `byte_offset` to a 1-based line number in the
/// currently-installed module source (the same `CURRENT_MODULE_SOURCE`
/// the dynamic-dispatch check uses). Returns `None` when no source is
/// installed or the offset is out of range. Used by the eval/Function
/// classifier to print `file:line` provenance in its refusal diagnostic
/// and `--diag` instrumentation without threading a `SourceMap` into
/// HIR lowering.
pub fn current_module_line_at(byte_offset: u32) -> Option<usize> {
    CURRENT_MODULE_SOURCE.with(|cell| {
        let borrowed = cell.borrow();
        let src = borrowed.as_ref()?;
        let offset = byte_offset as usize;
        if offset > src.len() {
            return None;
        }
        // Line number = 1 + count of newlines before the offset.
        Some(1 + src[..offset].bytes().filter(|&b| b == b'\n').count())
    })
}

/// #503: look up `// @perry-allow-dynamic` near `byte_offset` in the
/// currently-installed module source. Returns true if the annotation
/// appears on the same line as the offending site, or on any of the
/// contiguous comment/blank lines immediately above it (so authors can
/// stack other line comments like `// @ts-ignore` alongside the
/// annotation without losing the opt-out).
pub fn current_module_has_allow_dynamic_at(byte_offset: u32) -> bool {
    CURRENT_MODULE_SOURCE.with(|cell| {
        let borrowed = cell.borrow();
        let Some(src) = borrowed.as_ref() else {
            return false;
        };
        let offset = byte_offset as usize;
        if offset > src.len() {
            return false;
        }
        // Walk back to the start of the line containing `offset`.
        let line_start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = src[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(src.len());
        if src[line_start..line_end].contains("@perry-allow-dynamic") {
            return true;
        }
        // Walk up through contiguous comment-only and blank lines.
        // A "comment-only" line trims to either an empty string or a
        // string starting with `//`. The walk stops at the first line
        // that contains executable code — anything stronger would let
        // an annotation drift arbitrarily far from its target.
        let mut cursor = line_start;
        while cursor > 0 {
            let prev_end = cursor - 1; // index of '\n'
            let prev_start = src[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prev = &src[prev_start..prev_end];
            let trimmed = prev.trim();
            let is_comment_or_blank = trimmed.is_empty() || trimmed.starts_with("//");
            if !is_comment_or_blank {
                return false;
            }
            if prev.contains("@perry-allow-dynamic") {
                return true;
            }
            cursor = prev_start;
        }
        false
    })
}

/// #503: extract the package name from a source file path, if any. The
/// path is searched for the rightmost `node_modules/` segment; the
/// segment(s) immediately following it form the package name
/// (scope-aware). Returns `None` for user-source files (no
/// `node_modules/` in the path).
pub fn package_name_for_source_path(source_path: &str) -> Option<&str> {
    let idx = source_path.rfind("node_modules/")?;
    let after = &source_path[idx + "node_modules/".len()..];
    if let Some(stripped) = after.strip_prefix('@') {
        // Scoped: `@scope/pkg/...` → `@scope/pkg`
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            return None;
        }
        let end = idx + "node_modules/".len() + 1 + scope.len() + 1 + pkg.len();
        Some(&source_path[idx + "node_modules/".len()..end])
    } else {
        let pkg = after.split('/').next()?;
        if pkg.is_empty() {
            None
        } else {
            Some(pkg)
        }
    }
}

/// Parse the package name out of an import specifier. Mirrors the
/// `parse_package_specifier` helper in `crates/perry/src/commands/compile/resolve.rs`
/// but lives here so `is_native_module` doesn't gain a perry-crate dep.
fn package_name_of(path: &str) -> &str {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    if let Some(stripped) = normalized.strip_prefix('@') {
        // Scoped: `@scope/pkg/subpath` → `@scope/pkg`
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            normalized
        } else {
            // Return a slice covering "@scope/pkg" from the original normalized
            // string. Since `stripped = &normalized[1..]`, the scope segment
            // ends at `1 + scope.len()` and "@scope/pkg" ends at
            // `1 + scope.len() + 1 + pkg.len()`.
            let end = 1 + scope.len() + 1 + pkg.len();
            &normalized[..end]
        }
    } else {
        // Regular: `pkg/subpath` → `pkg`
        normalized.split('/').next().unwrap_or(normalized)
    }
}

/// Check if a module path refers to a native stdlib module.
///
/// Refs #665: when the user has opted the package into
/// `perry.compilePackages`, this returns false even for paths that
/// match the built-in NATIVE_MODULES manifest — the user's
/// `node_modules` copy will be compiled from source and HIR lowering
/// must not register the import as a native module (which would
/// cascade into `obj.prop` being lowered as a zero-arg FFI getter call
/// instead of a real PropertyGet → bound-method-closure).
pub fn is_native_module(path: &str) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    if !NATIVE_MODULES.contains(&normalized) {
        return false;
    }
    let pkg = package_name_of(path);
    let overridden = COMPILE_PACKAGES_OVERRIDE.with(|cell| cell.borrow().contains(pkg));
    !overridden
}

/// Check if a module path refers to a native module, including external native libraries.
/// External modules are provided by packages with `perry.nativeLibrary` in package.json.
pub fn is_native_module_with_externals(path: &str, externals: &[String]) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized) || externals.iter().any(|ext| ext == normalized)
}

/// Check if a native module import requires linking perry-stdlib.
/// Returns false for modules that are handled entirely by perry-runtime.
///
/// `net` is intentionally absent from `RUNTIME_ONLY_MODULES` so this
/// returns true for `import 'net'` — the auto-optimizer needs that to
/// enable the `net` feature on perry-stdlib.
pub fn requires_stdlib(module: &str) -> bool {
    if !is_native_module(module) {
        return false;
    }
    !perry_api_manifest::is_runtime_only_module(module)
}

/// The kind of module being imported, determining how it's executed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleKind {
    /// Native TypeScript compiled to machine code (default for .ts/.tsx files)
    #[default]
    NativeCompiled,
    /// Native Rust stdlib implementation (mysql2, pg, etc.)
    NativeRust,
    /// V8-interpreted JavaScript (fallback for .js modules)
    /// This requires explicit opt-in and user confirmation
    Interpreted,
}

/// POSIX credential accessor kind. process.{getuid,geteuid,getgid,getegid}()
/// (#1408) all share runtime/codegen plumbing — one HIR variant carries
/// the kind so expr.rs doesn't grow by four near-identical variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosixCredentialKind {
    Uid,
    Euid,
    Gid,
    Egid,
}

/// Whether a module is initialized eagerly (at program start, in topo order
/// across static imports) or lazily (on first dynamic `import()` resolving
/// to it). See issue #100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleInitKind {
    /// Reachable from the entry through at least one static-import chain.
    #[default]
    Eager,
    /// Every path from the entry to this module goes through a dynamic
    /// `import()` edge — init runs on first dispatch.
    Deferred,
}

/// Determine the module kind for a given import path
pub fn determine_module_kind(source: &str, resolved_path: Option<&std::path::Path>) -> ModuleKind {
    // First check if it's a native Rust stdlib module
    if is_native_module(source) {
        return ModuleKind::NativeRust;
    }

    // Check the resolved path extension
    if let Some(path) = resolved_path {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "ts" | "tsx" => return ModuleKind::NativeCompiled,
                "js" | "mjs" | "cjs" => return ModuleKind::Interpreted,
                _ => {}
            }
        }
    }

    // Default to native compiled (assume TypeScript)
    ModuleKind::NativeCompiled
}

/// Unique identifier for a class
pub type ClassId = u32;

/// Unique identifier for an enum
pub type EnumId = u32;

/// Unique identifier for an interface
pub type InterfaceId = u32;

/// Unique identifier for a type alias
pub type TypeAliasId = u32;
