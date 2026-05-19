//! HIR → LLVM IR compilation entry point.
//!
//! Public contract:
//!
//! ```ignore
//! let opts = CompileOptions { target: None, is_entry_module: true };
//! let object_bytes: Vec<u8> = perry_codegen::compile_module(&hir, opts)?;
//! ```
//!
//! The returned bytes are a regular object file produced by `clang -c`.
//! Perry's linking stage in `crates/perry/src/commands/compile.rs`
//! links them against `libperry_runtime.a` and `libperry_stdlib.a`.
//!
//! Currently supported (Phases 1, 2, 2.1, A-strings):
//!
//! - User functions with typed `double` ABI
//! - Recursive and forward calls via `FuncRef`
//! - If/else, for loops, let, return
//! - Binary arithmetic (add/sub/mul/div/mod) and compare
//! - Update (++/--) and LocalSet
//! - `Date.now()` via `js_date_now`
//! - **String literals** via the hoisted `StringPool` (one allocation per
//!   literal at module init time, registered as a permanent GC root via
//!   `js_gc_register_global_root`; use sites are a single `load`)
//! - `console.log(<expr>)` — uses `js_console_log_number` for static number
//!   literals (optimized path) and `js_console_log_dynamic` for everything
//!   else (NaN-tag dispatch at runtime)
//!
//! Anything else (objects, arrays, classes, closures, async, imports, …)
//! errors with an actionable "Phase X not yet supported" message.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use perry_hir::{Function, Module as HirModule};

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::runtime_decls;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I32, I64, I8, PTR, VOID};

/// Per-application metadata read from `perry.toml` by the CLI and baked into
/// compile-time system APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMetadata {
    pub version: String,
    pub build_number: i64,
    pub bundle_id: String,
}

impl Default for AppMetadata {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            build_number: 1,
            bundle_id: "com.perry.app".to_string(),
        }
    }
}

/// Options controlling code generation for a single module.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Target triple override. `None` uses the host default.
    pub target: Option<String>,
    /// Whether this module is the program entry point. When true, codegen
    /// emits a `main` function that calls `js_gc_init`, the string pool
    /// init, every non-entry module's `<prefix>__init`, then the entry
    /// module's own top-level statements.
    pub is_entry_module: bool,
    /// Prefixes of every non-entry module in the program. Only consulted
    /// when `is_entry_module = true` — `main` calls `<prefix>__init` for
    /// each one in order before running its own init statements. The
    /// order matches Perry's existing topological sort (set up by the
    /// CLI driver in `crates/perry/src/commands/compile.rs`).
    pub non_entry_module_prefixes: Vec<String>,
    /// For each imported function name in this module, the prefix of the
    /// source module that exports it. Used by `ExternFuncRef` lowering
    /// in `lower_call` to generate the correct cross-module call to
    /// `perry_fn_<source_prefix>__<funcname>`. Built by the CLI driver
    /// from each module's `hir.imports` table.
    pub import_function_prefixes: std::collections::HashMap<String, String>,
    /// Issue #678: for imports that traverse a re-export rename
    /// (e.g. `export { default as render } from './render.js'`), maps the
    /// consumer-visible name (`render`) to the actual export name in the
    /// origin module (`default`). Used by every `perry_fn_<src>__<name>`
    /// symbol-construction site so the suffix matches what the origin
    /// module actually emits. Absent entries (the common case) mean the
    /// name in origin matches the consumer's imported name; callers should
    /// treat a missing entry as identity. Without this, `import { Box }
    /// from "ink"` lowered to `perry_fn_<components_Box_js>__Box` but the
    /// origin module emitted `perry_fn_<components_Box_js>__default` (Box.js
    /// has `const Box = ...; export default Box`), and the linker failed
    /// with `Undefined symbols: _perry_fn_..._Box`.
    pub import_function_origin_names: std::collections::HashMap<String, String>,
    /// Issue #678 followup: imports of names from `ModuleKind::Interpreted`
    /// (V8-fallback) modules — `consumer_name → module_specifier`. Sparse
    /// map, only populated for V8-routed imports. When a name appears here
    /// the codegen sidesteps `perry_fn_<src>__<name>` symbol formation and
    /// emits a `js_call_v8_export(specifier, name, args, argc)` bridge call
    /// instead. Without this, native modules that imported from a JS
    /// fallback (e.g. `ink` pulled in by yoga-layout's V8 dependency)
    /// failed the link with `Undefined symbols: _perry_fn_..._render`
    /// even though re-export-rename resolution (#785) was correct — the
    /// origin module had been demoted to V8 so it never emitted the
    /// `perry_fn_<src>__<name>` symbol at all.
    pub import_function_v8_specifiers: std::collections::HashMap<String, String>,
    /// Issue #841: named imports + namespace imports from Node.js
    /// submodules that Perry knows about at the resolver level but has
    /// no perry-stdlib / compile-source backing for —
    /// `node:timers/promises`, `node:readline/promises`,
    /// `node:stream/promises`, `node:stream/consumers`, `node:sys`.
    ///
    /// Keyed by `local_name` → `(submodule_key, exported_name)`. Codegen's
    /// `Expr::ExternFuncRef` value-form path probes this BEFORE falling
    /// to the `TAG_TRUE` sentinel and, when hit, emits a call to the
    /// runtime helper `js_node_submodule_export_as_function(submod, name)`
    /// which returns a NaN-boxed function singleton. The previous default
    /// (`TAG_TRUE` → `typeof X === "boolean"`) is what #841 set out to
    /// fix.
    pub import_function_node_submodule: std::collections::HashMap<String, (String, String)>,
    /// Issue #841 companion: per-local-namespace registration for
    /// `import * as ns from "node:<submodule>"` shapes. Keyed by
    /// `local_namespace_name` → `submodule_key`. Codegen's namespace
    /// lowering paths consult this so:
    ///   - `typeof ns` reports `"object"` via the runtime stub
    ///   - `ns.X` returns the same function singleton that named-import
    ///     `import { X }` produces (so `typeof ns.X === "function"` /
    ///     `ns.X === <named>` both hold)
    pub namespace_node_submodules: std::collections::HashMap<String, String>,
    /// Issue #678 followup (namespace branch): when a `import * as ns from
    /// "<v8-module>"` lands in a `ModuleKind::Interpreted` source with no
    /// accompanying named imports, no member appears in
    /// `import_function_prefixes` / `import_function_v8_specifiers` (the
    /// V8 module has no statically known export list). Keyed by
    /// `namespace_local_name` → `module_specifier`; consulted by the
    /// StaticMethodCall and namespace-member-call lowering paths so
    /// `ns.member(args)` routes through `js_call_v8_export(specifier,
    /// member, args, argc)` instead of falling through to the
    /// `double_literal(0.0)` stub. Affects ramda (`import * as R`),
    /// date-fns, jose, effect — packages where consumers use a wildcard
    /// namespace import for ergonomics. Sparse map; absent entries mean
    /// the namespace resolves natively (NativeCompiled or NativeRust).
    pub namespace_v8_specifiers: std::collections::HashMap<String, String>,
    /// Issue #680: per-namespace member resolution. Keyed by
    /// `(namespace_local_name, member_name)` → `source_prefix`. Used by
    /// the namespace-member access lowering paths in `expr.rs` and
    /// `lower_call.rs` to disambiguate when multiple `import * as X / Y`
    /// sources export the same member name. Pre-fix
    /// `import_function_prefixes` was a flat name→prefix map and the LAST
    /// namespace import to register a name won, so `import * as random;
    /// import * as tracer` (both export `make`) made `random.make` resolve
    /// to `tracer.make` — Effect's `defaultServices.ts` SIGSEGV'd because
    /// it dispatched `tracer.make(Math.random())` instead of
    /// `random.make(Math.random())`.
    pub namespace_member_prefixes: std::collections::HashMap<(String, String), String>,
    /// When true, `compile_module` returns the textual LLVM IR (`.ll`)
    /// as bytes instead of invoking `clang -c` to produce an object file.
    /// Used by the bitcode-link path (`PERRY_LLVM_BITCODE_LINK=1`).
    pub emit_ir_only: bool,

    // ── Cross-module import plumbing ──
    /// Locals that are namespace imports (`import * as X from "./mod"`).
    /// Codegen uses this to know that `X.foo()` should be dispatched as
    /// a cross-module call rather than an object method call.
    pub namespace_imports: Vec<String>,
    /// Issue #321: subset of `namespace_imports` populated by the
    /// "named import resolves to a `export * as Foo from "./Foo"`" branch
    /// in `compile.rs`. When the user wrote `import { Effect } from
    /// "effect"` and effect's index.ts has `export * as Effect from
    /// "./Effect.js"`, Effect lands in `namespace_imports` (so member
    /// dispatch works) AND in this set (so the StaticMethodCall codegen
    /// arm knows it's safe to route var-shape members through
    /// `js_closure_callN`). Plain `import * as Effect from "./Effect"`
    /// (used heavily by effect's INTERNAL modules) populates only
    /// `namespace_imports`, NOT this set — the pre-existing direct-call
    /// path preserves their long-standing silently-wrong-but-doesn't-throw
    /// behavior on var-shape static calls (the right fix there is a
    /// broader audit; doing it together with the named-import fix
    /// surfaces init-order issues hiding behind the silent-wrong shape).
    pub namespace_reexport_named_imports: std::collections::HashSet<String>,
    /// Imported class definitions from other native modules, keyed by
    /// the local alias (or original name when no alias). Each entry
    /// carries the class HIR, the module prefix of its origin, and an
    /// optional local alias.
    pub imported_classes: Vec<ImportedClass>,
    /// Imported enum member lists, keyed by the local name under which
    /// the enum is visible in this module.
    pub imported_enums: Vec<(String, Vec<(String, perry_hir::EnumValue)>)>,
    /// Names of imported functions that are async. Codegen needs this to
    /// wrap calls in the promise machinery.
    pub imported_async_funcs: std::collections::HashSet<String>,
    /// Type alias map (name → Type) aggregated from all modules. Codegen
    /// uses this to resolve `Named` types in function signatures.
    pub type_aliases: std::collections::HashMap<String, perry_types::Type>,
    /// Imported function parameter counts, keyed by function name.
    pub imported_func_param_counts: std::collections::HashMap<String, usize>,
    /// Issue #608 — imported function names whose source-side signature
    /// has a trailing `...rest` parameter. Used by the cross-module call
    /// site in `lower_call.rs` to pack trailing args into a `js_array_alloc`
    /// rest array before the call so the callee's rest binding is a real
    /// array, not the raw arg in disguise. Sparse set (only `true` entries).
    pub imported_func_has_rest: std::collections::HashSet<String>,
    /// Imported function return types, keyed by local function name.
    pub imported_func_return_types: std::collections::HashMap<String, perry_types::Type>,
    /// Names of imports that are exported VARIABLES (not functions). When an
    /// `ExternFuncRef` with one of these names appears as a value (not as a
    /// Call callee), the codegen calls the getter function to fetch the value
    /// instead of wrapping it as a closure reference. Without this, `import
    /// { HONE_VERSION } from './version'` followed by `let v = HONE_VERSION`
    /// would create a closure wrapper around the getter, not the actual string.
    pub imported_vars: std::collections::HashSet<String>,

    // ── Feature plumbing ──
    //
    // These fields control which runtime libraries and FFI surfaces are
    // compiled into the resulting binary. They propagate the CLI's feature
    // detection into the codegen so auto-optimize and linker steps work.
    //
    // NOTE: most of these are informational for the CLI driver's auto-
    // optimize rebuild + linker step — `compile_module` itself only
    // consults `output_type` (to decide between `main` and a dylib init)
    // and `i18n_table` (to materialize the table as rodata). The rest
    // are round-tripped through the CompileOptions so the CLI can hand
    // them to `build_optimized_libs` / linker flag construction without
    // threading separate parameters.
    /// Output type. "executable" emits a `main`, "dylib" emits a shared
    /// library plugin with no entrypoint.
    pub output_type: String,
    /// Whether the project needs `libperry_stdlib.a` linked in.
    pub needs_stdlib: bool,
    /// Whether the project needs `libperry_ui_*.a` linked in.
    pub needs_ui: bool,
    /// Whether the project needs the Geisterhand inspector linked in.
    pub needs_geisterhand: bool,
    /// Port the Geisterhand inspector listens on when `needs_geisterhand`.
    pub geisterhand_port: u16,
    /// Whether the project needs the QuickJS fallback runtime linked in.
    pub needs_js_runtime: bool,
    /// Cargo feature names enabled for this build, computed by the CLI's
    /// `compute_required_features`. Used by the auto-optimize path to
    /// decide which optional runtime helpers to compile into
    /// `libperry_stdlib.a`.
    pub enabled_features: Vec<String>,
    /// For the entry module: names of every non-entry native module
    /// that needs its `<prefix>__init` called before the entry's own
    /// init. Already covered by `non_entry_module_prefixes` for the
    /// init sequence, but tracked separately for auto-optimize's
    /// feature scan.
    pub native_module_init_names: Vec<String>,
    /// JavaScript-only modules routed through QuickJS (full specifiers).
    pub js_module_specifiers: Vec<String>,
    /// Bundled TypeScript extensions — `(absolute_path, module_prefix)`.
    pub bundled_extensions: Vec<(String, String)>,
    /// Native library FFI from `package.json` — `(library_name,
    /// function_names, header_path)` tuples.
    pub native_library_functions: Vec<(String, Vec<String>, String)>,
    /// i18n translation table snapshot — `(translations, key_count,
    /// locale_count, locale_codes, default_locale_idx)`. The
    /// `default_locale_idx` is the row index used at compile time to
    /// resolve `Expr::I18nString` to the right translation — without
    /// it, the lowering would have to either pick locale 0 blindly or
    /// fall back to the verbatim key.
    /// Tier 4.6 (v0.5.336): wrapped in `Arc` so the per-module clone
    /// in the `compile_module` rayon worker is a cheap reference bump
    /// instead of duplicating the (potentially large) `Vec<String>` of
    /// every translated string. The tuple shape is unchanged for the
    /// downstream destructure at `compile_module` line 597.
    pub i18n_table: Option<std::sync::Arc<(Vec<String>, usize, usize, Vec<String>, usize)>>,

    /// When true, emit LLVM `reassoc contract` per-instruction fast-math
    /// flags on every f64 op. Off by default — Perry produces bit-exact
    /// output with Node's f64 arithmetic. On (via `--fast-math`,
    /// `PERRY_FAST_MATH=1`, or `perry.fastMath: true` in package.json),
    /// the optimizer is permitted to reassociate FP chains and fuse
    /// multiply-adds, producing observable 1-ULP differences from Node
    /// in ~30% of randomly-generated FP programs in exchange for a ~7x
    /// speedup on tight `sum += constant` loops (and ~0% on most other
    /// FP-heavy code, per benchmarks). See `docs/src/cli/fast-math.md`.
    /// Drives `crate::block::FAST_MATH`; included in the object cache
    /// key so toggling it invalidates cached `.o` bytes.
    pub fast_math: bool,
    /// App metadata backing `perry/system` compile-time introspection APIs.
    pub app_metadata: AppMetadata,

    /// Issue #100: when non-empty, this module is the target of at least
    /// one `await import("...")` site somewhere in the program. Codegen
    /// emits a `@__perry_ns_<prefix>` static global initialized to
    /// undefined, populates it from this list at the end of
    /// `__perry_init_<prefix>` (or `main` for the entry module), and
    /// registers its address as a GC root. The dispatch site in
    /// `Expr::DynamicImport` reads this global and wraps it in
    /// `js_promise_resolved`. Empty means no namespace global is emitted.
    pub namespace_entries: Vec<NamespaceEntry>,

    /// Issue #100: for each `Expr::DynamicImport` site in this module,
    /// maps the path-argument string (as it appears in
    /// `Expr::DynamicImport::paths`) to the sanitized prefix of the
    /// target module. Codegen uses this to load
    /// `@__perry_ns_<target_prefix>` for the resolved-promise return
    /// value (single-path) or to chain string-compare dispatches
    /// (multi-path). Empty if this module performs no dynamic imports.
    pub dynamic_import_path_to_prefix: std::collections::HashMap<String, String>,

    /// Issue #753: sanitized prefixes of modules whose init must NOT
    /// run as part of the entry module's eager init chain. Reachable
    /// from the entry only through dynamic `import()` edges, so their
    /// `<prefix>__init` fires lazily from the dispatch site. The entry
    /// module's `main` filters this set out of `non_entry_module_prefixes`
    /// when emitting the eager init call sequence. Empty when no module
    /// in the program is deferred.
    pub deferred_module_prefixes: std::collections::HashSet<String>,

    /// Issue #753: sanitized prefixes of THIS module's static-import +
    /// re-export source modules (non-entry only — the entry has no
    /// `__init` to call). The wrapper `<prefix>__init` calls each
    /// dep's `<dep>__init` (idempotently) before invoking the body.
    /// Required so that a Deferred module firing lazily transitively
    /// initializes any Deferred deps reached only through its own
    /// re-export chain — otherwise the namespace populator at the
    /// tail of `<prefix>__init_body` reads zero-initialized cross-
    /// module globals. For Eager modules the redundant calls
    /// short-circuit on the guard's first-write check.
    pub module_init_deps: Vec<String>,

    /// Issue #842: true iff this module is the target of at least one
    /// `await import("./this_module.ts")` site anywhere in the program.
    /// When `namespace_entries` is empty (side-effect-only target — no
    /// `export` statements), this flag is the only signal that the
    /// producer-side `@__perry_ns_<prefix>` global + populator must
    /// still be emitted, because the consumer-side `Expr::DynamicImport`
    /// dispatch declares it as an extern global unconditionally.
    /// Without this, side-effect-only dynamic-import targets fail at
    /// link with `Undefined symbols: ___perry_ns_<prefix>`.
    pub is_dynamic_import_target: bool,
}

/// Issue #100: one entry in a module's namespace-population list.
/// Codegen iterates this in `__perry_init_<prefix>` to build the
/// `__perry_ns_<prefix>` global. Each variant captures everything
/// needed to emit the value-fetch IR for that key without re-walking
/// the HIR — the driver resolves Var/Function/Class kind and the
/// source-module prefix when it builds the list.
#[derive(Debug, Clone)]
pub struct NamespaceEntry {
    /// The key as seen by the consumer of `await import("...")`.
    pub name: String,
    /// How to fetch the value at populate time.
    pub kind: NamespaceEntryKind,
}

/// Issue #100: how to materialise the value for a namespace entry.
#[derive(Debug, Clone)]
pub enum NamespaceEntryKind {
    /// Local module-level variable. Codegen loads the value directly
    /// from the `@perry_global_<prefix>__<id>` global identified by
    /// `global_name`.
    LocalVar { global_name: String },
    /// Local user function exported as a value. Codegen calls
    /// `js_closure_alloc_singleton(@__perry_wrap_<scoped>)`.
    LocalFunction { wrap_symbol: String },
    /// Local class exported as a value. Codegen emits the
    /// INT32-tagged class-id NaN-box that matches `Expr::ClassRef`.
    LocalClass { class_id: u32 },
    /// Re-exported variable from another module. Codegen calls
    /// `perry_fn_<source_prefix>__<source_local>()` as a 0-arg getter
    /// returning the f64 value.
    ForeignVar {
        source_prefix: String,
        source_local: String,
    },
    /// Re-exported function from another module. Codegen declares the
    /// target's `perry_fn_*` as extern, emits a per-callsite
    /// `__perry_wrap_extern_*` thin wrapper (if not already emitted by
    /// the import-wrapper pass), and calls
    /// `js_closure_alloc_singleton` against that wrapper.
    ForeignFunction {
        source_prefix: String,
        source_local: String,
        param_count: usize,
    },
    /// `export * as Name from "./sub"` — namespace re-export. The
    /// nested value IS the target module's `@__perry_ns_<source_prefix>`
    /// global, populated by the target's own `__init`.
    NestedNamespace { source_prefix: String },
}

/// A class imported from another native module.
#[derive(Debug, Clone)]
pub struct ImportedClass {
    /// The class name as exported from its origin module.
    pub name: String,
    /// Optional local alias (`import { Foo as Bar }`).
    pub local_alias: Option<String>,
    /// Symbol prefix of the origin module (for cross-module method calls).
    pub source_prefix: String,
    /// Number of constructor parameters (needed for dispatch).
    pub constructor_param_count: usize,
    /// Method names defined on this class.
    pub method_names: Vec<String>,
    /// Per-method explicit param counts, parallel to `method_names`. Issue #235:
    /// codegen uses this to declare cross-module method symbols with the
    /// correct arity (was hardcoded "6 as safe upper bound", which made the
    /// callee read garbage from uninitialized arg-register slots when the
    /// call site passed fewer args than the declaration claimed) AND to pad
    /// dispatch-tower call sites with TAG_UNDEFINED so default-param
    /// desugaring fires correctly. Empty Vec is the legacy fallback for
    /// source modules that haven't been updated to populate it — codegen
    /// falls back to the old upper bound when the entry is missing.
    pub method_param_counts: Vec<usize>,
    /// Issue #672: parallel to `method_names`. `true` means the method's
    /// last declared parameter is `...rest`. Without this, cross-module call
    /// sites to `c.cmd('a', 'b', 'c')` on `class C { cmd(name, ...args) }`
    /// would not pack the trailing `'b', 'c'` into a rest array — only the
    /// home module's `method_has_rest` was populated. Symmetric to #484's
    /// fix for the freestanding-function path. Empty Vec means "fall through
    /// to the old behavior (no rest)".
    pub method_has_rest: Vec<bool>,
    /// Static field names defined on this class. Used to declare the foreign
    /// `@perry_static_<src>__<class>__<field>` global with external linkage
    /// so cross-module `[Parent.Symbol.X] = …` reads/writes resolve to the
    /// source module's defining global. Without this, `StaticFieldGet`
    /// silently produces `0.0` for any imported class. Refs #420.
    pub static_field_names: Vec<String>,
    /// Static method names defined on this class. Without this, calls like
    /// `MyClass.staticMethod(...)` on an imported class are treated as a
    /// missing method and fall through to `0.0` — turning every
    /// `await Foo.connect(...)` into a no-op that resolves with the number 0.
    pub static_method_names: Vec<String>,
    /// Getter property names. Without these, cross-module `obj.prop` for a
    /// getter property silently falls through to `undefined` because the
    /// dispatch site at `expr.rs::PropertyGet` looks up `(class, "__get_prop")`
    /// in `method_names`, which previously had no cross-module entry.
    pub getter_names: Vec<String>,
    /// Setter property names. Symmetric to `getter_names` for `obj.prop = v`.
    pub setter_names: Vec<String>,
    /// Parent class name, if any.
    pub parent_name: Option<String>,
    /// Field names in declaration order (for allocation sizing and field index mapping).
    pub field_names: Vec<String>,
    /// Field types in the same order as `field_names`. Required for
    /// `receiver_class_name` to walk through chained `obj.a.b.c` accesses
    /// where `a` and `b` are fields whose declared type is itself an
    /// imported class. Without this, every field access on an imported
    /// class returns `Type::Any` and the dispatch chain breaks at the
    /// first hop. Empty (or filled with `Type::Any`) is the legacy fallback
    /// when the source side hasn't been updated to populate it yet.
    pub field_types: Vec<perry_types::Type>,
    /// Class id assigned by the source module. When present, the importing
    /// module reuses this id in its `class_ids` map so that `instanceof`
    /// on an imported class compares against the same id stamped onto
    /// instances by the source module's constructor. `None` falls back
    /// to a freshly-assigned id (legacy behavior).
    pub source_class_id: Option<u32>,
}

/// Cross-module import context, bundled into a single struct to avoid
/// adding five more individual parameters to every compile_* function.
/// Built once in `compile_module` from `CompileOptions`.
pub(crate) struct CrossModuleCtx {
    pub namespace_imports: std::collections::HashSet<String>,
    /// Issue #321: see `CompileOptions::namespace_reexport_named_imports`.
    pub namespace_reexport_named_imports: std::collections::HashSet<String>,
    /// Issue #680: per-namespace member resolution. See doc on
    /// `CompileOptions::namespace_member_prefixes`.
    pub namespace_member_prefixes: std::collections::HashMap<(String, String), String>,
    pub imported_async_funcs: std::collections::HashSet<String>,
    /// FuncIds of locally-defined async functions in this module. Populated
    /// from `hir.functions.is_async`. Used by `is_promise_expr` to refine
    /// `let p = asyncFn();` to `Promise(_)` so subsequent `p.then(cb)`
    /// chains route through `js_promise_then`.
    pub local_async_funcs: std::collections::HashSet<u32>,
    pub type_aliases: std::collections::HashMap<String, perry_types::Type>,
    pub imported_func_param_counts: std::collections::HashMap<String, usize>,
    /// Issue #678: see `CompileOptions::import_function_origin_names`.
    /// Cloned from the same field so codegen helpers reachable via
    /// `CrossModuleCtx` can resolve the origin name without an extra arg.
    pub import_function_origin_names: std::collections::HashMap<String, String>,
    /// Issue #678 followup: see `CompileOptions::import_function_v8_specifiers`.
    /// Routes V8-fallback imports through the runtime bridge instead of
    /// the missing `perry_fn_<src>__<name>` extern.
    pub import_function_v8_specifiers: std::collections::HashMap<String, String>,
    /// Issue #841: see `CompileOptions::import_function_node_submodule`.
    /// Routes named imports from the five recognized Node submodules
    /// (`timers/promises`, `readline/promises`, `stream/promises`,
    /// `stream/consumers`, `sys`) through
    /// `js_node_submodule_export_as_function` instead of falling to
    /// the `TAG_TRUE` sentinel.
    pub import_function_node_submodule: std::collections::HashMap<String, (String, String)>,
    /// Issue #841 companion: see `CompileOptions::namespace_node_submodules`.
    /// Routes namespace-import bindings from the five recognized Node
    /// submodules to a runtime stub object whose properties point at the
    /// same function singletons named imports produce.
    pub namespace_node_submodules: std::collections::HashMap<String, String>,
    /// See `CompileOptions::namespace_v8_specifiers`. Routes
    /// `import * as ns from "<v8-module>"; ns.member(args)` through the
    /// V8 bridge when the source has no statically known export list and
    /// no companion named import seeded `import_function_prefixes`.
    pub namespace_v8_specifiers: std::collections::HashMap<String, String>,
    /// Issue #608 — imported function names whose source-side signature
    /// has a trailing `...rest` parameter. Used by the cross-module call
    /// site in `lower_call.rs` to pack trailing args into a rest array.
    pub imported_func_has_rest: std::collections::HashSet<String>,
    pub imported_func_return_types: std::collections::HashMap<String, perry_types::Type>,
    /// Refs #915 (gap 3 / #321 follow-up): function ids in THIS module
    /// whose body unconditionally returns a `ClassRef` (or transitively
    /// returns another such factory). Maps function id → produced
    /// class name. Lets `lower_call`'s static-method dispatch tower
    /// recognise `Literal(...).pipe(...)` (where `Literal` is a
    /// factory) and route the `.pipe` lookup through the produced
    /// class's static methods. Built once in `compile_module` via a
    /// fixed-point pass over `hir.functions`.
    pub func_returns_class: std::collections::HashMap<u32, String>,
    /// Per-method explicit param counts, keyed by `(class_name, method_name)`.
    /// Built once in `compile_module` from BOTH local `hir.classes` AND
    /// `opts.imported_classes`. Used at every method-call dispatch site in
    /// `lower_call.rs` to pad missing trailing args with TAG_UNDEFINED so
    /// the callee's default-param desugaring fires correctly.
    /// Pre-fix the dispatch tower passed only the user-provided args + recv
    /// to a function declared with N+1 doubles, leaving any param the caller
    /// skipped to be read from an uninitialized arg-register slot. On
    /// AArch64 / Win64 those slots typically held a real heap pointer left
    /// over from a prior call's return state — dereferencing `options.session`
    /// inside the dispatch chain silently hung. See issue #235.
    pub method_param_counts: std::collections::HashMap<(String, String), usize>,
    /// Per-`(class, method)` rest-parameter flag. Set when the method's
    /// final declared param is `...rest` — drives the call-site
    /// rest-bundling in `lower_call.rs`'s static / dynamic dispatch
    /// arms. Closes #484. Sparse map (only `true` entries stored).
    pub method_has_rest: std::collections::HashMap<(String, String), bool>,
    /// Per-class `keys_array` global variable names. Each entry maps
    /// `class_name → @perry_class_keys_<modprefix>__<sanitized_class>`.
    /// Built once in `compile_module` (one entry per class — local
    /// definitions + imported stubs). `compile_new` looks up the
    /// class here and emits a direct global load + the inline-keys
    /// allocator. See `js_object_alloc_class_inline_keys` in
    /// `perry-runtime/src/object.rs`.
    pub class_keys_globals: std::collections::HashMap<String, String>,
    /// Imported class constructor function names. Maps class_name →
    /// full constructor symbol (e.g. "Editor" → "hone_editor_...__Editor_constructor").
    /// Populated from `opts.imported_classes`.
    pub imported_class_ctors: std::collections::HashMap<String, (String, usize)>,
    /// Compile-time i18n table for resolving `Expr::I18nString` against
    /// the project's default locale. `None` when i18n is not configured.
    /// Built from `opts.i18n_table` once at the top of `compile_module`
    /// and threaded through every `FnCtx` instantiation as a shared
    /// borrow via `cross_module.i18n`.
    pub i18n: Option<crate::expr::I18nLowerCtx>,
    /// Names of imports that are exported variables (not functions).
    pub imported_vars: std::collections::HashSet<String>,
    /// Whether perry-stdlib will be linked into the final binary. When
    /// false, compile_module_entry skips the `js_stdlib_init_dispatch()`
    /// call in main's prologue because only the runtime is linked and
    /// the stub symbol isn't pulled in (runtime is built with the
    /// `stdlib` feature on when perry-stdlib depends on it, which
    /// excludes the cfg-gated stub in `perry-runtime/src/stdlib_stubs.rs`).
    pub needs_stdlib: bool,
    /// Whether the project needs the Geisterhand inspector linked in.
    /// Threaded through from `CompileOptions::needs_geisterhand` so the
    /// entry-module init prelude can emit the `perry_geisterhand_start`
    /// call site (which also pins the geisterhand server module against
    /// `-dead_strip`, keeping `INSPECTOR_HTML` referenced).
    pub needs_geisterhand: bool,
    /// Port the Geisterhand inspector listens on when `needs_geisterhand`.
    pub geisterhand_port: u16,
    /// Whether the project pulls in `libperry_jsruntime.a` because some
    /// `.ts` module imports from a `.js` file the resolver classified as
    /// JS-runtime-loaded (issue #248). Threaded through so the entry-module
    /// init prelude can emit the `js_runtime_init()` call before any
    /// `js_load_module` / `js_call_function` site fires.
    pub needs_js_runtime: bool,
    /// Compile-time constant values for module globals. Maps LocalId → f64
    /// for variables like `__platform__` whose value is known at compile time.
    /// Used by `lower_if` to constant-fold platform checks and skip emitting
    /// dead branches (which may reference FFI functions that don't exist on
    /// the current target).
    pub compile_time_constants: std::collections::HashMap<u32, f64>,
    /// App metadata backing compile-time `perry/system` introspection APIs.
    pub app_metadata: AppMetadata,
    /// Functions with a 3-param clamp pattern: fid → true. Call sites
    /// emit `@llvm.smax.i32` + `@llvm.smin.i32` instead of a function call.
    pub clamp3_functions: std::collections::HashSet<u32>,
    /// Functions with clampU8 pattern (1 param, clamp to [0, 255]).
    pub clamp_u8_functions: std::collections::HashSet<u32>,
    /// Functions that always return integer (all returns end with `| 0` etc).
    pub returns_int_functions: std::collections::HashSet<u32>,
    /// (Issue #50) Module-level `const` 2D int arrays folded into flat
    /// `[N x i32]` LLVM constants. Maps local_id → info. Populated by
    /// scanning `hir.init`; threaded through every FnCtx so the IndexGet
    /// lowering can intercept `X[i][j]` / `krow[j]` patterns.
    pub flat_const_arrays: std::collections::HashMap<u32, crate::expr::FlatConstInfo>,
    /// FFI manifest signatures from `package.json`'s `nativeLibrary.functions`.
    /// Maps function name → (param_kinds, return_kind) where each kind is
    /// `"i64"`, `"f64"`, `"void"`, `"string"`, or `"ptr"`. Without this map,
    /// `lower_call` falls back to a heuristic that puts all numeric args/returns
    /// into d-registers (DOUBLE) — incorrect for handle-returning C functions
    /// like `hone_editor_create() -> *mut EditorView` whose actual ABI returns
    /// the pointer in `x0`, not `d0`. The manifest tells us when to use
    /// `i64`/`I64` so the LLVM declaration matches the platform C ABI.
    pub ffi_signatures: std::collections::HashMap<String, (Vec<String>, String)>,
    /// Per-module mapping: local class/binding name → import source spec.
    /// Built once in `compile_module` from `hir.imports`. Used by
    /// `lower_builtin_new` to disambiguate ambiguously-named built-in
    /// constructors. Without this, `import Client from "better-sqlite3"`
    /// (where `Client` is a default-import alias for the sqlite Database
    /// class) silently dispatches through the pg `Client` arm and emits
    /// an undefined `_js_pg_client_new` reference. With this map, the
    /// "Client" arm only fires when the local `Client` was imported from
    /// "pg" (named or default). See issue #602.
    pub imported_class_sources: std::collections::HashMap<String, String>,
    /// Issue #655: map from interface name → HIR Interface definition.
    /// Lets `static_type_of` resolve `obj.field` when `obj` is typed
    /// against a TS `interface` (not a `class`). The `class_table`
    /// only contains real classes, so without this lookup chained
    /// access like `m.get(k)!.field.shift()` fell through to generic
    /// property dispatch and Array methods returned garbage.
    pub interfaces: std::collections::HashMap<String, perry_hir::Interface>,
    /// Issue #100: namespace-entry list for this module's
    /// `@__perry_ns_<prefix>` populator. Empty unless this module is
    /// the target of at least one dynamic `import()` site in the
    /// program. Populated at the end of `__perry_init_<prefix>` (or
    /// `main` for the entry module).
    pub namespace_entries: Vec<NamespaceEntry>,
    /// Issue #100: map from each `Expr::DynamicImport` path-arg string
    /// to the sanitized prefix of the target module. Read by the
    /// dispatch site in `expr.rs::Expr::DynamicImport` to find the
    /// `@__perry_ns_<target_prefix>` global to load.
    pub dynamic_import_path_to_prefix: std::collections::HashMap<String, String>,
    /// Issue #753: sanitized prefixes of modules reached only through
    /// dynamic `import()` edges. Their `<prefix>__init` is excluded
    /// from the entry-main eager init call sequence and fires lazily
    /// from each `Expr::DynamicImport` dispatch site.
    pub deferred_module_prefixes: std::collections::HashSet<String>,
    /// Issue #753: this module's static-import + re-export source
    /// prefixes (non-entry only). Consumed by `compile_module_entry`
    /// when emitting the wrapper for `<prefix>__init` so dep init
    /// fires before the body — transitively pulls in any Deferred dep
    /// chain reached only through this module's re-exports.
    pub module_init_deps: Vec<String>,
    /// Issue #842: true iff this module is the target of at least one
    /// dynamic `import()` site in the program. Forces emission of
    /// `@__perry_ns_<prefix>` + populator even when `namespace_entries`
    /// is empty (side-effect-only modules with no `export`s).
    pub is_dynamic_import_target: bool,
}

/// Compile a Perry HIR module to an object file via LLVM IR.
///
/// CRITICAL (#686): `hir` MUST be `&HirModule` (shared reference), never
/// `&mut`. The caller computes `perry_hir::stable_hash::hash_module(hir)`
/// just before this call to derive the per-module object cache key. If
/// codegen ever mutated the HIR mid-compile, the cached `.o` would no
/// longer correspond to the hashed input and stale entries would be
/// served on subsequent builds. The `&` here is the load-bearing
/// guarantee — do not change to `&mut` without also moving the cache
/// hash to AFTER codegen.
pub fn compile_module(hir: &HirModule, opts: CompileOptions) -> Result<Vec<u8>> {
    // Set the per-instruction FMF emission mode for this build before
    // any LlBlock methods run. All modules in a single program build
    // share the same `fast_math` setting, so writing the AtomicBool
    // here (potentially redundantly when rayon parallelizes module
    // compiles) is safe — every store writes the same value.
    crate::block::FAST_MATH.store(opts.fast_math, std::sync::atomic::Ordering::Relaxed);

    let triple = opts.target.clone().unwrap_or_else(default_target_triple);

    let mut llmod = LlModule::new(&triple);
    // Null guard global: a zeroed i32 used as a safe dereference target
    // when a NaN-unboxed pointer is null/invalid. Prevents segfaults from
    // uninitialized locals or unhandled expressions producing 0.0/TAG_UNDEFINED.
    llmod.add_internal_global("perry_null_guard_zero", crate::types::I32, "0");
    runtime_decls::declare_phase1(&mut llmod);

    // Derive a per-module symbol prefix from the HIR module name:
    //
    //     self.module_symbol_prefix = hir.name.replace(|c: char|
    //         !c.is_alphanumeric() && c != '_', "_");
    //
    // Every emitted symbol that could collide across modules
    // (user functions, class methods, string pool globals, handle slots,
    // module-level globals) gets prefixed with this. The entry module's
    // `main` is the only globally-named symbol — non-entry modules emit
    // `<prefix>__init` instead.
    let module_prefix = sanitize(&hir.name);

    // Imports are no longer a hard error — Phase F.1 supports multi-
    // module compilation. Cross-module function CALLS via ExternFuncRef
    // still land in Phase F.2; for now they'll error at the use site
    // with a specific message.

    // Phase C.2: classes (and inheritance!) are supported. Perry's HIR
    // lowering aggressively pre-resolves both methods and super calls
    // into inline statements at the constructor/method body, so the
    // LLVM codegen mostly sees a flat object-allocation + field-set
    // pattern. We let everything through and let the expression-level
    // codegen error at any specific construct it doesn't know how to
    // handle.

    // Module-wide string literal pool. Owned by the codegen so that
    // `compile_function` and `compile_main` can take split borrows of
    // (&mut LlFunction, &mut StringPool) without confusing the borrow
    // checker — the pool lives outside LlModule. The module prefix
    // becomes part of every emitted global so multi-module programs
    // don't collide on `.str.0.handle`.
    let mut strings = StringPool::with_prefix(module_prefix.clone());

    // Class lookup table for `Expr::New`. Indexed by class name —
    // the HIR has unique names per module.
    let mut class_table: HashMap<String, &perry_hir::Class> =
        hir.classes.iter().map(|c| (c.name.clone(), c)).collect();
    // Refs #486: also register class-expression self-binding aliases so
    // `lookup_new("_X")` and other code paths that consult `class_table` by
    // name find the underlying class. See `class_ids` block below for the
    // companion id-map registration and the broader rationale.
    for c in &hir.classes {
        for alias in &c.aliases {
            class_table.entry(alias.clone()).or_insert(c);
        }
    }

    // Class id assignment: each user class gets an integer id
    // starting at 1 (0 is reserved for anonymous object literals).
    // Used by lower_new to tag the object header so virtual
    // dispatch and instanceof can read the actual class at runtime.
    //
    // We use the HIR `ClassId` (assigned by `LoweringContext::fresh_class`)
    // rather than a per-module enumerate index, because in multi-module
    // compilation the HIR counter is shared across modules (compile.rs
    // threads `next_class_id` through `lower_module_with_class_id_and_types`).
    // Importing modules look up imported classes via their HIR id (passed
    // as `ImportedClass.source_class_id`); using the HIR id here too means
    // the source module stamps the same id on `new C()` instances that
    // importing modules check against in `e instanceof C`.
    let mut class_ids: HashMap<String, u32> =
        hir.classes.iter().map(|c| (c.name.clone(), c.id)).collect();
    // Refs #486: register class-expression self-binding aliases (e.g. the
    // `_X` in `var X = class _X { ... }`) so `new _X()` from inside the class
    // body resolves to the same class id as `new X()` would. Without this,
    // lower_new("_X") falls into the placeholder path and stamps class_id=0
    // on the new instance, breaking method dispatch.
    for c in &hir.classes {
        for alias in &c.aliases {
            class_ids.entry(alias.clone()).or_insert(c.id);
        }
    }

    // Enum lookup table for `Expr::EnumMember`. Each (enum_name,
    // member_name) maps to its EnumValue, which the codegen lowers
    // to either a numeric or string constant. Built once here.
    let mut enum_table: HashMap<(String, String), perry_hir::EnumValue> = hir
        .enums
        .iter()
        .flat_map(|e| {
            e.members
                .iter()
                .map(move |m| ((e.name.clone(), m.name.clone()), m.value.clone()))
        })
        .collect();

    // ── Phase F: merge imported cross-module definitions ──────────
    //
    // Imported enums: add their members to the enum_table so
    // `Expr::EnumMember` can resolve them in this module.
    for (enum_name, members) in &opts.imported_enums {
        for (member_name, value) in members {
            enum_table
                .entry((enum_name.clone(), member_name.clone()))
                .or_insert_with(|| value.clone());
        }
    }

    // Imported classes: build lightweight stub `Class` objects so the
    // codegen dispatch tables (class_table, method_names, class_ids)
    // can resolve cross-module class method calls. The actual method
    // bodies live in the other module's .o — here we only need the
    // metadata for dispatch and the extern LLVM declarations for the
    // linker.
    let mut imported_class_stubs: Vec<perry_hir::Class> = Vec::new();
    // Fallback id range for imported classes whose source_class_id is None
    // (legacy callers that didn't populate it). Start above the max local
    // HIR id so we don't collide with local class ids.
    let next_class_id = hir.classes.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    for (idx, ic) in opts.imported_classes.iter().enumerate() {
        // Prefer the source module's class id so `instanceof` on an
        // imported class matches the id stamped onto real instances
        // by the source module's constructor. Fall back to a freshly
        // assigned id when the caller didn't pass one.
        let class_id = ic
            .source_class_id
            .unwrap_or_else(|| next_class_id + (idx as u32));
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);

        // Skip if already defined locally (local definition takes precedence).
        if class_table.contains_key(effective_name) {
            continue;
        }

        // Assign a class id for dispatch / instanceof.
        //
        // Refs #665: `or_insert` (first-writer-wins) instead of `insert`
        // (last-writer-wins). When two different classes are both
        // default-imported in the same file, both register under
        // `effective_name = "default"`. `class_table.entry().or_insert()`
        // below already keeps the first stub for that key; the side maps
        // must agree, otherwise the method registry builds symbols mixing
        // the FIRST writer's methods with the LAST writer's prefix +
        // canonical name, producing fnames the linker can't resolve.
        class_ids
            .entry(effective_name.to_string())
            .or_insert(class_id);
        // Also register the canonical name if aliased.
        if ic.local_alias.is_some() && !class_ids.contains_key(&ic.name) {
            class_ids.insert(ic.name.clone(), class_id);
        }

        // Build a stub Class with the minimum fields the codegen needs.
        // Most fields are empty — only name, extends_name, and methods
        // are consulted by dispatch.
        let stub = perry_hir::Class {
            id: 0, // imported — no local ClassId
            name: effective_name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: ic.parent_name.clone(),
            native_extends: None,
            extends_expr: None,
            fields: ic
                .field_names
                .iter()
                .enumerate()
                .map(|(i, name)| perry_hir::ClassField {
                    name: name.clone(),
                    key_expr: None,
                    // Use the real declared type when the source-side
                    // populated `field_types`; fall back to `Any` otherwise.
                    // Real types let `receiver_class_name`'s `PropertyGet`
                    // recursion identify chained imported-class field
                    // dispatch (e.g. `vm.viewport.scroll.scrollTop`).
                    ty: ic
                        .field_types
                        .get(i)
                        .cloned()
                        .unwrap_or(perry_types::Type::Any),
                    init: None,
                    is_private: false,
                    is_readonly: false,
                    decorators: Vec::new(),
                })
                .collect(),
            constructor: None,
            methods: ic
                .method_names
                .iter()
                .map(|m| perry_hir::Function {
                    id: 0,
                    name: m.clone(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: perry_types::Type::Any,
                    body: Vec::new(),
                    is_async: false,
                    is_generator: false,
                    was_plain_async: false,
                    was_unrolled: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                })
                .collect(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
        };
        imported_class_stubs.push(stub);
    }
    // Issue #309: break inheritance-chain cycles in imported_class_stubs.
    // Effect (and other heavily-modular TypeScript packages) declare
    // same-named classes across modules (e.g. multiple `class Base extends X`
    // inside IIFEs in Data.ts, plus `class Class extends Base` in
    // Effectable.ts). When pulled into a single importing module's
    // class_table by name, the chains can form a cycle:
    //     local Base → extends "Class" (imported stub from Effectable)
    //     imported Class → parent_name "Base" (resolves back to local Base)
    //     → cycle.
    // Every chain-walking site in codegen assumed acyclic inheritance, so
    // a single such cycle causes either an OOM (Vec-accumulating walks like
    // `apply_field_initializers_recursive`) or a CPU-hang (counter walks
    // like `class_field_global_index`). We break the cycle once at this
    // central point by detecting it via DFS over the (local ∪ stub) union
    // and dropping `extends_name` on the FIRST imported stub that closes
    // the cycle. All downstream chain walks then operate on a guaranteed-
    // acyclic graph. The fundamental name-collision problem (Data.ts's
    // local "Base" being a different class than Effectable.ts's "Base"
    // even though they share a name) is left unfixed — that requires
    // module-prefixing class names in HIR and is a separate refactor; the
    // cycle break here is purely defensive.
    {
        let local_class_names: std::collections::HashSet<&str> =
            hir.classes.iter().map(|c| c.name.as_str()).collect();
        let mut stub_idx_by_name: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (idx, stub) in imported_class_stubs.iter().enumerate() {
            stub_idx_by_name.entry(stub.name.clone()).or_insert(idx);
        }
        // For each stub, walk the chain in the union name space. If the
        // walk revisits a name OR exceeds a sane depth cap, drop this
        // stub's parent so the cycle dies here.
        let mut to_drop: Vec<usize> = Vec::new();
        for (idx, stub) in imported_class_stubs.iter().enumerate() {
            let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
            visited.insert(stub.name.clone());
            let mut cur = stub.extends_name.clone();
            let mut depth: usize = 0;
            let mut cycle = false;
            while let Some(name) = cur {
                depth += 1;
                if depth > 64 {
                    cycle = true;
                    break;
                }
                if !visited.insert(name.clone()) {
                    cycle = true;
                    break;
                }
                // Parent resolution: prefer LOCAL class over imported stub
                // (matches `class_table.entry().or_insert()` semantics
                // below).
                cur = if local_class_names.contains(name.as_str()) {
                    hir.classes
                        .iter()
                        .find(|c| c.name == name)
                        .and_then(|c| c.extends_name.clone())
                } else if let Some(&pidx) = stub_idx_by_name.get(&name) {
                    imported_class_stubs[pidx].extends_name.clone()
                } else {
                    None
                };
            }
            if cycle {
                to_drop.push(idx);
            }
        }
        for idx in to_drop {
            imported_class_stubs[idx].extends_name = None;
        }
    }

    // Add imported class stubs to the class_table (references into the
    // Vec we just built — the Vec lives for the remainder of compile_module).
    // Also build a map from class name → source module prefix so method
    // dispatch generates the correct cross-module symbol name.
    //
    // Skip imports that collide by name with a LOCAL class (#431). The
    // local class shadows the import in `class_table` (the
    // `class_table.entry().or_insert()` loop below preserves the local
    // entry), so this map must not point a local-class lookup at an
    // import's source prefix — doing so makes `compile_method` mangle
    // the LOCAL methods under the IMPORTED module's prefix while the
    // dispatch-table builder (line ~3614) still references them under
    // the local prefix, leaving `@perry_method_<local>__<C>__<m>`
    // undefined at link time. This is the cross-module sibling of
    // #336's intra-module collision; #336 disambiguated the
    // `@perry_class_keys_*` global, but the method-body prefix needs
    // the same fix for cross-module name reuse (Effect's `Class` /
    // `Refinement` / `Composite` / `ParseError` /
    // `PropertySignatureTransformation` / `DroppingStrategy` cases).
    let mut imported_class_prefix: HashMap<String, String> = HashMap::new();
    // Issue #568: when `import { Widget as PublicWidget }` (or the
    // re-export shape `export { Widget as PublicWidget }` followed by
    // `import { PublicWidget }`) renames a cross-module class, the stub
    // pushed into `class_table` carries `name = effective_name` (the
    // alias). Method-symbol mangling needs the SOURCE-side name (the
    // canonical `ic.name`) so the LLVM call resolves to the symbol the
    // source module's `.o` actually exports. This side map lets the
    // method-registry loop below recover the source name.
    let mut imported_class_source_name: HashMap<String, String> = HashMap::new();
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        if hir.classes.iter().any(|c| c.name == *effective_name) {
            continue;
        }
        // Refs #665: first-writer-wins to match `class_table`'s
        // `.or_insert()` semantics (see the class-id loop above). When two
        // different classes are both default-imported, both register under
        // `effective_name = "default"`; using `.insert()` would let the
        // LAST writer's source_prefix / canonical name win, while
        // `class_table["default"]` keeps the FIRST writer's stub. The
        // method-registry builder reads both, and the mismatch produces
        // method symbols mangled under the wrong class — the linker can't
        // resolve them and the build fails with "undefined value".
        imported_class_prefix
            .entry(effective_name.to_string())
            .or_insert_with(|| ic.source_prefix.clone());
        if effective_name != ic.name {
            imported_class_source_name
                .entry(effective_name.to_string())
                .or_insert_with(|| ic.name.clone());
        }
    }
    for stub in &imported_class_stubs {
        class_table.entry(stub.name.clone()).or_insert(stub);
    }

    // Local async function FuncIds — populated below from `hir.functions`
    // (the per-function loop further down). Built here so the CrossModuleCtx
    // construction is complete before the FnCtx instances reference it.
    let mut local_async_funcs: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        // Include both truly-async functions and those transformed from
        // async to generator (was_plain_async=true, is_async=false after
        // the v0.5.371 async-to-generator pass) — both return Promises
        // so is_promise_expr must recognize their call sites.
        if f.is_async || f.was_plain_async {
            local_async_funcs.insert(f.id);
        }
    }

    // Per-class keys-array globals: each class gets a single internal
    // global `@perry_class_keys_<modprefix>__<class>` that holds the
    // shared keys_array pointer (built ONCE at module init via
    // js_build_class_keys_array). Every `new ClassName()` site then
    // emits a direct global load + inline allocator call, bypassing
    // the per-call SHAPE_CACHE lookup AND the runtime
    // js_object_alloc_class_with_keys function entirely on the hot
    // allocation path.
    //
    // Per-class init data: (global_name, packed_keys_string, total_field_count).
    // Used by emit_string_pool to emit the build-call sequence.
    let mut class_keys_init_data: Vec<(String, String, u32)> = Vec::new();
    let mut class_keys_globals_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for c in &hir.classes {
        let global_name = format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name),);
        llmod.add_internal_global(&global_name, I64, "0");

        // Build the packed-keys string. Format: each field name
        // followed by `\0`. Parent classes contribute their fields
        // first (walking from deepest ancestor down) so the slot
        // order matches `class_field_global_index`'s assumption.
        let mut packed_keys = String::new();
        // Skip computed-key fields (`[Symbol.for("k")] = …`): their key is an
        // expression evaluated at runtime, not a stable string, so they don't
        // get an inline slot. Including their synthetic `__computed_field_*`
        // names in the packed keys would surface them as enumerable own
        // properties via Object.keys() and inflate the inline-slot count.
        // Their values are stored via `apply_field_initializers_recursive`'s
        // IndexSet path → js_object_set_field / js_object_set_symbol_property.
        let count_keyable = |fields: &[perry_hir::ClassField]| -> u32 {
            fields.iter().filter(|f| f.key_expr.is_none()).count() as u32
        };
        let mut total_field_count = count_keyable(&c.fields);
        let mut parent_chain: Vec<String> = Vec::new();
        // Resolver that finds a parent's `(fields_vec, next_extends)` either
        // in the local HIR or, failing that, in the imported_class_stubs
        // built earlier in this fn (which carry `ic.field_names` as full
        // ClassField records). Issue #485: without falling back to imports,
        // a local subclass that extends an IMPORTED parent ends up with a
        // packed_keys / total_field_count that omits the parent's fields,
        // so instances get allocated with too-few inline slots and the
        // parent's cross-module ctor's `this.field = ...` writes overflow
        // the object header — making `f.field` read undefined on the
        // importing side even though the parent's ctor "ran".
        let lookup_class_chain_link =
            |name: &str| -> Option<(Vec<perry_hir::ClassField>, Option<String>)> {
                if let Some(parent) = hir.classes.iter().find(|cls| cls.name == name) {
                    return Some((parent.fields.clone(), parent.extends_name.clone()));
                }
                if let Some(stub) = imported_class_stubs.iter().find(|cls| cls.name == name) {
                    return Some((stub.fields.clone(), stub.extends_name.clone()));
                }
                None
            };
        let mut p = c.extends_name.clone();
        while let Some(parent_name) = p {
            if let Some((parent_fields, parent_extends)) = lookup_class_chain_link(&parent_name) {
                parent_chain.push(parent_name.clone());
                total_field_count += count_keyable(&parent_fields);
                p = parent_extends;
            } else {
                break;
            }
        }
        // Walk from deepest ancestor to direct parent.
        for parent_name in parent_chain.iter().rev() {
            if let Some((parent_fields, _)) = lookup_class_chain_link(parent_name) {
                for f in &parent_fields {
                    if f.key_expr.is_some() {
                        continue;
                    }
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
            }
        }
        for f in &c.fields {
            if f.key_expr.is_some() {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        // Refs #486: register self-binding aliases (`_X` from `var X = class _X`)
        // so the inline-alloc fast path at lower_call.rs:2532 finds the keys
        // global when the class is referenced by its inner name. Without this,
        // `new _X()` would fall into the slower `js_object_alloc_class_with_keys`
        // path that builds packed_keys at the call site — which works but is
        // unnecessarily slow.
        for alias in &c.aliases {
            class_keys_globals_map
                .entry(alias.clone())
                .or_insert_with(|| global_name.clone());
        }
        class_keys_init_data.push((global_name, packed_keys, total_field_count));
    }
    // Same naming convention for IMPORTED class stubs. Pack the field
    // names so the importing module allocates the right inline slot count
    // and the slot index for each field matches what the source module's
    // constructor wrote. Without this, the object is allocated 0 inline
    // slots and `this.field = v` in the cross-module constructor writes
    // past the object, while reads on the importing side return undefined.
    for c in imported_class_stubs.iter() {
        if hir.classes.iter().any(|local| local.name == c.name) {
            continue;
        }
        // Skip duplicate imported stubs of the same name. Two namespace
        // re-exports of the same class (e.g., `export * as A from "./mod"`
        // and `export * as B from "./mod"`) can register the same class
        // twice in `imported_class_stubs`. Without this guard, codegen
        // would emit `@perry_class_keys_<modprefix>__<name>` twice and
        // clang would reject the IR with "redefinition of global". See #336.
        if class_keys_globals_map.contains_key(&c.name) {
            continue;
        }
        let global_name = format!("perry_class_keys_{}__{}", module_prefix, sanitize(&c.name),);
        llmod.add_internal_global(&global_name, I64, "0");
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        let mut packed_keys = String::new();
        let mut total_field_count = c.fields.len() as u32;
        // Issue #485: imported subclass stubs also need their parent's
        // fields prepended to the packed-keys, so allocations on this
        // importing side reserve enough inline slots for parent +
        // child. Without this, `new Sub()` in the importing module
        // allocates 0 slots when Sub has no own fields and the
        // cross-module ctor's `this.parentField = v` writes past the
        // object header — exactly the same shape collapse the local-
        // class branch above guards against.
        let mut parent_chain: Vec<String> = Vec::new();
        let mut p = c.extends_name.clone();
        while let Some(parent_name) = p {
            if let Some(parent) = imported_class_stubs
                .iter()
                .find(|cls| cls.name == parent_name)
            {
                parent_chain.push(parent_name.clone());
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == parent_name) {
                parent_chain.push(parent_name.clone());
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
            } else {
                break;
            }
        }
        for parent_name in parent_chain.iter().rev() {
            if let Some(parent) = imported_class_stubs
                .iter()
                .find(|cls| cls.name == *parent_name)
            {
                for f in &parent.fields {
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
            } else if let Some(parent) = hir.classes.iter().find(|cls| cls.name == *parent_name) {
                for f in &parent.fields {
                    packed_keys.push_str(&f.name);
                    packed_keys.push('\0');
                }
            }
        }
        for f in &c.fields {
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        class_keys_init_data.push((global_name, packed_keys, total_field_count));
    }

    // Derive __platform__ number from target triple:
    //   0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux,
    //   5 = Web, 6 = tvOS, 7 = watchOS, 8 = visionOS, 9 = HarmonyOS
    let platform_number: f64 = {
        let t = triple.to_lowercase();
        // HarmonyOS check must precede the plain `linux` arm: the OHOS triple is
        // `*-unknown-linux-ohos`, so a naive `contains("linux")` would classify it as 4.
        if t.contains("ohos") {
            9.0
        } else if t.contains("visionos") || t.contains("xros") {
            8.0
        } else if t.contains("watchos") {
            7.0
        } else if t.contains("ios") {
            1.0
        } else if t.contains("tvos") {
            6.0
        } else if t.contains("android") {
            2.0
        } else if t.contains("windows") || t.contains("mingw") || t.contains("msvc") {
            3.0
        } else if t.contains("linux") {
            4.0
        } else if t.contains("wasm") || t.contains("emscripten") {
            5.0
        } else {
            0.0
        } // macOS / darwin default
    };
    // Pre-scan hir.init for compile-time constant variables. These are
    // `declare const __platform__: number` / `declare const __plugins__: number`
    // that other backends (JS, WASM) inject at build time. The LLVM backend
    // uses these to constant-fold platform checks in `lower_if`, eliminating
    // dead branches that reference extern FFI functions absent on the target.
    let mut compile_time_constants: HashMap<u32, f64> = HashMap::new();
    for s in &hir.init {
        if let perry_hir::Stmt::Let {
            id,
            name,
            init: None,
            ..
        } = s
        {
            match name.as_str() {
                "__platform__" => {
                    compile_time_constants.insert(*id, platform_number);
                }
                "__plugins__" => {
                    compile_time_constants.insert(*id, 0.0);
                }
                _ => {}
            }
        }
    }

    // Issue #235: per-method explicit-param-count map covering BOTH local
    // classes (from `hir.classes`) AND imported classes (from
    // `opts.imported_classes`). Every method-call dispatch site in
    // `lower_call.rs` looks up here to pad missing trailing args with
    // TAG_UNDEFINED so the callee's default-param desugaring (`if (options
    // === undefined) options = {}`) fires correctly. Pre-fix the dispatch
    // tower passed only the user-provided args, leaving the callee to read
    // uninitialized arg-register slots for any param the caller skipped —
    // a real heap pointer from a prior call's leftover state, which when
    // dereferenced for `options.session` silently hung in the dispatch chain.
    let mut method_param_counts: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    // Parallel `(class, method) → has_rest_param` map. Closes #484:
    // `b.with(1)` on `class Builder { with<T>(id, ...args: T extends void ?
    // [] | [void] : [T]): this }` left `args` as undefined because the
    // codegen-side dispatch table didn't track the rest bit, so the
    // call site never bundled trailing args (zero, in that test) into
    // a `js_array_alloc(0)` rest array. The conditional rest type is
    // a red herring — even `...args: any[]` would have shown the same
    // signature gap, except for the freestanding-function path which
    // already had `func_signatures.has_rest`.
    let mut method_has_rest: std::collections::HashMap<(String, String), bool> =
        std::collections::HashMap::new();
    for cls in &hir.classes {
        for m in &cls.methods {
            method_param_counts.insert((cls.name.clone(), m.name.clone()), m.params.len());
            let has_rest = m.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), m.name.clone()), true);
            }
        }
        // Issue #894: track static methods too. Effect's `static pipe()` /
        // `static annotations()` synthesize a trailing `...arguments` rest
        // param when the body reads `arguments`. The StaticMethodCall
        // lowering at `expr.rs::Expr::StaticMethodCall` reads
        // `method_has_rest` to decide whether to bundle trailing args into
        // a rest array; without this, `Cls.pipe(a, b)` calls the method
        // with 2 scalar args while the signature expects (rest_array),
        // and `arguments.length` reads garbage / undefined.
        for sm in &cls.static_methods {
            method_param_counts.insert((cls.name.clone(), sm.name.clone()), sm.params.len());
            let has_rest = sm.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), sm.name.clone()), true);
            }
        }
    }
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name).to_string();
        for (i, mname) in ic.method_names.iter().enumerate() {
            // Default to 0 if the source side hasn't populated method_param_counts
            // yet (legacy ImportedClass with no parallel Vec). 0 means "no padding".
            let count = ic.method_param_counts.get(i).copied().unwrap_or(0);
            // Register under the canonical class name and the local alias if any.
            method_param_counts.insert((ic.name.clone(), mname.clone()), count);
            if effective_name != ic.name {
                method_param_counts.insert((effective_name.clone(), mname.clone()), count);
            }
            // Issue #672: same propagation for the rest-flag side. Without this,
            // call sites to imported-class methods with `...rest` parameters
            // skipped the rest-array packing path, leaving trailing positional
            // args either dropped or silently spread into the next slot —
            // `c.cmd("SET", "k", "v")` reached the callee as `args = "k"`.
            if ic.method_has_rest.get(i).copied().unwrap_or(false) {
                method_has_rest.insert((ic.name.clone(), mname.clone()), true);
                if effective_name != ic.name {
                    method_has_rest.insert((effective_name.clone(), mname.clone()), true);
                }
            }
        }
    }

    // Refs #915 (gap 3 / #321 follow-up): tag functions whose body
    // unconditionally returns a `ClassRef` (or transitively returns
    // another such factory) so call sites of the form
    // `Literal(value).pipe(...)` can dispatch the `.pipe` lookup as a
    // static-method call on the returned class. Iterate until
    // fixed-point so `Literal(value)` (which calls `makeLiteralClass`)
    // resolves to the same class as `makeLiteralClass(...)`.
    let mut func_returns_class_map: std::collections::HashMap<u32, String> =
        std::collections::HashMap::new();
    let n_funcs_for_factory_pass = hir.functions.len();
    for _ in 0..n_funcs_for_factory_pass {
        let mut changed = false;
        for f in &hir.functions {
            if func_returns_class_map.contains_key(&f.id) {
                continue;
            }
            let mut produced: Option<String> = None;
            let mut disqualified = false;
            collect_return_class(
                &f.body,
                &mut produced,
                &mut disqualified,
                &func_returns_class_map,
            );
            if !disqualified {
                if let Some(class_name) = produced {
                    func_returns_class_map.insert(f.id, class_name);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Build the cross-module context bundle from CompileOptions.
    let cross_module = CrossModuleCtx {
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        namespace_reexport_named_imports: opts.namespace_reexport_named_imports.clone(),
        namespace_member_prefixes: opts.namespace_member_prefixes,
        imported_async_funcs: opts.imported_async_funcs,
        local_async_funcs,
        type_aliases: opts.type_aliases,
        imported_func_param_counts: opts.imported_func_param_counts,
        import_function_origin_names: opts.import_function_origin_names.clone(),
        import_function_v8_specifiers: opts.import_function_v8_specifiers.clone(),
        // Issue #841: see CrossModuleCtx field docs.
        import_function_node_submodule: opts.import_function_node_submodule.clone(),
        namespace_node_submodules: opts.namespace_node_submodules.clone(),
        namespace_v8_specifiers: opts.namespace_v8_specifiers.clone(),
        imported_func_has_rest: opts.imported_func_has_rest,
        imported_func_return_types: opts.imported_func_return_types,
        func_returns_class: func_returns_class_map,
        method_param_counts,
        method_has_rest,
        class_keys_globals: class_keys_globals_map,
        imported_class_ctors: opts
            .imported_classes
            .iter()
            .map(|ic| {
                let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
                let ctor_name = format!("{}__{}_constructor", ic.source_prefix, ic.name);
                (
                    effective_name.to_string(),
                    (ctor_name, ic.constructor_param_count),
                )
            })
            .collect(),
        // Per-module i18n lowering context. Built from `opts.i18n_table`
        // when i18n is configured; `None` otherwise. The
        // `Expr::I18nString` lowering pulls the right translation row at
        // compile time using `default_locale_idx` and emits the resolved
        // string (with runtime interpolation for `{name}` placeholders).
        i18n: opts.i18n_table.as_ref().map(|arc| {
            // Tier 4.6: deref the `Arc<Tuple>` to access the inner
            // tuple fields. The `translations.clone()` here is still a
            // per-module Vec clone — wrapping the I18nLowerCtx field
            // in Arc too would eliminate it, but is a wider refactor
            // tracked as a follow-up.
            let (translations, key_count, _locale_count, _locale_codes, default_locale_idx) =
                arc.as_ref();
            crate::expr::I18nLowerCtx {
                translations: translations.clone(),
                key_count: *key_count,
                default_locale_idx: *default_locale_idx,
            }
        }),
        imported_vars: opts.imported_vars,
        needs_stdlib: opts.needs_stdlib,
        needs_geisterhand: opts.needs_geisterhand,
        geisterhand_port: opts.geisterhand_port,
        needs_js_runtime: opts.needs_js_runtime,
        compile_time_constants,
        app_metadata: opts.app_metadata.clone(),
        clamp3_functions: hir
            .functions
            .iter()
            .filter_map(|f| crate::collectors::detect_clamp3(f).map(|_| f.id))
            .collect(),
        clamp_u8_functions: hir
            .functions
            .iter()
            .filter(|f| crate::collectors::detect_clamp_u8(f))
            .map(|f| f.id)
            .collect(),
        returns_int_functions: hir
            .functions
            .iter()
            .filter(|f| crate::collectors::returns_integer(f))
            .map(|f| f.id)
            .collect(),
        flat_const_arrays: {
            // Issue #50: fold module-level `const X: number[][] = [[int, ...], ...]`
            // into a flat `[N x i32]` LLVM constant so `X[i][j]` / `krow[j]` can
            // load directly from `.rodata` instead of chasing the arena array
            // header. Qualifying locals are `Let { mutable: false }`, have a
            // rectangular int-literal 2D init, and are never mutated anywhere
            // in the module (LocalSet/Update/IndexSet/mutating methods).
            let mut map: std::collections::HashMap<u32, crate::expr::FlatConstInfo> =
                std::collections::HashMap::new();
            for s in &hir.init {
                if let perry_hir::Stmt::Let {
                    id,
                    init: Some(init),
                    mutable: false,
                    ..
                } = s
                {
                    if let Some((rows, cols, vals)) = crate::expr::try_flat_const_2d_int(init) {
                        let mut mutated = false;
                        if crate::collectors::has_any_mutation(&hir.init, *id) {
                            mutated = true;
                        }
                        if !mutated {
                            for f in &hir.functions {
                                if crate::collectors::has_any_mutation(&f.body, *id) {
                                    mutated = true;
                                    break;
                                }
                            }
                        }
                        if !mutated {
                            'outer: for c in &hir.classes {
                                for m in &c.methods {
                                    if crate::collectors::has_any_mutation(&m.body, *id) {
                                        mutated = true;
                                        break 'outer;
                                    }
                                }
                                if let Some(ctor) = &c.constructor {
                                    if crate::collectors::has_any_mutation(&ctor.body, *id) {
                                        mutated = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !mutated {
                            let gname = format!("perry_flat_{}__{}", module_prefix, id);
                            let init_str = format!(
                                "[{}]",
                                vals.iter()
                                    .map(|v| format!("i32 {}", v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                            let ty = format!("[{} x i32]", rows * cols);
                            llmod.add_raw_global(format!(
                                "@{} = private unnamed_addr constant {} {}",
                                gname, ty, init_str
                            ));
                            map.insert(
                                *id,
                                crate::expr::FlatConstInfo {
                                    global_name: gname,
                                    rows,
                                    cols,
                                },
                            );
                        }
                    }
                }
            }
            map
        },
        // FFI manifest: each `native_library_functions` entry is
        // `(function_name, param_kinds, return_kind)` from the package.json
        // `nativeLibrary.functions` declaration. Build a name → (params, returns)
        // map so `lower_call` can emit the correct LLVM signature for direct
        // calls to native C/Rust functions (matters when the C ABI differs
        // from Perry's all-double default — e.g. `*mut View` returns in `x0`,
        // not `d0`).
        ffi_signatures: opts
            .native_library_functions
            .iter()
            .map(|(name, params, ret)| (name.clone(), (params.clone(), ret.clone())))
            .collect(),
        // Per-module local-name → import-source map. Walks `hir.imports`
        // and records every named/default import binding's source spec.
        // `lower_builtin_new` consults this to gate ambiguously-named
        // built-in arms (Client / Pool / Database / Redis / MongoClient /
        // Decimal) on the import source — `import Client from
        // "better-sqlite3"` should not dispatch through pg's Client arm.
        // See issue #602.
        imported_class_sources: {
            let mut map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for import in &hir.imports {
                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Named { local, .. }
                        | perry_hir::ImportSpecifier::Default { local } => {
                            map.insert(local.clone(), import.source.clone());
                        }
                        perry_hir::ImportSpecifier::Namespace { .. } => {}
                    }
                }
            }
            map
        },
        interfaces: hir
            .interfaces
            .iter()
            .map(|i| (i.name.clone(), i.clone()))
            .collect(),
        namespace_entries: opts.namespace_entries.clone(),
        dynamic_import_path_to_prefix: opts.dynamic_import_path_to_prefix.clone(),
        deferred_module_prefixes: opts.deferred_module_prefixes.clone(),
        module_init_deps: opts.module_init_deps.clone(),
        is_dynamic_import_target: opts.is_dynamic_import_target,
    };

    // Module-level globals registry. Pre-walk:
    //   1. Collect every LocalId referenced from any function or method
    //      body (LocalGet / LocalSet / Update). Those that aren't a
    //      function/method's own param or Let must be module-level.
    //   2. Walk hir.init's top-level Lets and globalize ONLY the ones in
    //      that set. Lets that are only referenced from main itself stay
    //      as cheap stack alloca (preserves perf for the bench
    //      benchmarks that don't share state with helper functions).
    let mut referenced_from_fn: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Helper that handles "params + lets define a scope, refs minus
    // defines flow out". Used for every function/method/closure body.
    let scan_body = |params: &[perry_hir::Param],
                     body: &[perry_hir::Stmt],
                     out: &mut std::collections::HashSet<u32>| {
        let mut local_defs: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        collect_let_ids(body, &mut local_defs);
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut refs);
        for r in refs {
            if !local_defs.contains(&r) {
                out.insert(r);
            }
        }
    };
    for f in &hir.functions {
        scan_body(&f.params, &f.body, &mut referenced_from_fn);
    }
    for c in &hir.classes {
        for m in &c.methods {
            scan_body(&m.params, &m.body, &mut referenced_from_fn);
        }
        if let Some(ctor) = &c.constructor {
            scan_body(&ctor.params, &ctor.body, &mut referenced_from_fn);
        }
    }
    // Also walk every closure body. A self-referencing recursive
    // closure (`let f = (n) => f(n-1)`) needs `f` to be globalized
    // so the closure body can see the live storage instead of a
    // stale snapshot. Without this, the closure auto-capture sees
    // `f` is not yet declared and bails with "local not in scope".
    {
        let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: same gap as the main compile loop — static field inits
            // (`static make = (x) => ...`) need walking so the global-
            // detection pre-walk sees their captures and globalises any
            // module-level lets the closure body references.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
        for (_, closure_expr) in &closures {
            if let perry_hir::Expr::Closure { params, body, .. } = closure_expr {
                scan_body(params, body, &mut referenced_from_fn);
            }
        }
    }

    let mut module_globals: HashMap<u32, String> = HashMap::new();
    // Module global types: propagated to every FnCtx so functions that
    // access module globals (via LocalGet/LocalSet) see the correct
    // declared type. Without this, `editorInstance` (Named("Editor"))
    // in render.ts has its type only in the entry function's FnCtx,
    // so method calls in other functions fall through to the generic
    // dispatch instead of the class method registry.
    let mut module_global_types: HashMap<u32, perry_types::Type> = HashMap::new();
    // Collect exported variable names so we can create external
    // globals + getter functions for cross-module access.
    let exported_var_names: std::collections::HashSet<String> =
        hir.exported_objects.iter().cloned().collect();
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, name, ty, .. } = s {
            // Always record the declared type for module-level lets
            // so all functions see it (not just the entry function).
            if !matches!(ty, perry_types::Type::Any) {
                module_global_types.insert(*id, ty.clone());
            }
            if referenced_from_fn.contains(id) || exported_var_names.contains(name) {
                // Use external linkage for exported vars so other
                // modules can reference them. Internal for the rest.
                let is_exported = exported_var_names.contains(name);
                let global_name = format!("perry_global_{}__{}", module_prefix, id);
                // Use the compile-time constant value if one was registered
                // (e.g., __platform__, __plugins__). Otherwise default to 0.0.
                let init_value = if let Some(cv) = cross_module.compile_time_constants.get(id) {
                    format!("{:.1}", cv)
                } else {
                    "0.0".to_string()
                };
                // Use default (external) linkage for ALL module globals.
                // `internal` linkage lets clang -O3 assume the global is
                // never written by optnone functions (setjmp/try-catch),
                // causing it to constant-fold reads to 0.0. With external
                // linkage, the optimizer can't make cross-TU assumptions.
                // The module-unique name (perry_global_<prefix>__N)
                // prevents symbol collisions across modules.
                llmod.add_global(&global_name, DOUBLE, &init_value);
                module_globals.insert(*id, global_name.clone());

                // For exported variables, also emit a trivial getter
                // function `perry_fn_<prefix>__<name>` that returns
                // the global. The ExternFuncRef wrapper in importing
                // modules calls this symbol — without it, exported
                // constants (like `export const Key = { ... }`) cause
                // linker errors because the wrapper tries to call a
                // function that doesn't exist.
                // Skip the getter for names that are also functions — the
                // compiled function body will provide the correct symbol.
                // Without this, `export function isSetupComplete()` gets
                // a trivial getter that wraps a broken _i64 stub (returns 0)
                // instead of the real function that reads the module global.
                let is_also_function = hir
                    .functions
                    .iter()
                    .any(|f| f.is_exported && f.name == *name);
                // Also skip the value-getter when this name is already an
                // exported function alias (e.g. `export const async = _async`
                // or `export { _void as void }`). For those the #460 forwarding
                // wrapper below emits a `perry_fn_<modprefix>__<name>`
                // definition that actually calls the underlying function;
                // emitting a getter here on top would be a redef and is
                // semantically wrong (it'd return the closure value instead
                // of invoking it).
                let is_function_alias = hir.exported_functions.iter().any(|(exp, _)| exp == name);
                if is_exported && !is_also_function && !is_function_alias {
                    let fn_name = format!("perry_fn_{}__{}", module_prefix, sanitize(name),);
                    let getter = llmod.define_function(&fn_name, DOUBLE, vec![]);
                    let _ = getter.create_block("entry");
                    let blk = getter.block_mut(0).unwrap();
                    let val = blk.load(DOUBLE, &format!("@{}", global_name));
                    blk.ret(DOUBLE, &val);

                    // #460: also emit a duplicate getter under any renamed
                    // export targeting this local. `export { _await as await }`
                    // means consumers compute the callee symbol from the
                    // exported name `await` — without an alias getter the
                    // link fails on `_perry_fn_<mod>__<keyword>`. The wrapper
                    // returns the same global value the local-name getter
                    // returns; callers that invoke it as a function get the
                    // closure handle (matching status quo for non-renamed
                    // `export const f = aFunctionRef` exports).
                    for export in &hir.exports {
                        if let perry_hir::Export::Named { local, exported } = export {
                            if local == name && exported != name {
                                let alias_fn =
                                    format!("perry_fn_{}__{}", module_prefix, sanitize(exported));
                                if alias_fn == fn_name {
                                    continue;
                                }
                                let g = llmod.define_function(&alias_fn, DOUBLE, vec![]);
                                let _ = g.create_block("entry");
                                let b = g.block_mut(0).unwrap();
                                let v = b.load(DOUBLE, &format!("@{}", global_name));
                                b.ret(DOUBLE, &v);
                            }
                        }
                    }
                }
            }
        }
    }

    // Phase E: register and emit static class fields as module globals.
    // Each `static foo: T = init` becomes `@perry_static_<modprefix>__
    // <class>__<field>` initialized to 0.0. The init expression runs
    // in compile_module_entry's main/init function before user code.
    let mut static_field_globals: HashMap<(String, String), String> = HashMap::new();
    for c in &hir.classes {
        for sf in &c.static_fields {
            // Computed-key static fields (`static [Symbol.for(...)] = init`)
            // are stored in a runtime side table by
            // `init_static_fields_late`; they don't get a string-named
            // global. Refs #420, #894.
            if sf.key_expr.is_some() {
                continue;
            }
            let name = format!(
                "perry_static_{}__{}__{}",
                module_prefix,
                sanitize(&c.name),
                sanitize(&sf.name),
            );
            // External linkage so importing modules can reference the same
            // global. Static class fields are spec-level shared state across
            // the whole program (same `Symbol.X` value seen everywhere); they
            // must be a single defining global, not per-module copies.
            // Refs #420: drizzle's `Sub extends Base` reads `[Base.Symbol.X]`
            // when Sub is in a different file from Base; without external
            // linkage, the importing module's `StaticFieldGet { Base, Symbol }`
            // had no symbol to resolve and silently produced 0.0.
            llmod.add_global(&name, DOUBLE, "0.0");
            static_field_globals.insert((c.name.clone(), sf.name.clone()), name);
        }
    }
    // Register foreign static-field globals from imported classes. The source
    // module emits the defining external global (above); the consumer just
    // declares a reference and adds it to its own `static_field_globals` map
    // so `Expr::StaticFieldGet/Set` lowering finds it.
    // Track which globals we've already emitted to avoid double-declarations.
    let mut external_globals_emitted: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip imported-class entries whose source matches this module's
        // prefix — the local-class loop above already emitted the defining
        // global. Re-declaring as external would produce a duplicate-symbol
        // error in the LLVM IR (clang rejects `@x = global` next to `@x =
        // external global`). Same-named local classes also win.
        if ic.source_prefix == module_prefix {
            // Still register in the static_field_globals map so HIR lookups
            // by the imported alias resolve to the local definition.
            for sf_name in &ic.static_field_names {
                let key = (effective_name.to_string(), sf_name.clone());
                if !static_field_globals.contains_key(&key) {
                    let global_name = format!(
                        "perry_static_{}__{}__{}",
                        module_prefix,
                        sanitize(&ic.name),
                        sanitize(sf_name),
                    );
                    static_field_globals.insert(key, global_name);
                }
            }
            continue;
        }
        if hir.classes.iter().any(|c| c.name == ic.name) {
            continue;
        }
        for sf_name in &ic.static_field_names {
            let global_name = format!(
                "perry_static_{}__{}__{}",
                ic.source_prefix,
                sanitize(&ic.name),
                sanitize(sf_name),
            );
            // Declare external (not define) — the source module owns the
            // defining global. Skip if already declared (multiple imports of
            // the same class).
            if external_globals_emitted.insert(global_name.clone()) {
                llmod.add_external_global(&global_name, DOUBLE);
            }
            // Register under both the alias (if any) and the source name so
            // either resolves.
            static_field_globals.insert(
                (effective_name.to_string(), sf_name.clone()),
                global_name.clone(),
            );
            if effective_name != ic.name {
                static_field_globals.insert((ic.name.clone(), sf_name.clone()), global_name);
            }
        }
    }

    // Method registry: (class_name, method_name) → LLVM function name.
    // Built from `class.methods` so the dispatch in `lower_call` knows
    // which mangled function name to call for `obj.method(args)`. Method
    // names are also scoped by module prefix.
    let mut method_names: HashMap<(String, String), String> = HashMap::new();
    for c in class_table.values() {
        // Use the source module prefix for imported classes so the method
        // symbol name matches where the method was actually compiled.
        let class_prefix = imported_class_prefix.get(&c.name).unwrap_or(&module_prefix);
        // Issue #568: when `c` is the stub for an imported renamed class
        // (`export { Widget as PublicWidget }` consumed via
        // `import { PublicWidget }`), `c.name` is the local alias
        // ("PublicWidget"). The source module emits its symbols mangled
        // with the ORIGINAL name ("Widget"); the consumer-side LLVM
        // symbol must match. `mangle_class_name` is the source-side
        // canonical name; the dispatch-table KEY stays `c.name` so
        // `receiver_class_name` lookups (which see the renamed type)
        // still hit.
        let mangle_class_name = imported_class_source_name
            .get(&c.name)
            .map(|s| s.as_str())
            .unwrap_or(c.name.as_str());
        for m in &c.methods {
            let llvm_name = scoped_method_name(class_prefix, mangle_class_name, &m.name);
            method_names.insert((c.name.clone(), m.name.clone()), llvm_name.clone());
            // Refs #486: also register self-binding aliases (e.g. `_X` from
            // `var X = class _X`) so static method dispatch on a receiver typed
            // as `_X` (the inner name) finds the same LLVM symbol as the
            // canonical `X`-typed dispatch.
            for alias in &c.aliases {
                method_names
                    .entry((alias.clone(), m.name.clone()))
                    .or_insert_with(|| llvm_name.clone());
            }
        }
        // Constructor: register as a method so compile_method can find it.
        // Emitted for ALL classes (even without explicit constructors)
        // so cross-module `new` can call the constructor.
        {
            let ctor_method_name = format!("{}_constructor", c.name);
            method_names.insert(
                (c.name.clone(), ctor_method_name.clone()),
                format!("{}__{}_constructor", class_prefix, mangle_class_name),
            );
        }
        // Getters: register under the property name with a `__get_`
        // prefix to avoid colliding with a regular method of the same
        // name. The dispatch site for `obj.prop` checks the getter
        // map first, then falls back to the regular method registry.
        for (prop, f) in &c.getters {
            method_names.insert(
                (c.name.clone(), format!("__get_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__get_{}", f.name),
                ),
            );
        }
        for (prop, f) in &c.setters {
            method_names.insert(
                (c.name.clone(), format!("__set_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__set_{}", f.name),
                ),
            );
        }
        // Static methods. Registered under their plain method name
        // so `Counter.increment()` (StaticMethodCall) can look them
        // up the same way as instance methods, but emitted as
        // `perry_static_<modprefix>__<class>__<method>` (no `this`).
        // The class/method names are sanitized so private methods
        // (`#helper`) produce a valid LLVM identifier.
        for sm in &c.static_methods {
            method_names.insert(
                (c.name.clone(), sm.name.clone()),
                format!(
                    "perry_static_{}__{}__{}",
                    class_prefix,
                    sanitize(mangle_class_name),
                    sanitize(&sm.name),
                ),
            );
        }
    }

    // Phase F: register imported class methods in the method_names
    // registry and pre-declare them as extern LLVM functions so the
    // linker can resolve cross-module method calls.
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip if locally defined — local methods take precedence.
        if hir.classes.iter().any(|c| c.name == *effective_name) {
            continue;
        }
        let src = &ic.source_prefix;

        for (method_idx, method_name) in ic.method_names.iter().enumerate() {
            // The source module emitted its methods as
            // `perry_method_<source_prefix>__<class>__<method>`.
            // Use the canonical class name (ic.name) for the symbol
            // since that's how the source module mangled it.
            let llvm_fn = format!(
                "perry_method_{}__{}__{}",
                sanitize(src),
                sanitize(&ic.name),
                sanitize(method_name),
            );
            method_names
                .entry((effective_name.to_string(), method_name.clone()))
                .or_insert_with(|| llvm_fn.clone());

            // Declare extern: `double method(double this, double arg0, …)`.
            // Pre-#235 this was hardcoded to 6 doubles ("safe upper bound").
            // The bug: call sites that passed fewer args (the common case for
            // methods with default params) made the callee read garbage from
            // uninitialized arg-register slots — typically a real heap pointer
            // from a prior call's leftover state. Dereferencing that garbage
            // for `options.session` etc. silently hung in the dispatch chain.
            // We now read the actual arity from the parallel
            // `method_param_counts` Vec populated by the source side. If the
            // source module didn't populate it (legacy or out-of-sync build),
            // fall back to 6 to preserve compat.
            // Total arity = explicit params + 1 implicit `this`.
            let arity = ic
                .method_param_counts
                .get(method_idx)
                .copied()
                .map(|n| n + 1)
                .unwrap_or(6);
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat_n(DOUBLE, arity).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }

        // Cross-module getters. The dispatch site at
        // `expr.rs::PropertyGet` looks up `(class, "__get_<prop>")` in
        // `method_names`; without this loop the entry is missing for
        // imported classes and `obj.prop` silently falls through to
        // `undefined`. The source module mangles getters as
        // `perry_method_<src>__<class>____get_get_<prop>` (the inner
        // `get_<prop>` is the HIR function name from
        // `lower_getter_method`, then codegen prepends `__get_`).
        for prop in &ic.getter_names {
            let inner_fn_name = format!("get_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__get_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__get_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Getters take only `this` (NaN-boxed double) and return double.
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE]);
        }

        // Cross-module setters. Symmetric to getters: source-side
        // mangling is `perry_method_<src>__<class>____set_set_<prop>`.
        for prop in &ic.setter_names {
            let inner_fn_name = format!("set_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__set_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__set_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Setters take `this` plus the new value, both NaN-boxed
            // doubles, and return double (the assigned value).
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE, DOUBLE]);
        }

        // Constructor: declared as
        // `<source_prefix>__<class>_constructor(i64 this, double arg0, …) → void`
        let ctor_fn = format!("{}__{}_constructor", sanitize(src), sanitize(&ic.name),);
        let mut ctor_params: Vec<crate::types::LlvmType> = vec![DOUBLE];
        for _ in 0..ic.constructor_param_count {
            ctor_params.push(DOUBLE);
        }
        llmod.declare_function(&ctor_fn, VOID, &ctor_params);

        // Cross-module static methods. Source module emits these as
        // `perry_static_<source_prefix>__<class>__<method>` (no `this`
        // receiver). Register them in `method_names` under the same
        // (class, method) key the StaticMethodCall lowering looks up.
        for sm in &ic.static_method_names {
            let llvm_fn = format!(
                "perry_static_{}__{}__{}",
                sanitize(src),
                sanitize(&ic.name),
                sanitize(sm),
            );
            method_names
                .entry((effective_name.to_string(), sm.clone()))
                .or_insert_with(|| llvm_fn.clone());
            // Declare conservatively with 6 double params; LLVM's direct-call
            // resolution doesn't require an exact arity match for declarations.
            let param_types: Vec<crate::types::LlvmType> = std::iter::repeat_n(DOUBLE, 6).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }
    }

    // Resolve user function names up-front so body lowering can emit
    // forward/recursive calls without worrying about emission order.
    // Names are scoped by module prefix to avoid cross-module collisions.
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool, bool)> = HashMap::new();
    let mut func_synthetic_arguments: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    for f in &hir.functions {
        func_names.insert(f.id, scoped_fn_name(&module_prefix, &f.name));
        let has_rest = f.params.iter().any(|p| p.is_rest);
        if f.params
            .last()
            .map(|p| p.is_rest && p.name == "arguments")
            .unwrap_or(false)
        {
            func_synthetic_arguments.insert(f.id);
        }
        let returns_number = matches!(
            f.return_type,
            perry_types::Type::Number | perry_types::Type::Int32
        );
        func_signatures.insert(f.id, (f.params.len(), has_rest, returns_number));
    }

    // Module-level boxed_vars: union of every per-function/method/
    // closure/module-init boxed set. We compute this once here because
    // closures emitted in `compile_closure` need to know whether their
    // transitively-captured ids from an enclosing function were boxed
    // at the creation site. Since HIR LocalIds are globally unique
    // across the module, a single union set is enough: each id either
    // lives in a box or it doesn't, irrespective of which function
    // owns it.
    let mut module_boxed_vars: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        module_boxed_vars.extend(collect_boxed_vars(&f.body));
    }
    for c in &hir.classes {
        for m in &c.methods {
            module_boxed_vars.extend(collect_boxed_vars(&m.body));
        }
        for (_, getter_fn) in &c.getters {
            module_boxed_vars.extend(collect_boxed_vars(&getter_fn.body));
        }
        for (_, setter_fn) in &c.setters {
            module_boxed_vars.extend(collect_boxed_vars(&setter_fn.body));
        }
        for sm in &c.static_methods {
            module_boxed_vars.extend(collect_boxed_vars(&sm.body));
        }
        if let Some(ctor) = &c.constructor {
            module_boxed_vars.extend(collect_boxed_vars(&ctor.body));
        }
    }
    module_boxed_vars.extend(collect_boxed_vars(&hir.init));

    // Module-wide LocalId → Type map. Used by closure bodies to
    // learn the types of captured vars from the enclosing scope.
    // HIR LocalIds are globally unique within the module, so a
    // single flat map works.
    let mut module_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
    collect_let_types_in_stmts(&hir.init, &mut module_local_types);
    for f in &hir.functions {
        for p in &f.params {
            module_local_types.insert(p.id, p.ty.clone());
        }
        collect_let_types_in_stmts(&f.body, &mut module_local_types);
    }
    for c in &hir.classes {
        for m in &c.methods {
            for p in &m.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&m.body, &mut module_local_types);
        }
        for (_, getter_fn) in &c.getters {
            for p in &getter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&getter_fn.body, &mut module_local_types);
        }
        for (_, setter_fn) in &c.setters {
            for p in &setter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&setter_fn.body, &mut module_local_types);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&ctor.body, &mut module_local_types);
        }
        for sm in &c.static_methods {
            for p in &sm.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&sm.body, &mut module_local_types);
        }
    }

    // Cross-module function declares are emitted lazily by `lower_call`
    // via `FnCtx.pending_declares` (drained back into `llmod` at the
    // end of each compile_function/closure/method/static call). The
    // previous pre-walker (`collect_extern_func_refs_in_*`) had to
    // mirror the entire HIR Expr/Stmt grammar to find every cross-module
    // call shape — it missed `Expr::Closure` bodies, `Stmt::Try`/`Switch`,
    // and many other containers, which produced clang
    // "use of undefined value @perry_fn_*" errors when a call was hidden
    // inside an arrow callback. Lazy emission tracks declares at the
    // actual emission point so any path the lowering reaches is covered.

    // Pre-walk for closures: every `Expr::Closure` in the program needs
    // its body emitted as a top-level LLVM function so the closure
    // creation site can take its address. Collect them all first, then
    // emit each via `compile_closure` (Phase D.1).
    //
    // We must walk every container that the compile loop below also
    // compiles — methods, ctors, getters, setters, static_methods —
    // otherwise a closure body in (say) a `get size() { return arr.filter(...).length }`
    // ends up referenced by `js_closure_alloc(@perry_closure_*)` but
    // never defined, and clang errors with "use of undefined value".
    let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
    {
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: static fields with closure inits (`static make = (x) =>
            // ...`) emit `js_closure_alloc(@perry_closure_*)` at module-init
            // time too — the codegen path that initialises
            // `@perry_static_<class>__<field>` globals. Pre-fix this loop
            // walked instance fields (`c.fields`) only, so closures inside
            // `c.static_fields[i].init` were never collected and clang
            // errored on the undefined `@perry_closure_*` reference.
            // Surfaced on Effect's `SchemaAST.ts` (Union.make / Union.unify)
            // and any class shipping arrow-style static helpers.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
    }

    // Build closure rest param index: for each closure that has a rest
    // parameter, record its func_id → rest param position. Used by
    // the closure call site in `lower_call` to bundle trailing args.
    let closure_rest_params: HashMap<u32, usize> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                params.iter().position(|p| p.is_rest).map(|idx| (*fid, idx))
            } else {
                None
            }
        })
        .collect();

    // Refs #915 (gap 1 from #899): closures whose rest param is the
    // HIR-synthesized `arguments` need to bundle ALL passed args into
    // the rest slot at dispatch time — JS spec semantics for
    // `arguments.length` count every passed arg, not just the trailing
    // tail after the fixed params. The runtime side reads this through
    // `js_register_closure_synthetic_arguments` (vs the regular
    // `js_register_closure_rest`).
    let closure_synthetic_arguments: std::collections::HashSet<u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                let last_is_synth_args = params
                    .last()
                    .map(|p| p.is_rest && p.name == "arguments")
                    .unwrap_or(false);
                if last_is_synth_args {
                    Some(*fid)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Refs #421: declared param count for every non-rest closure. Used by
    // `emit_string_pool` to register each closure's arity so the runtime can
    // pad missing args with TAG_UNDEFINED in the dynamic-dispatch path.
    let closure_arities: HashMap<u32, u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                if params.iter().any(|p| p.is_rest) {
                    return None;
                }
                Some((*fid, params.len() as u32))
            } else {
                None
            }
        })
        .collect();

    // Integer specialization: for pure numeric recursive functions (like
    // fibonacci), emit an i64 variant that uses integer registers and
    // integer arithmetic. The f64 wrapper calls fptosi → i64_fn → sitofp.
    let mut i64_specialized: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        // Skip integer specialization for functions that access module globals.
        // The i64 body emitter can't handle module global loads (it produces
        // `ret 0` instead of reading the global), creating a broken stub
        // that shadows the real compiled function.
        let uses_module_globals = f.body.iter().any(|s| {
            fn walks(s: &perry_hir::Stmt, mg: &HashMap<u32, String>) -> bool {
                match s {
                    perry_hir::Stmt::Return(Some(perry_hir::Expr::LocalGet(id))) => {
                        mg.contains_key(id)
                    }
                    perry_hir::Stmt::Expr(perry_hir::Expr::LocalGet(id)) => mg.contains_key(id),
                    _ => false,
                }
            }
            walks(s, &module_globals)
        });
        if crate::collectors::is_integer_specializable(f) && !uses_module_globals {
            if let Some(llvm_name) = func_names.get(&f.id) {
                let i64_name = format!("{}_i64", llvm_name);
                crate::collectors::emit_i64_function(&mut llmod, f, &i64_name);
                // Emit the f64 wrapper that calls the i64 version.
                // Mark as alwaysinline so LLVM exposes the integer ops
                // to callers — critical for vectorizing clamp patterns.
                let params: Vec<(LlvmType, String)> = f
                    .params
                    .iter()
                    .map(|p| (DOUBLE, format!("%arg{}", p.id)))
                    .collect();
                let wrapper = llmod.define_function(llvm_name, DOUBLE, params);
                wrapper.force_inline = true;
                let _ = wrapper.create_block("entry");
                let blk = wrapper.block_mut(0).unwrap();
                let mut i64_args: Vec<(LlvmType, String)> = Vec::new();
                for p in &f.params {
                    let i64_v = blk.fptosi(DOUBLE, &format!("%arg{}", p.id), I64);
                    i64_args.push((I64, i64_v));
                }
                let refs: Vec<(LlvmType, &str)> =
                    i64_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
                let i64_result = blk.call(I64, &i64_name, &refs);
                let f64_result = blk.sitofp(I64, &i64_result, DOUBLE);
                blk.ret(DOUBLE, &f64_result);
                i64_specialized.insert(f.id);
            }
        }
    }

    // Lower each user function into the module (skip i64-specialized ones).
    for f in &hir.functions {
        if i64_specialized.contains(&f.id) {
            continue;
        }
        compile_function(
            &mut llmod,
            f,
            &func_names,
            &mut strings,
            &class_table,
            &method_names,
            &module_globals,
            &module_global_types,
            &opts.import_function_prefixes,
            &enum_table,
            &static_field_globals,
            &class_ids,
            &func_signatures,
            &func_synthetic_arguments,
            &module_boxed_vars,
            &closure_rest_params,
            &cross_module,
        )
        .with_context(|| format!("lowering function '{}'", f.name))?;
    }

    // Closes #460: emit forwarding wrappers for `export { local as exported }`
    // renames where the exported name differs from the function's local HIR
    // name. Without these, cross-module callers compute the callee symbol
    // from the *exported* name (`perry_fn_<src>__<exported>`) and link-fail
    // because the body was emitted under the *local* name. Bites contextual-
    // keyword renames the worst — Effect's `void_ as void`, `_async as async`,
    // `_await as await`, etc. all left link-undefined `_perry_fn_..._<keyword>`.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_id: HashMap<u32, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.id, f)).collect();
        for (exported_name, func_id) in &hir.exported_functions {
            let Some(f) = func_by_id.get(func_id) else {
                continue;
            };
            if &f.name == exported_name {
                continue;
            }
            let alias_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(exported_name));
            let target_sym = match func_names.get(func_id) {
                Some(s) => s.clone(),
                None => continue,
            };
            if alias_sym == target_sym {
                continue;
            }
            if !emitted_aliases.insert(alias_sym.clone()) {
                continue;
            }
            let param_count = f.params.len();
            let wrap_params: Vec<(LlvmType, String)> = (0..param_count)
                .map(|i| (DOUBLE, format!("%a{}", i)))
                .collect();
            let wf = llmod.define_function(&alias_sym, DOUBLE, wrap_params);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let arg_names: Vec<String> = (0..param_count).map(|i| format!("%a{}", i)).collect();
            let call_args: Vec<(LlvmType, &str)> =
                arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = blk.call(DOUBLE, &target_sym, &call_args);
            blk.ret(DOUBLE, &result);
        }
    }

    // Closes #461: emit an undefined-returning stub for every named export
    // that doesn't already have a `perry_fn_<modprefix>__<exported>` symbol.
    // The cross-module call site resolves any namespace property access to
    // `perry_fn_<src>__<name>` (lower_call.rs::ExternFuncRef path) — that
    // works for value exports because either the function body itself or
    // the variable getter at line 1099 claims the symbol. It does NOT work
    // for:
    //   * exported classes — `export class Union` produces method/keys
    //     symbols but no function-shaped getter, so `AST.Union` from a
    //     consumer link-fails on `_perry_fn_<SchemaAST>__Union`;
    //   * exported interfaces / type aliases — `export interface Order`
    //     is type-only at runtime, but type annotations like
    //     `order.Order<...>` leak into the value-position symbol resolver
    //     and link-fail on `_perry_fn_<Order_ts>__Order`.
    // The stub returns NaN-boxed undefined; that matches the consumer-side
    // no-op wrapper at line 1955 (which already returns undefined for
    // imported classes referenced as values) so the link- and runtime-
    // visible behavior of cross-module class/type references is symmetric.
    {
        use std::collections::HashSet;
        let mut emitted_stubs: HashSet<String> = HashSet::new();
        let stub_targets: Vec<String> = hir
            .exports
            .iter()
            .filter_map(|e| match e {
                perry_hir::Export::Named { exported, .. } => Some(exported.clone()),
                _ => None,
            })
            .collect();
        for exported in stub_targets {
            let stub_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&exported));
            if llmod.has_function(&stub_sym) {
                continue;
            }
            if !emitted_stubs.insert(stub_sym.clone()) {
                continue;
            }
            let wf = llmod.define_function(&stub_sym, DOUBLE, vec![]);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            blk.ret(DOUBLE, &undef);
        }
    }

    // Lower each closure body as a top-level LLVM function.
    for (func_id, closure_expr) in &closures {
        compile_closure(
            &mut llmod,
            *func_id,
            closure_expr,
            &func_names,
            &mut strings,
            &class_table,
            &method_names,
            &module_globals,
            &opts.import_function_prefixes,
            &enum_table,
            &static_field_globals,
            &class_ids,
            &func_signatures,
            &func_synthetic_arguments,
            &module_prefix,
            &module_boxed_vars,
            &module_local_types,
            &closure_rest_params,
            &cross_module,
        )
        .with_context(|| format!("lowering closure func_id={}", func_id))?;
    }

    // Lower each class method as `perry_method_<modprefix>__<class>__<name>(
    // this_box, arg0, arg1, ...) -> double`. Methods are emitted as
    // standalone LLVM functions; the dispatch in `lower_call` calls
    // them directly.
    for class in &hir.classes {
        for method in &class.methods {
            compile_method(
                &mut llmod,
                class,
                method,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &module_global_types,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &func_synthetic_arguments,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;
        }
        // Getters and setters are also methods, just registered under
        // a __get_/__set_ prefix in the registry. Emit their bodies
        // with the same prefix as the LLVM function name.
        for (prop, getter_fn) in &class.getters {
            let mut renamed = getter_fn.clone();
            renamed.name = format!("__get_{}", prop);
            compile_method(
                &mut llmod,
                class,
                &renamed,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &module_global_types,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &func_synthetic_arguments,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering getter '{}::{}'", class.name, prop))?;
        }
        for (prop, setter_fn) in &class.setters {
            let mut renamed = setter_fn.clone();
            renamed.name = format!("__set_{}", prop);
            compile_method(
                &mut llmod,
                class,
                &renamed,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &module_global_types,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &func_synthetic_arguments,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering setter '{}::{}'", class.name, prop))?;
        }
        // Emit standalone constructor for cross-module use.
        // Compiled like a method: takes (i64 this, double arg0, ...) → void.
        // The constructor name matches the import declaration:
        // `<prefix>__<class>_constructor`.
        //
        // Refs #420: when class has no own ctor but extends a parent that
        // does, JS spec's default ctor is `constructor(...args) { super(...args); }`.
        // Adopt the closest ancestor-with-ctor's params as the synthesized
        // ctor's signature, so when this standalone ctor is called via
        // super() with the args meant for the grandparent, they can be
        // forwarded correctly. The compile_method post-init step (below)
        // emits the actual super-call.
        {
            let ctor_body = if let Some(c) = class.constructor.as_ref() {
                (c.params.clone(), c.body.clone(), c.captures.clone())
            } else if class.extends_name.is_some() {
                // Walk ancestors for the first one with a ctor; adopt its
                // params (cleared of ids — they'll be fresh).
                let mut found_params: Vec<perry_hir::Param> = Vec::new();
                let mut cur = class.extends_name.clone();
                while let Some(pname) = cur {
                    // v0.5.760: also consult `opts.imported_classes` for
                    // cross-module parent ctors. Pre-fix the loop fell
                    // through to the next ancestor when `class_table`'s
                    // entry for an imported class returned a stub with
                    // `constructor: None` (stubs always have None) — even
                    // though the source module did have a real ctor with
                    // params. Result: `class Child extends Parent { x =
                    // "y" }` (no own ctor, parent in another module) had
                    // its synthesized ctor with ZERO params, so the user's
                    // `new Child("arg")` lost the arg before reaching
                    // Parent_constructor. Refs #420.
                    let imported_ctor_params = opts
                        .imported_classes
                        .iter()
                        .find(|i| i.local_alias.as_deref().unwrap_or(&i.name) == pname.as_str())
                        .map(|ic| ic.constructor_param_count)
                        .unwrap_or(0);
                    if let Some(pclass) = class_table.get(pname.as_str()) {
                        if let Some(pctor) = &pclass.constructor {
                            found_params = pctor.params.clone();
                            break;
                        }
                        if imported_ctor_params > 0 {
                            for i in 0..imported_ctor_params {
                                found_params.push(perry_hir::Param {
                                    id: 0xFFFF_0000 + i as u32,
                                    name: format!("__forward_arg{}", i),
                                    ty: perry_types::Type::Any,
                                    default: None,
                                    decorators: Vec::new(),
                                    is_rest: false,
                                });
                            }
                            break;
                        }
                        cur = pclass.extends_name.clone();
                    } else if let Some(stub) = imported_class_stubs.iter().find(|c| c.name == pname)
                    {
                        // Imported stub — params not in HIR; use its ctor
                        // param count as a synthetic count of unnamed args.
                        if imported_ctor_params > 0 {
                            for i in 0..imported_ctor_params {
                                found_params.push(perry_hir::Param {
                                    id: 0xFFFF_0000 + i as u32,
                                    name: format!("__forward_arg{}", i),
                                    ty: perry_types::Type::Any,
                                    default: None,
                                    decorators: Vec::new(),
                                    is_rest: false,
                                });
                            }
                        } else {
                            cur = stub.extends_name.clone();
                            continue;
                        }
                        break;
                    } else {
                        break;
                    }
                }
                (found_params, Vec::new(), Vec::new())
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };
            let ctor_as_method = perry_hir::Function {
                id: 0,
                name: format!("{}_constructor", class.name),
                type_params: Vec::new(),
                params: ctor_body.0,
                return_type: perry_types::Type::Void,
                body: ctor_body.1,
                is_async: false,
                is_generator: false,
                was_plain_async: false,
                was_unrolled: false,
                is_exported: false,
                captures: ctor_body.2,
                decorators: Vec::new(),
            };
            compile_method(
                &mut llmod,
                class,
                &ctor_as_method,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &module_global_types,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &func_synthetic_arguments,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering constructor for '{}'", class.name))?;
        }
        // Static methods compile as plain functions named
        // `perry_static_<modprefix>__<class>__<method>` — no `this`
        // parameter, no class_stack push.
        for sm in &class.static_methods {
            compile_static_method(
                &mut llmod,
                &class.name,
                sm,
                &func_names,
                &mut strings,
                &class_table,
                &method_names,
                &module_globals,
                &opts.import_function_prefixes,
                &enum_table,
                &static_field_globals,
                &class_ids,
                &func_signatures,
                &func_synthetic_arguments,
                &module_prefix,
                &module_boxed_vars,
                &closure_rest_params,
                &cross_module,
            )
            .with_context(|| format!("lowering static method '{}::{}'", class.name, sm.name))?;
        }
    }

    // Emit FuncRef-as-value wrappers. For each user function, generate
    // a thin wrapper `__perry_wrap_<name>` whose signature matches the
    // closure-call ABI: `double(i64 this_closure, double arg0, double
    // arg1, ...)`. The wrapper discards the closure pointer and forwards
    // the args to the underlying function.
    //
    // The wrapper exists so that `apply(add, 3, 4)` can pass `add` as
    // a value and have `apply` call it via `js_closure_call2`. Without
    // a wrapper, the closure call would invoke `add(closure, 3, 4)`
    // (wrong calling convention) instead of `add(3, 4)`.
    //
    // Wrappers are emitted unconditionally for every user function;
    // dead-code elimination at link time will remove unused ones.
    for f in &hir.functions {
        let original_name = func_names.get(&f.id).cloned().unwrap();
        // Wrapper signature: i64 closure_ptr + N doubles for args.
        // Cap at 5 since js_closure_call only goes up to 5 args.
        let arity = f.params.len().min(5);
        let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
        for i in 0..arity {
            wrap_params.push((DOUBLE, format!("%a{}", i)));
        }
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with just the arg doubles.
        let call_args: Vec<(LlvmType, &str)> = (0..arity)
            .map(|i| {
                (
                    DOUBLE,
                    if i == 0 {
                        "%a0"
                    } else if i == 1 {
                        "%a1"
                    } else if i == 2 {
                        "%a2"
                    } else if i == 3 {
                        "%a3"
                    } else {
                        "%a4"
                    },
                )
            })
            .collect();
        let result = blk.call(DOUBLE, &original_name, &call_args);
        blk.ret(DOUBLE, &result);
    }

    // Closes #837 / refs #836: emit `__perry_wrap_perry_fn_<src>__<exported>`
    // closure wrappers for every `Export::Named { local, exported }` rename
    // where `exported != local`. The regular wrapper loop above keys
    // wrappers on the local HIR name (`__perry_wrap_perry_fn_<src>__<local>`),
    // and the `perry_fn_<src>__<exported>` alias loop further down emits
    // the direct-call alias. The MISSING piece was the closure-wrapper
    // alias: consumer-side `expr.rs:~3819` builds
    // `__perry_wrap_perry_fn_<src>__<exported>` whenever an imported name
    // is referenced as a function VALUE (passed as a callback, stored in
    // a variable, etc.) — for renamed exports that symbol had no definition
    // and the link failed.
    //
    // Concrete failures pre-fix:
    //   * uuid's `v35.js`: `export default function v35(...)` — the local
    //     is `v35`, the exported name is `default`. Consumers
    //     (`v3.js`, `v5.js`) pass `v35` as a closure value to their own
    //     default wrappers, which transitively pulls
    //     `__perry_wrap_perry_fn_<v35.js>__default` into the link.
    //   * zod's `regexes.ts`: `const _null = ...; export { _null as null }`
    //     (same for `undefined`). A consumer that reads `regexes.null`
    //     /`regexes.undefined` as a value falls through to the
    //     function-shape branch (no `imported_vars` entry under the
    //     renamed name) and references the wrapper symbol.
    //
    // Function-renames forward to the local function body (same shape as
    // the regular wrapper above). Variable-renames have no callable body —
    // emit a no-op that returns undefined, matching the
    // `__perry_wrap_perry_unknown_func` fallback pattern below.
    {
        use std::collections::HashSet;
        let mut emitted_export_wrappers: HashSet<String> = HashSet::new();
        // Pre-compute the local-name → Function lookup. The renamed
        // export may resolve to a real HIR function (forward to it) or
        // to something else (variable / class / type / `export default
        // function NAME` whose body the HIR lowerer never recorded) —
        // for the latter we emit a no-op so the link still succeeds.
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            if local == exported {
                continue;
            }
            let exported_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported)
            );
            // Skip if a wrapper with this exact name already exists — the
            // regular wrapper loop above may have produced it when the
            // *local* HIR name happens to sanitize to the same symbol.
            if llmod.has_function(&exported_wrap) {
                continue;
            }
            if !emitted_export_wrappers.insert(exported_wrap.clone()) {
                continue;
            }
            // Determine the local target. Prefer a real HIR function;
            // fall back to a no-op (variable/class/type rename).
            if let Some(f) = func_by_local_name.get(local.as_str()) {
                let arity = f.params.len().min(5);
                let mut wrap_params: Vec<(LlvmType, String)> =
                    vec![(I64, "%this_closure".to_string())];
                for i in 0..arity {
                    wrap_params.push((DOUBLE, format!("%a{}", i)));
                }
                let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let target = scoped_fn_name(&module_prefix, &f.name);
                let call_args: Vec<(LlvmType, String)> =
                    (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                let call_args_ref: Vec<(LlvmType, &str)> =
                    call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                let result = blk.call(DOUBLE, &target, &call_args_ref);
                blk.ret(DOUBLE, &result);
            } else {
                // Variable / class / type rename — no callable function
                // body to forward to. Emit a no-op returning undefined,
                // mirroring `__perry_wrap_perry_unknown_func`.
                //
                // External linkage (the default): consumers in OTHER
                // translation units take this symbol's address through
                // `js_closure_alloc_singleton(@__perry_wrap_<sym>)`, so
                // the wrapper must be visible across TUs. Internal
                // linkage gets DCE'd because the defining TU never
                // references the wrapper itself — `expr.rs:~3819`
                // imports it via `pending_declares`, which generates a
                // `declare` extern in the consumer TU.
                let wf = llmod.define_function(
                    &exported_wrap,
                    DOUBLE,
                    vec![
                        (I64, "%this_closure".to_string()),
                        (DOUBLE, "%a0".to_string()),
                        (DOUBLE, "%a1".to_string()),
                        (DOUBLE, "%a2".to_string()),
                        (DOUBLE, "%a3".to_string()),
                        (DOUBLE, "%a4".to_string()),
                    ],
                );
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                blk.ret(DOUBLE, "0x7FFC000000000001");
            }
        }
    }

    // Closes #836: two additional producer-side emissions for cross-module
    // exports that the regular wrapper/getter/stub loops above leave behind.
    //
    // Sub-bug A — sanitize-mismatch raw aliases. The producer side hashes
    // every export through `sanitize()` (replaces every non-alphanumeric +
    // non-underscore char with `_`), so `export const $ZodCheck` lands at
    // `perry_fn_<src>___ZodCheck` (one `_` from the `__` separator, two
    // from the sanitized `$`). The consumer side, in contrast, builds the
    // callee symbol with `import_origin_suffix()`, which returns the
    // exported name VERBATIM — so consumer references read
    // `perry_fn_<src>__$ZodCheck` and link-fail. The mismatch hits every
    // export whose name contains `$` (zod's `$ZodCheck`/`$ZodCheckString
    // Format`/`$constructor` family — ~80 symbols on the zod surface), or
    // any other character `sanitize()` rewrites. Fix: for every named
    // export where `sanitize(name) != name`, emit raw-name aliases
    // forwarding to the sanitized definition.
    //
    // Sub-bug B — missing `__perry_wrap_perry_fn_<src>__<exported>` for
    // `local == exported` non-function exports. Concrete shape:
    //   import * as z from "./external.js";
    //   export { z };
    //   export default z;
    // The `Export::Named { local: "z", exported: "z" }` entry is skipped
    // by every loop above because `local == exported` and `z` is not a
    // HIR function (it's a namespace import alias). A consumer that
    // imports `{ z }` from this module references the wrapper symbol
    // when reading `z` as a value (e.g. `console.log(z)` or passing it
    // as a callback), and link-fails on the missing wrapper. Fix: emit
    // a no-op wrapper for every named export whose `local==exported`
    // name is not the body of a HIR function (and where no wrapper has
    // already been emitted by the regular `__perry_wrap_<fn>` loop).
    //
    // Note: anonymous `export default function() {...}` produces zero
    // exports and zero functions in the HIR today (lower.rs drops it on
    // the floor — separate bug, will need its own PR to wire up the
    // synthetic `default` name). That path is OUT OF SCOPE here; the
    // test below uses a named default to sidestep it.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            let sanitized = sanitize(exported);

            // Sub-bug A: emit raw-name aliases when the exported name
            // sanitizes to a different symbol. Two aliases per mismatch:
            //   * `perry_fn_<src>__<raw_exported>` — value/getter form,
            //     forwards to the already-emitted sanitized symbol.
            //   * `__perry_wrap_perry_fn_<src>__<raw_exported>` — closure-
            //     wrapper form, forwards to the sanitized wrapper if it
            //     exists, otherwise emits a no-op (matches the variable/
            //     class branch in the #837 loop above).
            if sanitized != *exported {
                let sanitized_target = format!("perry_fn_{}__{}", module_prefix, sanitized);
                let raw_target = format!("perry_fn_{}__{}", module_prefix, exported);
                if !llmod.has_function(&raw_target) && emitted_aliases.insert(raw_target.clone()) {
                    // Look up the param count to match the sanitized
                    // target's arity. Default to 0 — that matches the
                    // variable-getter shape (zero-arg fetcher) which is
                    // the common case here. Functions with `$`-prefixed
                    // names (rare, but possible) need to match arity;
                    // we look it up from the HIR if the local resolves
                    // to a known function.
                    let param_count = func_by_local_name
                        .get(local.as_str())
                        .map(|f| f.params.len())
                        .unwrap_or(0);
                    let wrap_params: Vec<(LlvmType, String)> = (0..param_count)
                        .map(|i| (DOUBLE, format!("%a{}", i)))
                        .collect();
                    let wf = llmod.define_function(&raw_target, DOUBLE, wrap_params);
                    let _ = wf.create_block("entry");
                    let blk = wf.block_mut(0).unwrap();
                    let arg_names: Vec<String> =
                        (0..param_count).map(|i| format!("%a{}", i)).collect();
                    let call_args: Vec<(LlvmType, &str)> =
                        arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    let result = blk.call(DOUBLE, &sanitized_target, &call_args);
                    blk.ret(DOUBLE, &result);
                }
                let raw_wrap = format!("__perry_wrap_perry_fn_{}__{}", module_prefix, exported);
                let sanitized_wrap =
                    format!("__perry_wrap_perry_fn_{}__{}", module_prefix, sanitized);
                if !llmod.has_function(&raw_wrap) && emitted_aliases.insert(raw_wrap.clone()) {
                    if llmod.has_function(&sanitized_wrap) {
                        // Forward to the sanitized wrapper. Both have the
                        // same closure-call ABI: (i64 this_closure, double
                        // a0, …, double a4).
                        let wf = llmod.define_function(
                            &raw_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let result = blk.call(
                            DOUBLE,
                            &sanitized_wrap,
                            &[
                                (I64, "%this_closure"),
                                (DOUBLE, "%a0"),
                                (DOUBLE, "%a1"),
                                (DOUBLE, "%a2"),
                                (DOUBLE, "%a3"),
                                (DOUBLE, "%a4"),
                            ],
                        );
                        blk.ret(DOUBLE, &result);
                    } else {
                        // No sanitized wrapper either (variable/class/
                        // namespace re-export with sanitize-mismatch
                        // name). Emit a no-op returning undefined.
                        let wf = llmod.define_function(
                            &raw_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }

            // Sub-bug B: emit no-op wrapper for `local==exported` named
            // exports where local isn't a HIR function and no wrapper
            // is yet defined. Catches `import * as z; export { z };`
            // and any `export { ClassName }` or `export { someConst }`
            // where the consumer reads the value as a closure.
            //
            // Issue #967: when the `local==exported` name IS registered
            // in `hir.exported_functions` as an alias for a real function
            // body (the canonical shape: `function add(a,b){…}; export
            // default add;` lowers to `Export::Named { local: "default",
            // exported: "default" }` *and* pushes `("default", add_id)`
            // into `exported_functions`), the no-op wrapper short-circuits
            // any consumer-side closure dispatch through this default
            // import. The consumer's
            // `__perry_wrap_perry_fn_<src>__default` then transmutes a
            // function pointer that returns `undefined` no matter the args
            // — `const fn = add; fn(2,3)` evaluates to `undefined` instead
            // of `5`. Ramda/date-fns trip this on every `var sum =
            // reduce(add, 0)` style barrel where `add`/`reduce` are
            // default-exports of locally-named functions. Fix: when
            // `exported_functions` points the name at a real HIR function,
            // emit a forwarding wrapper to that function's body (mirrors
            // the `local != exported` branch at L2792 above).
            if local == exported && !func_by_local_name.contains_key(local.as_str()) {
                let exported_wrap = format!(
                    "__perry_wrap_perry_fn_{}__{}",
                    module_prefix,
                    sanitize(exported)
                );
                if !llmod.has_function(&exported_wrap)
                    && emitted_aliases.insert(exported_wrap.clone())
                {
                    // Check if this name is registered as an alias
                    // pointing at a real HIR function (`export default
                    // <namedFn>` shape).
                    let aliased_func: Option<&perry_hir::Function> = hir
                        .exported_functions
                        .iter()
                        .find(|(n, _)| n == exported)
                        .and_then(|(_, fid)| hir.functions.iter().find(|f| f.id == *fid));
                    if let Some(f) = aliased_func {
                        let arity = f.params.len().min(5);
                        let mut wrap_params: Vec<(LlvmType, String)> =
                            vec![(I64, "%this_closure".to_string())];
                        for i in 0..arity {
                            wrap_params.push((DOUBLE, format!("%a{}", i)));
                        }
                        let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let target = scoped_fn_name(&module_prefix, &f.name);
                        let call_args: Vec<(LlvmType, String)> =
                            (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                        let call_args_ref: Vec<(LlvmType, &str)> =
                            call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                        let result = blk.call(DOUBLE, &target, &call_args_ref);
                        blk.ret(DOUBLE, &result);
                    } else {
                        let wf = llmod.define_function(
                            &exported_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }
        }
    }

    // Issue #774: emit closure-call wrappers for class instance methods
    // so `Expr::SuperPropertyGet` (value-form `super.<method>`) can
    // materialize them via `js_closure_alloc_singleton(@__perry_wrap_<method>)`.
    // Methods have signature `perry_method_<...>(this_box, args...)`;
    // the closure-call ABI is `(i64 closure, double a0, ...)` and
    // doesn't carry a separate `this`. Strict JS for `const fn =
    // super.greet; fn(x)` calls the method with `this=undefined`,
    // which is what we forward here.
    let mut emitted_wrappers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for class in &hir.classes {
        for method in &class.methods {
            let Some(method_fn_name) = method_names
                .get(&(class.name.clone(), method.name.clone()))
                .cloned()
            else {
                continue;
            };
            let wrap_name = format!("__perry_wrap_{}", method_fn_name);
            if !emitted_wrappers.insert(wrap_name.clone()) {
                continue;
            }
            let arity = method.params.len().min(5);
            let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
            for i in 0..arity {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            // Forward `this=undefined` then the args.
            let undef_lit = crate::nanbox::i64_literal(crate::nanbox::TAG_UNDEFINED);
            let undef_double = blk.bitcast_i64_to_double(&undef_lit);
            let mut call_args: Vec<(LlvmType, String)> = Vec::with_capacity(arity + 1);
            call_args.push((DOUBLE, undef_double));
            for i in 0..arity {
                call_args.push((DOUBLE, format!("%a{}", i)));
            }
            let call_args_ref: Vec<(LlvmType, &str)> =
                call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let result = blk.call(DOUBLE, &method_fn_name, &call_args_ref);
            blk.ret(DOUBLE, &result);
        }
    }

    // #337: emit an always-defined fallback wrapper for the
    // `perry_unknown_func` sentinel. `expr.rs::Expr::FuncRef` falls
    // through to `wrap_name = "__perry_wrap_perry_unknown_func"` when the
    // FuncRef's id isn't in `func_names` (cross-module reference whose
    // Source HIR wasn't lowered into THIS LLVM module — should normally
    // route to ExternFuncRef instead, but some HIR shapes still emit
    // FuncRef with an unresolvable id). Pre-fix the wrapper was never
    // defined and clang errored with `use of undefined value
    // @__perry_wrap_perry_unknown_func`. This stub returns TAG_UNDEFINED
    // (encoded as `f64::from_bits(0x7FFC_0000_0000_0001)` =
    // 1.7800590868057611e-307 — the canonical undefined sentinel matching
    // `value::TAG_UNDEFINED`); any runtime-side `js_closure_callN`
    // through this wrapper just observes "the callable returned
    // undefined", which is the right fail-closed behavior matching how
    // the `__perry_wrap_extern_*` wrappers handle missing extern
    // imported classes (see the existing comment block below).
    //
    // Emitted unconditionally — link-time DCE strips it when no
    // `Expr::FuncRef(unknown_id)` site exists in this module.
    {
        let wrap_name = "__perry_wrap_perry_unknown_func";
        let wf = llmod.define_function(
            wrap_name,
            DOUBLE,
            vec![
                (I64, "%this_closure".to_string()),
                (DOUBLE, "%a0".to_string()),
                (DOUBLE, "%a1".to_string()),
                (DOUBLE, "%a2".to_string()),
                (DOUBLE, "%a3".to_string()),
                (DOUBLE, "%a4".to_string()),
            ],
        );
        // Fix #420 (v0.5.576): internal linkage so multi-module
        // programs (drizzle-orm has 5+ modules each emitting this
        // fallback) don't fail link with `duplicate symbol
        // ___perry_wrap_perry_unknown_func`. Same pattern as the
        // `__perry_wrap_extern_*` wrappers below — comment block at
        // line ~1957 explicitly calls out that wrappers like this
        // should be `internal` linkage so each module gets its own
        // dead-code-eliminable copy.
        wf.linkage = "internal".to_string();
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // f64::from_bits(0x7FFC_0000_0000_0001) — TAG_UNDEFINED. Format
        // identical to how other expr.rs lowerings emit it.
        blk.ret(DOUBLE, "0x7FFC000000000001");
    }

    // Emit ExternFuncRef-as-value wrappers for every imported function in
    // `opts.import_function_prefixes`. Each gets a thin wrapper plus a
    // static `ClosureHeader` so the value can be passed as a callback,
    // stored in a variable, or used in a truthiness / equality check —
    // all the things you can do with a regular closure pointer.
    //
    // The wrappers are `internal` linkage so multiple modules can each
    // emit their own copy without colliding at link time. Dead-code
    // elimination strips wrappers for externs that are never referenced
    // as values.
    //
    // Why this exists: when an imported function appears as a STANDALONE
    // value (`if (this.ffi.setCursors)` capability check, `arr.forEach(
    // importedFn)` callback, or `someFn === otherFn` reference equality),
    // the lowering needs *some* JSValue to thread through. The previous
    // pragmatic fix returned `TAG_TRUE` — correct for truthiness but it
    // would crash at runtime the moment anything called the value via
    // `js_closure_callN` (the runtime would dereference garbage from
    // the function pointer's prefix bytes looking for a `ClosureHeader`).
    // The static-ClosureHeader approach makes those calls actually work:
    // `get_valid_func_ptr` reads `type_tag` at offset 12, sees
    // `CLOSURE_MAGIC = 0x434C4F53 ("CLOS")`, and dispatches to the
    // wrapper, which forwards the args to `perry_fn_<src>__<name>`.
    {
        use std::collections::HashSet;
        let mut emitted_wrappers: HashSet<String> = HashSet::new();
        // Build a quick lookup of imported class names (and their local aliases).
        // Classes have no `perry_fn_<src>__<Class>` symbol — method/constructor/
        // static dispatch happens via separate tables. For these we still need
        // the `__perry_extern_closure_*` global (other code may load it as a
        // value), but the wrapper body must NOT call a missing function: emit
        // a no-op that returns `undefined` so any indirect call through the
        // closure header fails closed instead of failing at link time.
        let mut imported_class_names: HashSet<String> = HashSet::new();
        for ic in &opts.imported_classes {
            imported_class_names.insert(ic.name.clone());
            if let Some(alias) = &ic.local_alias {
                imported_class_names.insert(alias.clone());
            }
        }
        // Stable iteration order for deterministic IR output.
        let mut imports: Vec<(&String, &String)> = opts.import_function_prefixes.iter().collect();
        imports.sort_by(|a, b| a.0.cmp(b.0));
        for (name, source_prefix) in imports {
            let is_class = imported_class_names.contains(name);
            // Issue #678 followup: V8-fallback imports have no native target
            // — the wrapper body cannot call `perry_fn_<src>__<name>` because
            // that symbol doesn't exist. Emit the same no-op wrapper +
            // ClosureHeader as the imported-class branch so direct calls of
            // the function-reference-as-value still link (and fail closed at
            // runtime). Actual call sites — Call/PropertyGet-Call/namespace
            // member call — route through `emit_v8_export_call` and do NOT
            // touch this wrapper.
            let is_v8_import = cross_module
                .import_function_v8_specifiers
                .contains_key(name);
            let wrapper_name = format!("__perry_wrap_extern_{}__{}", source_prefix, name);
            if !emitted_wrappers.insert(wrapper_name.clone()) {
                continue;
            }
            if is_class || is_v8_import {
                // No-op wrapper + a closure header that points at it. The
                // wrapper returns NaN-tagged `undefined` so any indirect call
                // (`MyClass.somethingThatIsActuallyAFn()`) returns undefined.
                // Match the regular wrapper's calling convention — `%this_closure`
                // followed by 6 double params — so direct calls in the IR don't
                // tear off into garbage stack slots.
                let mut wrap_params: Vec<(LlvmType, String)> = Vec::with_capacity(7);
                wrap_params.push((I64, "%this_closure".to_string()));
                for i in 0..6 {
                    wrap_params.push((DOUBLE, format!("%a{}", i)));
                }
                let wf = llmod.define_function(&wrapper_name, DOUBLE, wrap_params);
                wf.linkage = "internal".to_string();
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let undef =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                blk.ret(DOUBLE, &undef);
                let global_name = format!("__perry_extern_closure_{}__{}", source_prefix, name);
                let init = format!("{{ ptr @{}, i32 0, i32 1129074515 }}", wrapper_name);
                llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
                continue;
            }
            // Issue #678: when a re-export rename routes this name to an
            // origin export with a different suffix (`export { default as
            // render }`), call into the origin's real symbol — `perry_fn_<
            // src>__default`, not `perry_fn_<src>__render`. The local
            // wrapper still uses the consumer-visible name so this
            // module's own callers can find it.
            let origin_suffix =
                crate::expr::import_origin_suffix(&cross_module.import_function_origin_names, name);
            let target_name = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
            // Look up the param count from the import metadata. Fall back
            // to 0 if missing — emits a no-arg wrapper, which is wrong
            // for nonzero-arity functions but won't break compilation.
            // (Read from `cross_module.imported_func_param_counts` rather
            // than `opts.imported_func_param_counts` because the latter
            // was moved into `cross_module` earlier in this function.)
            let param_count = cross_module
                .imported_func_param_counts
                .get(name)
                .copied()
                .unwrap_or(0);
            // Make sure the target is declared. The lazy-declares path
            // in `lower_call.rs::ExternFuncRef` only fires when the
            // function is actually CALLED — if it's only referenced as
            // a value, the declare would be missing without this.
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat_n(DOUBLE, param_count).collect();
            llmod.declare_function(&target_name, DOUBLE, &param_types);
            // Wrapper: `define internal double @__perry_wrap_extern_<src>__<name>(
            //              i64 %this_closure, double %a0, …, double %aN-1)`
            // discards the closure pointer and forwards the doubles to
            // `perry_fn_<src>__<name>`.
            let mut wrap_params: Vec<(LlvmType, String)> = Vec::with_capacity(param_count + 1);
            wrap_params.push((I64, "%this_closure".to_string()));
            for i in 0..param_count {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrapper_name, DOUBLE, wrap_params);
            wf.linkage = "internal".to_string();
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let arg_names: Vec<String> = (0..param_count).map(|i| format!("%a{}", i)).collect();
            let call_args: Vec<(LlvmType, &str)> =
                arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = blk.call(DOUBLE, &target_name, &call_args);
            blk.ret(DOUBLE, &result);
            // Static `ClosureHeader` global pointing at the wrapper.
            // Layout matches `crates/perry-runtime/src/closure.rs`:
            //   { *const u8 func_ptr (8 bytes),
            //     u32 capture_count (4 bytes),
            //     u32 type_tag      (4 bytes) }
            // The runtime's `get_valid_func_ptr` reads `type_tag` at
            // offset 12 and validates against `CLOSURE_MAGIC = 0x434C4F53`
            // ("CLOS" in ASCII = 1129074515 decimal). If the magic doesn't
            // match, the call fast-paths to `undefined` instead of
            // dispatching, so any non-closure value passed where a closure
            // is expected fails closed rather than crashing.
            let global_name = format!("__perry_extern_closure_{}__{}", source_prefix, name);
            let init = format!("{{ ptr @{}, i32 0, i32 1129074515 }}", wrapper_name);
            llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
        }
    }

    // Issue #100: emit the per-module `@__perry_ns_<prefix>` global iff
    // this module is the target of at least one dynamic `import()` site
    // anywhere in the program. Defined here with external linkage so the
    // consumer-side `Expr::DynamicImport` lowering (which may live in a
    // different LLVM module) can `load double, ptr @__perry_ns_<prefix>`.
    // Initialized to NaN-boxed `undefined`; the populator at the end of
    // `__perry_init_<prefix>` overwrites this with the real namespace
    // object built via `js_create_namespace`. Registered as a GC root
    // (same as every other module global) so the underlying ObjectHeader
    // survives sweeps after init returns.
    // Pre-emit each export-key string constant at module level. We add
    // them BEFORE compile_module_entry so the populator can reference
    // them by their stable `.str.N` global name. Stored alongside the
    // entries so the populator emit-site can look up `(global_name,
    // byte_len)` per key without rebuilding any LLVM IR. Vec entries
    // are parallel to `cross_module.namespace_entries`.
    let mut namespace_key_globals: Vec<(String, usize)> = Vec::new();
    // Issue #842: also emit `@__perry_ns_<prefix>` for side-effect-only
    // dynamic-import targets (`namespace_entries` empty but the module
    // is still a target). Without this, the consumer-side extern
    // declaration links to nothing. The empty-entries case still emits
    // the global; the populator below handles `n == 0` by calling
    // `js_create_namespace(0, ...)` which returns an empty object.
    if !cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target {
        let ns_name = format!("__perry_ns_{}", module_prefix);
        // Hex double literal for TAG_UNDEFINED (0x7FFC_0000_0000_0001).
        llmod.add_global(&ns_name, DOUBLE, "0x7FFC000000000001");
        for entry in &cross_module.namespace_entries {
            let (gname, byte_len) = llmod.add_string_constant(&entry.name);
            namespace_key_globals.push((gname, byte_len));
        }
    }
    // For each `Expr::DynamicImport` target this module dispatches to,
    // declare the foreign module's `@__perry_ns_<target_prefix>` as an
    // extern global so the dispatch site can load it. Deduped via
    // `BTreeSet` so a multi-path site that resolves to N targets emits
    // N declarations exactly once even when multiple `paths` arrays
    // share entries.
    if !cross_module.dynamic_import_path_to_prefix.is_empty() {
        let mut foreign_prefixes: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for prefix in cross_module.dynamic_import_path_to_prefix.values() {
            if prefix != module_prefix.as_str() {
                foreign_prefixes.insert(prefix.clone());
            }
        }
        for prefix in foreign_prefixes {
            let ns_name = format!("__perry_ns_{}", prefix);
            llmod.add_external_global(&ns_name, DOUBLE);
            // Issue #753: declare each dynamic-import target's `__init`
            // so the dispatch site in `Expr::DynamicImport` can call it
            // before loading the namespace. The wrapper-side init is
            // idempotent — calling it for an already-initialized
            // target costs a load + cmp + cond_br. For Deferred
            // targets it's the only thing that triggers their init.
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }
    }

    // Emit either `int main()` (entry module) or `void <prefix>__init()`
    // (non-entry module). The entry main calls each non-entry init in
    // order before running its own statements.
    compile_module_entry(
        &mut llmod,
        hir,
        &func_names,
        &mut strings,
        &class_table,
        &method_names,
        &module_globals,
        &opts.import_function_prefixes,
        &enum_table,
        &static_field_globals,
        &class_ids,
        &func_signatures,
        &func_synthetic_arguments,
        &module_prefix,
        opts.is_entry_module,
        &opts.non_entry_module_prefixes,
        &module_boxed_vars,
        &closure_rest_params,
        &cross_module,
        &opts.output_type,
        &namespace_key_globals,
    )
    .with_context(|| format!("lowering entry of module '{}'", hir.name))?;

    // Issue #392: pre-intern every user-class method name into the
    // string pool so `emit_string_pool` (which takes `&strings`) can
    // emit the `js_register_class_method(class_id, name_ptr, ...)`
    // calls in module init without needing mutable access to the
    // pool. Only iterate classes DEFINED in this module — the
    // `perry_method_*` symbols for imported classes live in the
    // defining module's object file. `imported_class_prefix` lists
    // every imported class name; we exclude those.
    for class in &hir.classes {
        let cid = class_ids.get(&class.name).copied().unwrap_or(0);
        if cid == 0 {
            continue;
        }
        for method in &class.methods {
            let _ = strings.intern(&method.name);
        }
        // Refs #486: also intern getter property names so the cross-module
        // `js_register_class_getter` registration loop in emit_string_pool
        // can find their bytes_global without re-running through the
        // string pool's mutable interner.
        for (prop, _) in &class.getters {
            let _ = strings.intern(prop);
        }
    }

    // After all user code is lowered, the string pool's contents are final.
    // Emit the bytes globals, handle globals, and the
    // `__perry_init_strings_<prefix>` function that runs once at startup.
    // The function name is scoped by module prefix so multiple modules
    // can each have their own string-pool init without colliding.
    // Issue #653: build wrapper-rest map for top-level user functions.
    // Each `__perry_wrap_<name>` was emitted above; if the underlying
    // function declares a rest param, we tell the runtime to bundle
    // trailing args into an array before invoking the wrapper. Without
    // this, calling a function-as-value via the closure-spread path
    // (`js_closure_call_apply_with_spread`) leaks raw element bits into
    // the rest-param slot.
    let user_fn_wrapper_rest: Vec<(String, usize)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            f.params.iter().position(|p| p.is_rest).and_then(|idx| {
                func_names
                    .get(&f.id)
                    .map(|name| (format!("__perry_wrap_{}", name), idx))
            })
        })
        .collect();

    // Refs #915 (gap 1 from #899): wrappers whose underlying function ends in
    // the HIR-synthesized `arguments` rest param. The wrapper symbol is keyed
    // the same way (`__perry_wrap_<scoped_name>`) but registered with the
    // synthetic-arguments runtime fn so `dispatch_rest_bundled` bundles ALL
    // passed args, matching JS spec semantics for `arguments.length`.
    let user_fn_wrapper_synthetic_arguments: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let last_is_synth_args = f
                .params
                .last()
                .map(|p| p.is_rest && p.name == "arguments")
                .unwrap_or(false);
            if last_is_synth_args {
                func_names
                    .get(&f.id)
                    .map(|name| format!("__perry_wrap_{}", name))
            } else {
                None
            }
        })
        .collect();

    // Wrapper arities — declared param count per top-level user-function
    // wrapper. Used to register `fn.length` for `FuncRef` values that
    // codegen materialises through `__perry_wrap_<name>`. Refs
    // ramda's `converge` / `juxt` / `useWith` chain that reads
    // `.length` off function values to compute curry arities.
    let user_fn_wrapper_arity: Vec<(String, u32)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| (format!("__perry_wrap_{}", name), f.params.len() as u32))
        })
        .collect();

    emit_string_pool(
        &mut llmod,
        &strings,
        &module_prefix,
        &class_keys_init_data,
        &class_ids,
        &class_table,
        &closure_rest_params,
        &closure_arities,
        &user_fn_wrapper_rest,
        &closure_synthetic_arguments,
        &user_fn_wrapper_synthetic_arguments,
        &user_fn_wrapper_arity,
    );

    // Emit the buffer alias-scope metadata once per module, covering every
    // scope id allocated across compile_function / compile_closure /
    // compile_method / compile_static_method / compile_module_entry. Must
    // run AFTER all function compilation so the counter reflects the true
    // total — otherwise functions whose scope ids exceed the init
    // function's count emit `!alias.scope !N` references with no matching
    // metadata definition (issue #71).
    let total_buffer_scopes = llmod.buffer_alias_counter;
    emit_buffer_alias_metadata(&mut llmod, total_buffer_scopes);

    let ll_text = llmod.to_ir();
    log::debug!(
        "perry-codegen: emitted {} bytes of LLVM IR for '{}' ({} interned strings)",
        ll_text.len(),
        hir.name,
        strings.len()
    );
    // Save .ll files when PERRY_SAVE_LL=<dir> is set
    if let Ok(save_dir) = std::env::var("PERRY_SAVE_LL") {
        let filename = format!("{}/{}.ll", save_dir, module_prefix);
        let _ = std::fs::write(&filename, &ll_text);
    }
    if opts.emit_ir_only {
        Ok(ll_text.into_bytes())
    } else {
        crate::linker::compile_ll_to_object(&ll_text, opts.target.as_deref())
    }
}

/// Compile a single user function into the module.
/// Shadow-stack push/pop + slot-set emission for every user
/// function. Default ON as of Phase D part 2 (v0.5.238); set
/// `PERRY_SHADOW_STACK=0`/`off`/`false` to disable for bisection.
/// Cached at first call so subsequent compile_* calls skip the
/// env-var lookup.
///
/// Why on by default now: the shadow stack precisely covers every
/// pointer-typed local in compiled JS frames, complementing the
/// conservative C-stack scan. With Phase A complete and the GC
/// tracer consuming the shadow stack as a parallel root source
/// (v0.5.221), enabling it is a strict-improvement default —
/// fewer over-promoted objects in generational mode, no change
/// in observed correctness, modest per-function-entry overhead
/// (one frame_push call + N slot stores at safepoints) that's
/// invisible on every measured benchmark. Phase D part 2 then
/// uses the shadow stack's authoritative JS-frame coverage to
/// shrink the conservative scanner — which only makes sense once
/// the shadow stack is guaranteed to be live.
fn shadow_stack_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SHADOW_STACK").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Gen-GC Phase C2 emission gate. PERRY_WRITE_BARRIERS=1 / on /
/// true → emit `js_write_barrier(parent_bits, child_bits)` after
/// every heap-store site. Default OFF — barriers cost a function
/// call per store and the runtime entry's old-vs-young range scan
/// is O(blocks). C3 will replace the range scan with a single
/// GC_FLAG_YOUNG bit-test, at which point flipping the default
/// becomes attractive.
pub(crate) fn write_barriers_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

fn compile_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    module_global_types: &HashMap<u32, perry_types::Type>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;

    // Phase A assumes all user-function params are `double`. Parameter
    // registers are named `%arg{LocalId}` so the body can store them into
    // alloca slots keyed by the same HIR LocalId.
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);

    // Gen-GC Phase A sub-phase 3a: opt-in shadow-frame emission
    // for user functions. Pointer-typed param + local slots are
    // assigned pre-lowering via `collect_pointer_typed_locals`;
    // the frame is sized to hold all of them. Sub-phase 3b emits
    // the slot-set calls at Let/LocalSet sites to actually
    // populate the frame with live values; today the slots stay
    // zero (the tracer doesn't consume them yet — Phase A ship
    // criterion is "shadow stack is built but not yet consumed").
    let shadow_slot_map = if shadow_stack_enabled() {
        let m = crate::collectors::collect_pointer_typed_locals(&f.params, &f.body);
        lf.enable_shadow_frame(m.len() as u32);
        m
    } else {
        std::collections::HashMap::new()
    };

    // Small leaf functions (≤ 8 statements) get alwaysinline so LLVM
    // exposes their operations to the caller's optimizer context — critical
    // for vectorizing clamp helpers and similar patterns. Excluded:
    // async/generator functions, AND functions rewritten by the
    // async-to-generator pre-pass (was_plain_async=true). Inlining the
    // rewritten wrapper into its caller breaks GC-root coverage of the
    // step closure's iter capture, hanging async chains (issue #447).
    if f.body.len() <= 8 && !f.is_async && !f.is_generator && !f.was_plain_async {
        lf.force_inline = true;
    }
    let _ = lf.create_block("entry");

    // Store each param into an alloca slot, collecting LocalId → slot
    // mappings. We release the &mut LlBlock at scope end before handing
    // the function over to the FnCtx lowering pass.
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    // Param types feed local_types so type-aware dispatch (e.g. string
    // concat detection on a `: string` parameter) works inside the body.
    // Also seed with module global types so functions that access module
    // globals see the correct declared types (e.g., Named("Editor")).
    let mut local_types: HashMap<u32, perry_types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &f.params {
        local_types.insert(p.id, p.ty.clone());
    }

    // Pre-walk: which locals need to be boxed? A local is boxed when
    // it's captured by a closure AND written by someone (either the
    // enclosing function or inside a closure). Box-backing lets multiple
    // closures share the same mutable cell — critical for the common
    // `let x = 0; return { get: () => x, set: (n) => x = n }` pattern.
    let boxed_vars = module_boxed_vars.clone();

    // Pre-walk: which locals are provably integer-valued? Used by
    // `BinaryOp::Mod` to emit integer modulo instead of libm `fmod()`.
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let integer_locals = crate::collectors::collect_integer_locals(
        &f.body,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );
    // Issue #140 gate: locals that appear in an `arr[i]` / `uint8[i]` / `arr.at(i)`
    // index subtree. Pure accumulators skip the Let-site i32 shadow so the body
    // stays a single-f64-alloca chain that LLVM's autovectorizer can widen.
    let index_used_locals = crate::collectors::collect_index_used_locals(&f.body);
    // Issue #436: locals whose every write has a strictly-i32-bounded rhs
    // (bitwise / `|0` / `>>>0` / Buffer-byte / MathImul / returns_int call,
    // but NOT bare Add/Sub/Mul of int-stable). Used at the Let-site i32
    // gate alongside `index_used_locals` so accumulators like image_conv's
    // FNV-1a `h` (writes `(h^dst[i])|0` and `imul32(h,K)`) get the i32
    // fast path even without index use, while #435's overflow-prone
    // `sum += compute(i)` accumulators stay out (the bare Add is
    // explicitly excluded).
    let strictly_i32_bounded_locals = crate::collectors::collect_strictly_i32_bounded_locals(
        &f.body,
        &integer_locals,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );

    // Pre-walk: which `let x = new Class(...)` locals never escape?
    let non_escaping_news =
        crate::collectors::collect_non_escaping_news(&f.body, &boxed_vars, module_globals, classes);
    let non_escaping_arrays =
        crate::collectors::collect_non_escaping_arrays(&f.body, &boxed_vars, module_globals);
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body,
        &boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: Vec::new(),
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &integer_locals,
        shadow_slot_map,
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        i32_counter_slots: HashMap::new(),
        index_used_locals: &index_used_locals,
        strictly_i32_bounded_locals: &strictly_i32_bounded_locals,
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news,
        non_escaping_arrays,
        non_escaping_object_literals,
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        was_unrolled: f.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_alias_base,
    };

    // Issue #92 follow-up: pre-register `buffer_data_slots` entries for
    // `Buffer`-typed function parameters so that the readInt32BE/etc.
    // intrinsic fast path in `lower_call.rs` fires on
    // `function decode(row: Buffer) { row.readInt32BE(off) }` — the real
    // Postgres-driver hot-path shape, not just the `const buf = Buffer.alloc(N)`
    // micro-benchmark. Skipped when the param is reassigned (has_any_mutation
    // covers LocalSet/Update/ARRAY_MUTATORS — `buf = ...`, `buf.fill(...)` etc.)
    // because a cached data_ptr would go stale, and skipped for boxed params
    // (same reason via cross-closure mutation). Uint8Array-typed params are
    // deliberately excluded: a pre-existing crash surfaces when the same
    // program defines both a Buffer-param and a Uint8Array-param function and
    // then invokes them in sequence (reproducible on main without any of
    // this extension's changes). Tracked separately; Buffer coverage alone
    // hits the Postgres decode path which is the target workload here.
    for p in &f.params {
        let is_buffer_typed = matches!(
            &p.ty,
            perry_types::Type::Named(n) if n == "Buffer"
        );
        if !is_buffer_typed {
            continue;
        }
        if ctx.boxed_vars.contains(&p.id) {
            continue;
        }
        if crate::collectors::has_any_mutation(&f.body, p.id) {
            continue;
        }
        let Some(param_slot) = ctx.locals.get(&p.id).cloned() else {
            continue;
        };
        let blk = ctx.block();
        let arg_val = blk.load(DOUBLE, &param_slot);
        let handle = crate::expr::unbox_to_i64(blk, &arg_val);
        let handle_ptr = blk.inttoptr(I64, &handle);
        let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
        let buf_slot = ctx.func.alloca_entry(PTR);
        ctx.block().store(PTR, &data_ptr, &buf_slot);
        let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
        ctx.buffer_data_slots.insert(p.id, (buf_slot, scope_idx));
    }

    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of '{}'", f.name))?;

    // A function that falls off the end without an explicit `return`
    // returns `undefined` in JS — emit the NaN-boxed TAG_UNDEFINED
    // value so the LLVM verifier has a terminator AND user code that
    // does `f() === undefined` / `f() !== undefined` observes the
    // correct value. For async functions, wrap undefined in a
    // resolved promise so callers can await the result.
    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if f.is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &undef);
        }
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    drop(ctx); // releases &mut LlFunction borrow on llmod
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}

/// Compile a closure body as a top-level LLVM function.
///
/// Signature: `double perry_closure_<modprefix>__<func_id>(i64 this_closure,
/// double arg0, double arg1, …)`. The first parameter is the closure
/// pointer (raw i64); the remaining params are the closure's own
/// declared parameters.
///
/// Inside the body, captured variables (`closure.captures`) are mapped
/// to capture indices and accessed via the runtime
/// `js_closure_get/set_capture_f64(this_closure, idx)` calls. The
/// `closure_captures` field on `FnCtx` carries the LocalId → capture
/// index map; `current_closure_ptr` carries the closure pointer SSA
/// value name.
#[allow(clippy::too_many_arguments)]
fn compile_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    module_local_types: &HashMap<u32, perry_types::Type>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    // Destructure the closure expression. We trust that the caller
    // passes only `Expr::Closure` here (from `collect_closures_*`).
    let (params, body, captures, captures_this, enclosing_class, is_async) = match closure_expr {
        perry_hir::Expr::Closure {
            params,
            body,
            captures,
            captures_this,
            enclosing_class,
            is_async,
            ..
        } => (
            params,
            body,
            captures,
            *captures_this,
            enclosing_class.clone(),
            *is_async,
        ),
        _ => return Err(anyhow!("compile_closure: expected Expr::Closure")),
    };

    let llvm_name = format!("perry_closure_{}__{}", module_prefix, func_id);

    // Param list: i64 this_closure, then each param as double.
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    for p in params {
        llvm_params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, llvm_params);
    let _ = lf.create_block("entry");

    // Allocate slots for the closure's own params (captures don't get
    // alloca slots — they're accessed via the runtime).
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    // Start with the closure's own params as local_types, then
    // merge in the module-wide map so captured-from-outer ids have
    // their types available inside the body. Without this, closures
    // that capture an array `items` and do `items.length` miss the
    // typed fast path and return undefined.
    let mut local_types: HashMap<u32, perry_types::Type> =
        params.iter().map(|p| (p.id, p.ty.clone())).collect();
    for (id, ty) in module_local_types.iter() {
        local_types.entry(*id).or_insert_with(|| ty.clone());
    }

    // Build the capture map: each captured LocalId gets the index it
    // occupies in the closure's capture array. Identical logic to the
    // `compute_auto_captures` helper used by the closure creation site
    // — they MUST agree on the slot indices, otherwise the body reads
    // captures from the wrong slots. Sorting the auto-detected ids
    // gives deterministic indexing across both call sites.
    //
    // Filter module globals out of the explicit captures list — same
    // reason as in `compute_auto_captures` (closures auto-load module
    // globals through `@perry_global_*`). Without this, the body and
    // creation sites disagree on capture indices and a globalized
    // block-scoped let captured by a closure ends up with a
    // value-instead-of-box-pointer in its capture slot.
    let mut auto_captures: Vec<u32> = captures
        .iter()
        .copied()
        .filter(|id| !module_globals.contains_key(id))
        .collect();
    {
        let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut referenced);
        let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_let_ids(body, &mut inner_lets);
        let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        let already: std::collections::HashSet<u32> = auto_captures.iter().copied().collect();
        let mut sorted: Vec<u32> = referenced.into_iter().collect();
        sorted.sort();
        for id in sorted {
            if !param_ids.contains(&id)
                && !inner_lets.contains(&id)
                && !already.contains(&id)
                && !module_globals.contains_key(&id)
            {
                auto_captures.push(id);
            }
        }
    }
    let closure_captures: HashMap<u32, u32> = auto_captures
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();

    // `this` capture. Object-literal methods get `captures_this=true`
    // AND the creation site (lower_object_literal) patches a reserved
    // capture slot at index `auto_captures.len()` with the containing
    // object pointer. At function entry we read that slot and store it
    // into the `this` alloca so `Expr::This` loads the real receiver.
    //
    // Arrow-in-class leftover path (`enclosing_class.is_some()` without
    // the object-literal patch) keeps the old 0.0 sentinel — reads
    // return a bogus value but don't crash.
    let this_stack = if captures_this || enclosing_class.is_some() {
        let this_cap_idx = auto_captures.len() as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        if captures_this {
            let idx_str = this_cap_idx.to_string();
            let v = blk.call(
                DOUBLE,
                "js_closure_get_capture_f64",
                &[(I64, "%this_closure"), (I32, &idx_str)],
            );
            blk.store(DOUBLE, &v, &slot);
        } else {
            blk.store(DOUBLE, "0.0", &slot);
        }
        vec![slot]
    } else {
        Vec::new()
    };
    let class_stack = match enclosing_class.clone() {
        Some(c) => vec![c],
        None => Vec::new(),
    };

    // Boxed vars inside the closure body: mutable captures from the
    // closure's own let-bindings. We don't add the captured-from-outer
    // ids here because those are already boxed in the outer function;
    // the closure body just sees them via the capture mechanism.
    let closure_boxed_vars = module_boxed_vars.clone();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let integer_locals = crate::collectors::collect_integer_locals(
        body,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );
    let index_used_locals = crate::collectors::collect_index_used_locals(body);
    let strictly_i32_bounded_locals = crate::collectors::collect_strictly_i32_bounded_locals(
        body,
        &integer_locals,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );

    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        body,
        &closure_boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_arrays =
        crate::collectors::collect_non_escaping_arrays(body, &closure_boxed_vars, module_globals);
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        body,
        &closure_boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack,
        class_stack,
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures,
        current_closure_ptr: Some("%this_closure".to_string()),
        enums,
        // Async closures (arrow functions declared `async () => ...`)
        // must wrap their return values in `js_promise_resolved` so the
        // call site sees a NaN-boxed Promise pointer — same contract as
        // regular async functions. Consumers like the Fastify server
        // runtime inspect the returned value with `js_is_promise` and
        // break if a raw object pointer (or any non-Promise) is handed
        // back. Issue #125.
        is_async_fn: is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: closure_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &integer_locals,
        shadow_slot_map: std::collections::HashMap::new(),
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        i32_counter_slots: HashMap::new(),
        index_used_locals: &index_used_locals,
        strictly_i32_bounded_locals: &strictly_i32_bounded_locals,
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news,
        non_escaping_arrays,
        non_escaping_object_literals,
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        was_unrolled: false,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_alias_base,
    };

    stmt::lower_stmts(&mut ctx, body)
        .with_context(|| format!("lowering closure body func_id={}", func_id))?;

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &undef);
        }
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    drop(ctx);
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}

/// Compile a class instance method as a top-level LLVM function with the
/// signature `perry_method_<class>_<name>(this_box: double, args: double…)
/// -> double`. The first parameter (`this`) is stored in a slot whose
/// pointer is pushed onto `this_stack`, then `class_stack` is set so
/// inner `Expr::This` and `super` work correctly.
fn compile_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    module_global_types: &HashMap<u32, perry_types::Type>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;

    // Build the param list: (this, arg0, arg1, ...). All are doubles.
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    // Allocate slots for `this` and each parameter; pre-populate with
    // the incoming values.
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "%this_arg", &this_slot);
        let mut map = HashMap::new();
        for p in &method.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        (this_slot, map)
    };

    let mut local_types: HashMap<u32, perry_types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &method.params {
        local_types.insert(p.id, p.ty.clone());
    }

    let method_boxed_vars = module_boxed_vars.clone();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let integer_locals = crate::collectors::collect_integer_locals(
        &method.body,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );
    let index_used_locals = crate::collectors::collect_index_used_locals(&method.body);
    let strictly_i32_bounded_locals = crate::collectors::collect_strictly_i32_bounded_locals(
        &method.body,
        &integer_locals,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );

    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &method.body,
        &method_boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        &method.body,
        &method_boxed_vars,
        module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &method.body,
        &method_boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: vec![this_slot],
        class_stack: vec![class.name.clone()],
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: method.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: method_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &integer_locals,
        shadow_slot_map: std::collections::HashMap::new(),
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        i32_counter_slots: HashMap::new(),
        index_used_locals: &index_used_locals,
        strictly_i32_bounded_locals: &strictly_i32_bounded_locals,
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news,
        non_escaping_arrays,
        non_escaping_object_literals,
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        was_unrolled: method.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_alias_base,
    };

    // Constructors emitted as standalone cross-module LLVM functions (named
    // `<prefix>__<class>_constructor`) must bake the field initializers into
    // their body. At the `new ImportedClass(...)` call site, `lower_new`
    // applies initializers against the imported class stub — which has none
    // — so without this, imported classes construct with all fields left
    // as uninitialized register values (read as NaN-boxed undefined).
    let is_constructor_method = method.name == format!("{}_constructor", class.name);
    if is_constructor_method {
        // Stage field initializers around the parent body chain so leaf
        // fields can read state set by parent body (Refs #420):
        //   - has extends: apply only ancestors here; self-fields apply
        //     later (after super() in own-body case, after explicit parent
        //     ctor call in no-own-body case).
        //   - no extends: apply all (= just self) here.
        let init_mode = if class.extends_name.is_some() {
            crate::lower_call::FieldInitMode::AncestorsOnly
        } else {
            crate::lower_call::FieldInitMode::All
        };
        crate::lower_call::apply_field_initializers_recursive(&mut ctx, &class.name, init_mode)
            .with_context(|| {
                format!(
                    "applying field initializers for '{}' constructor",
                    class.name
                )
            })?;
        // Refs #420: when a class has no own constructor but extends a parent
        // that DOES have a body, JS spec requires a default ctor that calls
        // `super(...args)` — implicit forward. perry's standalone ctor for
        // such a class previously emitted only field initializers, so the
        // parent's ctor body (e.g. ColumnBuilder's `this.config = {...}`)
        // never ran when called via the cross-module dispatch path. Inject a
        // call to the parent's standalone ctor symbol here, forwarding all
        // args. The walk skips empty-bodied parents (matching the JS spec
        // chain semantics).
        if class.constructor.is_none() && class.extends_name.is_some() {
            let mut effective_parent: Option<&str> = class.extends_name.as_deref();
            while let Some(pname) = effective_parent {
                let Some(pc) = ctx.classes.get(pname).copied() else {
                    break;
                };
                let has_local_body = pc.constructor.is_some();
                let has_imported_ctor = ctx.imported_class_ctors.contains_key(pname);
                if has_local_body || has_imported_ctor {
                    break;
                }
                effective_parent = pc.extends_name.as_deref();
            }
            if let Some(pname) = effective_parent {
                let pname_owned = pname.to_string();
                // Resolve the standalone-ctor symbol name. Prefer the
                // local class table (same module) for an inline call;
                // fall back to imported_class_ctors for cross-module.
                let (ctor_sym, param_count) = if let Some(pclass) =
                    ctx.classes.get(&pname_owned).copied()
                {
                    if pclass.constructor.is_some() {
                        // Local class with own ctor — use the per-module-prefix
                        // standalone symbol, same one compile_method emits.
                        let module_prefix = ctx.strings.module_prefix().to_string();
                        let sym = format!("{}__{}_constructor", module_prefix, pname_owned);
                        let pcount = pclass
                            .constructor
                            .as_ref()
                            .map(|c| c.params.len())
                            .unwrap_or(0);
                        (sym, pcount)
                    } else if let Some((sym, n)) =
                        ctx.imported_class_ctors.get(&pname_owned).cloned()
                    {
                        (sym, n)
                    } else {
                        // No callable ctor symbol — bail.
                        stmt::lower_stmts(&mut ctx, &method.body).with_context(|| {
                            format!("lowering body of method '{}::{}'", class.name, method.name)
                        })?;
                        // Fall through to the default ret at end.
                        if !ctx.block().is_terminated() {
                            let undef = crate::nanbox::double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            ));
                            ctx.block().ret(DOUBLE, &undef);
                        }
                        let _ = std::mem::take(&mut ctx.ic_globals);
                        let _ = std::mem::take(&mut ctx.typed_parse_rodata);
                        let _ = std::mem::take(&mut ctx.pending_declares);
                        return Ok(());
                    }
                } else if let Some((sym, n)) = ctx.imported_class_ctors.get(&pname_owned).cloned() {
                    (sym, n)
                } else {
                    ("".to_string(), 0)
                };
                if !ctor_sym.is_empty() {
                    let undef_lit =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    // Forward this method's params, padding with undefined if
                    // the parent expects more.
                    let mut forwarded: Vec<String> = Vec::with_capacity(param_count);
                    for (i, p) in method.params.iter().enumerate() {
                        if i >= param_count {
                            break;
                        }
                        let slot = ctx.locals.get(&p.id).cloned();
                        if let Some(slot) = slot {
                            forwarded.push(ctx.block().load(DOUBLE, &slot));
                        } else {
                            forwarded.push(undef_lit.clone());
                        }
                    }
                    while forwarded.len() < param_count {
                        forwarded.push(undef_lit.clone());
                    }
                    // Load `this` from the this_stack.
                    let this_slot = ctx.this_stack.last().cloned();
                    let this_box = if let Some(slot) = this_slot {
                        ctx.block().load(DOUBLE, &slot)
                    } else {
                        undef_lit.clone()
                    };
                    let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                        .chain(forwarded.iter().map(|_| DOUBLE))
                        .collect();
                    let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                        Vec::with_capacity(1 + forwarded.len());
                    ctor_args.push((DOUBLE, &this_box));
                    for la in &forwarded {
                        ctor_args.push((DOUBLE, la.as_str()));
                    }
                    ctx.pending_declares.push((
                        ctor_sym.clone(),
                        crate::types::VOID,
                        ctor_param_types,
                    ));
                    ctx.block().call_void(&ctor_sym, &ctor_args);
                }
            }
            // Apply self field initializers AFTER the parent body chain has
            // run, so they can read state set by the parent body (e.g. drizzle's
            // PgText.enumValues = this.config.enumValues — this.config is set
            // in Column body via super-chain). Refs #420.
            crate::lower_call::apply_field_initializers_recursive(
                &mut ctx,
                &class.name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )
            .with_context(|| {
                format!(
                    "applying self field initializers for '{}' constructor",
                    class.name
                )
            })?;
        }
    }

    stmt::lower_stmts(&mut ctx, &method.body)
        .with_context(|| format!("lowering body of method '{}::{}'", class.name, method.name))?;

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.block().ret(DOUBLE, &undef);
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    drop(ctx);
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}

/// Emit the module's entry function.
///
/// For the **entry module**: emits `int main()` that bootstraps GC, runs
/// the entry module's own string pool init, then calls every non-entry
/// module's `<prefix>__init` function in order, then runs the entry
/// module's top-level statements, then `return 0`.
///
/// For **non-entry modules**: emits `void <prefix>__init()` that runs the
/// non-entry module's string pool init followed by its top-level
/// statements. The entry module's main calls these via the
/// `non_entry_module_prefixes` list.
///
/// Each module gets its OWN string pool init function
/// (`__perry_init_strings_<prefix>`) so multiple modules in the same
/// program don't collide on the symbol name.
#[allow(clippy::too_many_arguments)]
fn compile_module_entry(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    is_entry: bool,
    non_entry_module_prefixes: &[String],
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    output_type: &str,
    // Issue #100: parallel-to-`cross_module.namespace_entries` list of
    // `(string_constant_global_name, byte_len)` for each export-key.
    // The populator emits one `getelementptr` per key into the stack
    // keys array — `byte_len` becomes the corresponding entry in the
    // key-lengths array passed to `js_create_namespace`. Empty when
    // this module is not a dynamic-import target.
    namespace_key_globals: &[(String, usize)],
) -> Result<()> {
    let strings_init_name = format!("__perry_init_strings_{}", module_prefix);

    let is_dylib = output_type == "dylib";

    if is_entry {
        // Pre-declare each non-entry module's init function as an
        // extern so the entry main can call them. The actual definition
        // lives in the OTHER module's compiled .o file; the linker
        // resolves the symbols at link time.
        for prefix in non_entry_module_prefixes {
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }
        // Issue #753: emit a no-op `<entry_prefix>__init` stub so the
        // dispatch site in some other module that does `await
        // import("./entry.ts")` resolves at link time. The entry
        // module's actual body runs in `main`, not in a separate
        // `__init` — the stub exists purely to satisfy the dispatch's
        // unconditional init call. The namespace populator at the
        // tail of `main` (when `cross_module.namespace_entries` is
        // non-empty) is what makes the entry observable through the
        // dynamic-import namespace; the stub does no work.
        {
            let stub_name = format!("{}__init", module_prefix);
            let stub = llmod.define_function(&stub_name, VOID, vec![]);
            let _ = stub.create_block("entry");
            stub.block_mut(0).unwrap().ret_void();
        }

        // For dylib output, emit `void perry_module_init()` instead of
        // `int main()`. The host process calls this once after dlopen to
        // initialize the GC, string pools, module globals (including GC
        // root registration), and run top-level statements. Without this,
        // module-level Maps/Arrays would never be registered as GC roots
        // and the first GC cycle after connect() would free them (issue #54).
        let ic_base = llmod.ic_counter;
        let buffer_alias_base = llmod.buffer_alias_counter;
        // Declare `perry_geisterhand_start` BEFORE `main` is created — once
        // `main` holds a mutable borrow on `llmod`, no further
        // `llmod.declare_function` calls are allowed. Inline (not in
        // `runtime_decls`) because most builds don't link geisterhand.
        if cross_module.needs_geisterhand && !is_dylib {
            llmod.declare_function("perry_geisterhand_start", VOID, &[I32]);
        }
        let main = if is_dylib {
            llmod.define_function("perry_module_init", VOID, vec![])
        } else {
            llmod.define_function("main", I32, vec![])
        };
        let _ = main.create_block("entry");
        {
            let blk = main.block_mut(0).unwrap();
            blk.call_void("js_gc_init", &[]);
            // Wire up stdlib HANDLE_METHOD_DISPATCH eagerly when stdlib is
            // linked. Previously this was only called from
            // `ensure_pump_registered`, which fires lazily on the first
            // deferred-promise resolution — so sync-only programs (e.g.
            // pure crypto/hash pipelines — issue #86) never registered
            // the dispatcher and handle-based method calls fell through
            // to `js_native_call_method` which returned a non-Perry NaN
            // (`typeof === 'number'`). Guarded on `needs_stdlib` because
            // the runtime-only link doesn't pull in the stub symbol.
            if cross_module.needs_stdlib {
                blk.call_void("js_stdlib_init_dispatch", &[]);
            }
            // Issue #248 Phase 2: bootstrap the V8/perry-jsruntime runtime
            // before any `js_load_module` / `js_call_function` site fires.
            // Without this, the runtime's per-thread JsRuntimeState is None
            // and every interop FFI bails with `[js_load_module] no JS
            // runtime state!` while silently returning undefined. Gated on
            // `needs_js_runtime` so non-interop builds don't pull in the
            // V8 init cost.
            if cross_module.needs_js_runtime {
                blk.call_void("js_runtime_init", &[]);
            }
            // Start the Geisterhand HTTP inspector if requested. The
            // port comes from `--geisterhand-port` (default 7676). Calling
            // `perry_geisterhand_start` here also pins the geisterhand
            // server module against macOS's lazy-load `-dead_strip`, so
            // the inspector_ui HTML embedded via `include_str!` makes it
            // into the final binary instead of being eliminated as
            // unreferenced rodata.
        }
        if !is_dylib && cross_module.needs_geisterhand {
            // Function was declared above (before `main` claimed
            // `&mut llmod`). Lifetime: `port_str` lives for the body of
            // this block, long enough for `call_void` to consume the
            // `&str` reference.
            let port_str = cross_module.geisterhand_port.to_string();
            let blk = main.block_mut(0).unwrap();
            blk.call_void("perry_geisterhand_start", &[(I32, port_str.as_str())]);
        }
        {
            let blk = main.block_mut(0).unwrap();
            // Entry module's own string pool first.
            blk.call_void(&strings_init_name, &[]);
            // Then every non-entry module's init in order. Each
            // non-entry module's `<prefix>__init` runs its own string
            // pool init internally before its top-level statements.
            //
            // Issue #753: skip Deferred modules — those reached only
            // through dynamic `import()` edges. Their `<prefix>__init`
            // fires lazily from each `Expr::DynamicImport` dispatch
            // site, idempotently guarded by `@__perry_init_done_<prefix>`
            // so a program that never reaches the dispatch never pays
            // the startup cost. The extern declaration at line ~3947
            // still emits for every non-entry prefix so the dispatch
            // site can resolve the symbol at link time.
            for prefix in non_entry_module_prefixes {
                if cross_module.deferred_module_prefixes.contains(prefix) {
                    continue;
                }
                blk.call_void(&format!("{}__init", prefix), &[]);
            }
        }
        // Mark the boundary between init prelude and user code so
        // hoisted post-init setup (cached `@perry_class_keys_*` loads
        // for the inline allocator) is spliced AFTER the init calls.
        // Without this, the load reads the global before
        // `__perry_init_strings_*` populates it — `keys_array` is null
        // on every freshly allocated object and field-by-name lookup
        // returns undefined.
        main.mark_entry_init_boundary();

        let main_boxed_vars = module_boxed_vars.clone();
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
            .clamp3_functions
            .union(&cross_module.clamp_u8_functions)
            .chain(cross_module.returns_int_functions.iter())
            .copied()
            .collect();
        let main_integer_locals = crate::collectors::collect_integer_locals(
            &hir.init,
            &cross_module.flat_const_arrays.keys().copied().collect(),
            &clamp_fn_ids,
        );
        let main_index_used_locals = crate::collectors::collect_index_used_locals(&hir.init);
        let main_strictly_i32_bounded_locals =
            crate::collectors::collect_strictly_i32_bounded_locals(
                &hir.init,
                &main_integer_locals,
                &cross_module.flat_const_arrays.keys().copied().collect(),
                &clamp_fn_ids,
            );
        let main_non_escaping_news = crate::collectors::collect_non_escaping_news(
            &hir.init,
            &main_boxed_vars,
            module_globals,
            classes,
        );
        let main_non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
            &hir.init,
            &main_boxed_vars,
            module_globals,
        );
        let main_non_escaping_object_literals =
            crate::collectors::collect_non_escaping_object_literals(
                &hir.init,
                &main_boxed_vars,
                module_globals,
            );
        let mut init_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
        crate::boxed_vars::collect_let_types_in_stmts(&hir.init, &mut init_local_types);
        let mut ctx = FnCtx {
            func: main,
            locals: HashMap::new(),
            local_types: init_local_types,
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_label: None,
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            import_function_origin_names: &cross_module.import_function_origin_names,
            import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
            // Issue #841: node:submodule named-import + namespace registries.
            import_function_node_submodule: &cross_module.import_function_node_submodule,
            namespace_node_submodules: &cross_module.namespace_node_submodules,
            namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            func_synthetic_arguments,
            func_returns_class: &cross_module.func_returns_class,
            boxed_vars: main_boxed_vars,
            prealloc_boxes: std::collections::HashSet::new(),
            closure_rest_params: closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
            namespace_member_prefixes: &cross_module.namespace_member_prefixes,
            imported_async_funcs: &cross_module.imported_async_funcs,
            local_async_funcs: &cross_module.local_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_has_rest: &cross_module.imported_func_has_rest,
            method_param_counts: &cross_module.method_param_counts,
            method_has_rest: &cross_module.method_has_rest,
            imported_func_return_types: &cross_module.imported_func_return_types,
            ffi_signatures: &cross_module.ffi_signatures,
            imported_class_sources: &cross_module.imported_class_sources,
            interfaces: &cross_module.interfaces,
            try_depth: 0,
            pending_declares: Vec::new(),
            integer_locals: &main_integer_locals,
            shadow_slot_map: std::collections::HashMap::new(),
            arena_state_slot: None,
            class_keys_slots: HashMap::new(),
            cached_lengths: HashMap::new(),
            bounded_index_pairs: Vec::new(),
            i32_counter_slots: HashMap::new(),
            index_used_locals: &main_index_used_locals,
            strictly_i32_bounded_locals: &main_strictly_i32_bounded_locals,
            i18n: &cross_module.i18n,
            dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
            local_class_aliases: HashMap::new(),
            local_class_field_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: &cross_module.compile_time_constants,
            app_metadata: &cross_module.app_metadata,
            scalar_replaced: std::collections::HashMap::new(),
            scalar_replaced_arrays: std::collections::HashMap::new(),
            scalar_ctor_target: Vec::new(),
            non_escaping_news: main_non_escaping_news,
            non_escaping_arrays: main_non_escaping_arrays,
            non_escaping_object_literals: main_non_escaping_object_literals,
            flat_const_arrays: &cross_module.flat_const_arrays,
            array_row_aliases: HashMap::new(),
            clamp3_functions: &cross_module.clamp3_functions,
            clamp_u8_functions: &cross_module.clamp_u8_functions,
            was_unrolled: hir.init_was_unrolled,
            ic_site_counter: ic_base,
            ic_globals: Vec::new(),
            typed_parse_rodata: Vec::new(),
            typed_parse_counter: 0,
            buffer_data_slots: HashMap::new(),
            buffer_alias_base,
        };
        // Register every module-level global's ADDRESS as a GC root so
        // the mark phase can discover pointer-typed values (Maps, Arrays,
        // user class instances) stored in them. Without this, a Map
        // held only in a module `const CACHE = new Map<...>()` would be
        // freed by the next GC cycle because the conservative stack
        // scan can't see the global's address — only `js_gc_register_global_root`
        // populates `GLOBAL_ROOTS`, which `mark_global_roots` scans.
        // Closes issue #36 (pg driver's CONN_STATES Map crash after bulk
        // decode crossed the malloc-count GC threshold). Safe to register
        // number-valued globals too — `try_mark_value` + the raw-pointer
        // fallback both validate against the known-heap-pointer set and
        // discard non-matching bits.
        register_module_globals_as_gc_roots(&mut ctx, module_globals);
        // Initialize static class fields with their declared init
        // expressions. Runs once at the top of main, before user code.
        //
        // Split into two phases (#894): early emits the bits that don't
        // read user-let values (Error-extending class registry, well-
        // known symbol method hooks); late runs AFTER user init so
        // computed-Symbol-key static fields whose key/init reference
        // module-level lets see populated slots.
        init_static_fields_early(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of module '{}'", hir.name))?;
        init_static_fields_late(&mut ctx, hir)?;

        // Issue #100: populate `@__perry_ns_<module_prefix>` from the
        // namespace_entries list AFTER user init has run (so every
        // local export's binding is set) and BEFORE the event-loop
        // bootstrap (so the namespace is observable to any consumer
        // who dispatches `await import("./this_module.ts")` during
        // event-loop turns). For the entry-module case this is the
        // unusual scenario where some other module dynamic-imports
        // the entry itself — uncommon but supported.
        // Issue #842: also run the populator for side-effect-only
        // dynamic-import targets (`namespace_entries` empty but module
        // is a target). The populator emits `js_create_namespace(0, ...)`
        // → an empty NaN-boxed object → stored into `@__perry_ns_<prefix>`,
        // satisfying the consumer-side extern reference.
        if (!cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target)
            && !ctx.block().is_terminated()
        {
            emit_namespace_populator(
                &mut ctx,
                &cross_module.namespace_entries,
                namespace_key_globals,
                module_prefix,
            );
        }

        if !ctx.block().is_terminated() {
            if is_dylib {
                // Dylib: no event loop — the host manages its own event
                // loop and calls perry_fn_* entry points as needed. Just
                // return after running top-level statements (which set up
                // module-level state like Maps, class registrations, etc.).
                ctx.block().ret_void();
            } else {
                // Event loop: keep running while there are active event
                // sources (timers, intervals, WS servers, pending stdlib
                // async ops). Without this, event-driven servers (WS,
                // setInterval-based) exit immediately after init.
                //
                // Structure:
                //   loop_header: check if any source is active → body or exit
                //   loop_body:   tick all queues, sleep 10ms, jump to header
                //   loop_exit:   ret 0
                let header_idx = ctx.new_block("event_loop.header");
                let body_idx = ctx.new_block("event_loop.body");
                let exit_idx = ctx.new_block("event_loop.exit");
                let header_label = ctx.block_label(header_idx);
                let body_label = ctx.block_label(body_idx);
                let exit_label = ctx.block_label(exit_idx);

                // Initial microtask flush (4 rounds) before entering the
                // event loop — handles fire-and-forget .then() chains that
                // don't need the full event loop.
                for _ in 0..4 {
                    let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                    let _ = ctx.block().call(I32, "js_timer_tick", &[]);
                    let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                    let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
                }
                ctx.block().call_void("js_run_stdlib_pump", &[]);
                ctx.block().call_void("js_run_jsruntime_pump", &[]);
                ctx.block().br(&header_label);

                // loop_header: check if there's any reason to keep running
                ctx.current_block = header_idx;
                let has_timers = ctx.block().call(I32, "js_timer_has_pending", &[]);
                let has_callbacks = ctx.block().call(I32, "js_callback_timer_has_pending", &[]);
                let has_intervals = ctx.block().call(I32, "js_interval_timer_has_pending", &[]);
                let has_stdlib = ctx.block().call(I32, "js_stdlib_has_active_handles", &[]);
                let has_jsruntime = ctx
                    .block()
                    .call(I32, "js_jsruntime_has_active_handles", &[]);
                // #591: TASK_QUEUE may carry a pending `.then` continuation
                // that was queued by `js_run_stdlib_pump`'s resolution path
                // in the SAME body iteration that already drained the inflight
                // counter and PENDING_RESOLUTIONS to zero. Without this gate,
                // the header check would flip to "exit" before the next body's
                // microtask drain ran the continuation.
                let has_microtasks = ctx.block().call(I32, "js_microtasks_pending", &[]);
                let any1 = ctx.block().or(I32, &has_timers, &has_callbacks);
                let any2 = ctx.block().or(I32, &has_intervals, &has_stdlib);
                let any_js = ctx.block().or(I32, &any2, &has_jsruntime);
                let any3 = ctx.block().or(I32, &any1, &any_js);
                let any = ctx.block().or(I32, &any3, &has_microtasks);
                let zero = "0".to_string();
                let cmp = ctx.block().icmp_ne(I32, &any, &zero);
                ctx.block().cond_br(&cmp, &body_label, &exit_label);

                // loop_body: tick everything, sleep, loop
                ctx.current_block = body_idx;
                let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                let _ = ctx.block().call(I32, "js_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
                ctx.block().call_void("js_run_stdlib_pump", &[]);
                ctx.block().call_void("js_run_jsruntime_pump", &[]);
                // Issue #84: condvar-backed wait. Returns immediately when
                // a tokio worker (net/ws/http/fetch/redis/spawn) notifies
                // after pushing to its queue; otherwise blocks until the
                // next timer/interval deadline or a 1 s safety cap.
                ctx.block().call_void("js_wait_for_event", &[]);
                ctx.block().br(&header_label);

                // loop_exit: done
                ctx.current_block = exit_idx;
                ctx.block().ret(I32, "0");
            }
        }
        let ic_globals = std::mem::take(&mut ctx.ic_globals);
        let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
        let ic_end = ctx.ic_site_counter;
        let pending = std::mem::take(&mut ctx.pending_declares);
        let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
        drop(ctx);
        llmod.ic_counter = ic_end;
        llmod.buffer_alias_counter += buffer_alias_used;
        for (name, ret, params) in pending {
            llmod.declare_function(&name, ret, &params);
        }
        for ic_name in &ic_globals {
            llmod.add_raw_global(format!(
                "@{} = private global [2 x i64] zeroinitializer",
                ic_name
            ));
        }
        for raw in &typed_parse_rodata {
            llmod.add_raw_global(raw.clone());
        }
    } else {
        // Issue #753: idempotent init guard. Every non-entry module gets
        // a one-byte `@__perry_init_done_<prefix>` flag and a thin
        // wrapper `<prefix>__init` that returns immediately when the
        // flag is set or stores 1 + dispatches to `<prefix>__init_body`
        // when it isn't. The wrapper is what the entry main calls
        // eagerly (for Eager modules) and what every
        // `Expr::DynamicImport` dispatch site calls (for any module
        // that's a dynamic-import target — possibly multiple sites in
        // the same program). The 2-state guard matches ESM's
        // partial-cycle semantics: re-entry during init returns without
        // re-running the body, leaving the namespace populator's work
        // partially observable. The wrapper sets `done = 1` BEFORE
        // calling the body so the re-entry path returns immediately.
        let done_global = format!("__perry_init_done_{}", module_prefix);
        llmod.add_internal_global(&done_global, I8, "0");
        let init_name = format!("{}__init", module_prefix);
        let init_body_name = format!("{}__init_body", module_prefix);
        {
            let wrap_fn = llmod.define_function(&init_name, VOID, vec![]);
            let _ = wrap_fn.create_block("entry");
            let _ = wrap_fn.create_block("guard.ret");
            let _ = wrap_fn.create_block("guard.do");
            let ret_label = wrap_fn.block_mut(1).unwrap().label.clone();
            let do_label = wrap_fn.block_mut(2).unwrap().label.clone();
            {
                let blk = wrap_fn.block_mut(0).unwrap();
                let done = blk.load(I8, &format!("@{}", done_global));
                let already = blk.icmp_ne(I8, &done, "0");
                blk.cond_br(&already, &ret_label, &do_label);
            }
            {
                let blk = wrap_fn.block_mut(1).unwrap();
                blk.ret_void();
            }
            {
                let blk = wrap_fn.block_mut(2).unwrap();
                blk.store(I8, "1", &format!("@{}", done_global));
                // Trigger init of static-dep + re-export source modules
                // before the body runs. Each `<dep>__init` is itself
                // wrapped by the same guard pattern, so this short-
                // circuits when the dep was already initialized
                // (Eager-via-main path) and fires the body when the
                // dep is Deferred and this is the first reach. The
                // entry module has no `__init` so the driver excludes
                // it from `module_init_deps`.
                for dep_prefix in &cross_module.module_init_deps {
                    if dep_prefix == module_prefix {
                        continue;
                    }
                    blk.call_void(&format!("{}__init", dep_prefix), &[]);
                }
                blk.call_void(&init_body_name, &[]);
                blk.ret_void();
            }
        }
        // Declare every dep's `__init` symbol so the wrapper's calls
        // resolve at link time. Most overlap with `non_entry_module_prefixes`
        // (whose declarations live in the entry module's compilation),
        // but a non-entry module compiled standalone has no entry-side
        // declaration list — emit them here too. `declare_function`
        // dedupes by name.
        for dep_prefix in &cross_module.module_init_deps {
            if dep_prefix == module_prefix {
                continue;
            }
            llmod.declare_function(&format!("{}__init", dep_prefix), VOID, &[]);
        }
        // The body retains every existing semantic of `<prefix>__init`
        // (strings init, globals/GC registration, top-level statements,
        // namespace populator at the tail). It's `internal` linkage:
        // only the wrapper above ever calls it, both within this module
        // and across modules via the wrapper's external symbol.
        let init_name = init_body_name;
        // Debug: emit puts("INIT: <prefix>") at the top of each module init
        let debug_init_const = if std::env::var("PERRY_DEBUG_INIT").is_ok() {
            let debug_msg = format!("INIT: {}\0", module_prefix);
            let (const_name, _) = llmod.add_string_constant(&debug_msg);
            llmod.declare_function("puts", I32, &[PTR]);
            Some(const_name)
        } else {
            None
        };
        let ic_base = llmod.ic_counter;
        let buffer_alias_base = llmod.buffer_alias_counter;
        let init_fn = llmod.define_function(&init_name, VOID, vec![]);
        init_fn.linkage = "internal".to_string();
        let _ = init_fn.create_block("entry");
        {
            let blk = init_fn.block_mut(0).unwrap();
            if let Some(ref cname) = debug_init_const {
                blk.call_void("puts", &[(PTR, &format!("@{}", cname))]);
            }
            // Each non-entry module runs its own string pool init at
            // the start of its module init function. The entry main
            // calls each module init in order (after running its own
            // strings init), so by the time user code in any module
            // executes, every module's strings are alive.
            blk.call_void(&strings_init_name, &[]);
        }
        // Same boundary as the entry-module main: hoisted post-init
        // setup must run AFTER the strings init populates module
        // globals like `@perry_class_keys_*`.
        init_fn.mark_entry_init_boundary();

        let init_boxed_vars = module_boxed_vars.clone();
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
            .clamp3_functions
            .union(&cross_module.clamp_u8_functions)
            .chain(cross_module.returns_int_functions.iter())
            .copied()
            .collect();
        let init_integer_locals = crate::collectors::collect_integer_locals(
            &hir.init,
            &cross_module.flat_const_arrays.keys().copied().collect(),
            &clamp_fn_ids,
        );
        let init_index_used_locals = crate::collectors::collect_index_used_locals(&hir.init);
        let init_strictly_i32_bounded_locals =
            crate::collectors::collect_strictly_i32_bounded_locals(
                &hir.init,
                &init_integer_locals,
                &cross_module.flat_const_arrays.keys().copied().collect(),
                &clamp_fn_ids,
            );
        let init_non_escaping_news = crate::collectors::collect_non_escaping_news(
            &hir.init,
            &init_boxed_vars,
            module_globals,
            classes,
        );
        let init_non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
            &hir.init,
            &init_boxed_vars,
            module_globals,
        );
        let init_non_escaping_object_literals =
            crate::collectors::collect_non_escaping_object_literals(
                &hir.init,
                &init_boxed_vars,
                module_globals,
            );
        let mut ctx = FnCtx {
            func: init_fn,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_label: None,
            classes,
            this_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            import_function_origin_names: &cross_module.import_function_origin_names,
            import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
            // Issue #841: node:submodule named-import + namespace registries.
            import_function_node_submodule: &cross_module.import_function_node_submodule,
            namespace_node_submodules: &cross_module.namespace_node_submodules,
            namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            func_synthetic_arguments,
            func_returns_class: &cross_module.func_returns_class,
            boxed_vars: init_boxed_vars,
            prealloc_boxes: std::collections::HashSet::new(),
            closure_rest_params: closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
            namespace_member_prefixes: &cross_module.namespace_member_prefixes,
            imported_async_funcs: &cross_module.imported_async_funcs,
            local_async_funcs: &cross_module.local_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_has_rest: &cross_module.imported_func_has_rest,
            method_param_counts: &cross_module.method_param_counts,
            method_has_rest: &cross_module.method_has_rest,
            imported_func_return_types: &cross_module.imported_func_return_types,
            ffi_signatures: &cross_module.ffi_signatures,
            imported_class_sources: &cross_module.imported_class_sources,
            interfaces: &cross_module.interfaces,
            try_depth: 0,
            pending_declares: Vec::new(),
            integer_locals: &init_integer_locals,
            shadow_slot_map: std::collections::HashMap::new(),
            arena_state_slot: None,
            class_keys_slots: HashMap::new(),
            cached_lengths: HashMap::new(),
            bounded_index_pairs: Vec::new(),
            i32_counter_slots: HashMap::new(),
            index_used_locals: &init_index_used_locals,
            strictly_i32_bounded_locals: &init_strictly_i32_bounded_locals,
            i18n: &cross_module.i18n,
            dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
            local_class_aliases: HashMap::new(),
            local_class_field_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: &cross_module.compile_time_constants,
            app_metadata: &cross_module.app_metadata,
            scalar_replaced: std::collections::HashMap::new(),
            scalar_replaced_arrays: std::collections::HashMap::new(),
            scalar_ctor_target: Vec::new(),
            non_escaping_news: init_non_escaping_news,
            non_escaping_arrays: init_non_escaping_arrays,
            non_escaping_object_literals: init_non_escaping_object_literals,
            flat_const_arrays: &cross_module.flat_const_arrays,
            array_row_aliases: HashMap::new(),
            clamp3_functions: &cross_module.clamp3_functions,
            clamp_u8_functions: &cross_module.clamp_u8_functions,
            was_unrolled: hir.init_was_unrolled,
            ic_site_counter: ic_base,
            ic_globals: Vec::new(),
            typed_parse_rodata: Vec::new(),
            typed_parse_counter: 0,
            buffer_data_slots: HashMap::new(),
            buffer_alias_base,
        };
        // Register every module-level global's ADDRESS as a GC root —
        // same reason as the entry-module branch above (issue #36). For
        // non-entry modules the registration runs inside their __init
        // function, which the entry main calls in topological order
        // right after js_gc_init, so by the time any user code executes
        // every module's globals are already GC-rooted.
        register_module_globals_as_gc_roots(&mut ctx, module_globals);
        // Issue #894: split into early/late around `lower_stmts` so a
        // computed-Symbol-key static field whose key/init reference
        // top-level module lets (e.g. effect's `make()` factory:
        // `static [TypeId] = variance`) sees populated globals.
        init_static_fields_early(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init).with_context(|| {
            format!(
                "lowering init statements of non-entry module '{}'",
                hir.name
            )
        })?;
        init_static_fields_late(&mut ctx, hir)?;

        // Issue #100: populate `@__perry_ns_<module_prefix>` from the
        // namespace_entries list at the tail of the non-entry __init.
        // The entry main has already called this module's __init AFTER
        // every static-import dependency's __init (topo sort) — so
        // re-export sources have populated their getters. Local
        // exports' bindings are also set because `lower_stmts` ran
        // above. The dispatcher in `Expr::DynamicImport` loads
        // `@__perry_ns_<prefix>` and wraps it in `js_promise_resolved`.
        // Issue #842: also run the populator for side-effect-only
        // dynamic-import targets (`namespace_entries` empty but module
        // is a target). The populator emits `js_create_namespace(0, ...)`
        // → an empty NaN-boxed object → stored into `@__perry_ns_<prefix>`,
        // satisfying the consumer-side extern reference.
        if (!cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target)
            && !ctx.block().is_terminated()
        {
            emit_namespace_populator(
                &mut ctx,
                &cross_module.namespace_entries,
                namespace_key_globals,
                module_prefix,
            );
        }

        if !ctx.block().is_terminated() {
            ctx.block().ret_void();
        }
        let ic_globals = std::mem::take(&mut ctx.ic_globals);
        let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
        let ic_end = ctx.ic_site_counter;
        let pending = std::mem::take(&mut ctx.pending_declares);
        let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
        drop(ctx);
        llmod.ic_counter = ic_end;
        llmod.buffer_alias_counter += buffer_alias_used;
        for (name, ret, params) in pending {
            llmod.declare_function(&name, ret, &params);
        }
        for ic_name in &ic_globals {
            llmod.add_raw_global(format!(
                "@{} = private global [2 x i64] zeroinitializer",
                ic_name
            ));
        }
        for raw in &typed_parse_rodata {
            llmod.add_raw_global(raw.clone());
        }
    }
    Ok(())
}

/// Emit LLVM alias-scope metadata for the module's Buffer/Uint8Array data
/// pointers. Each buffer registered in `FnCtx::buffer_data_slots` gets a
/// unique scope within a shared alias domain, plus a scope-list node
/// (`!alias.scope` target) and a noalias-list node (`!noalias` target) that
/// enumerates every *other* buffer's scope.
///
/// Numbering (chosen to avoid colliding with `!0 = !{}` used by
/// `!invariant.load`):
/// - `!100`                — shared alias domain
/// - `!(101 + idx)`        — per-buffer scope, one per entry in buffer_data_slots
/// - `!(201 + idx)`        — scope list referenced by `!alias.scope` on loads/stores
/// - `!(301 + idx)`        — noalias list referenced by `!noalias` on loads/stores
///
/// LLVM's LoopVectorizer can then prove that `src[i]` reads don't alias
/// `dst[j]` writes — the fix for the "unsafe dependent memory operations"
/// vectorization remark on the image_conv blur kernel.
fn emit_buffer_alias_metadata(llmod: &mut LlModule, count: u32) {
    if count == 0 {
        return;
    }
    // Shared domain.
    llmod.add_metadata_line("!100 = distinct !{!100}".to_string());
    // Per-buffer scope nodes.
    for i in 0..count {
        let sid = 101 + i;
        llmod.add_metadata_line(format!("!{} = distinct !{{!{}, !100}}", sid, sid));
    }
    // Single-element alias-scope lists (one per buffer).
    for i in 0..count {
        let list_id = 201 + i;
        let scope_id = 101 + i;
        llmod.add_metadata_line(format!("!{} = !{{!{}}}", list_id, scope_id));
    }
    // Noalias lists: for buffer i, every *other* buffer's scope.
    for i in 0..count {
        let list_id = 301 + i;
        let others: Vec<String> = (0..count)
            .filter(|j| *j != i)
            .map(|j| format!("!{}", 101 + j))
            .collect();
        if others.is_empty() {
            // Single buffer: empty noalias set — LLVM accepts `!{}` but
            // it's a no-op. Still emit so `!noalias !{N}` references resolve.
            llmod.add_metadata_line(format!("!{} = !{{}}", list_id));
        } else {
            llmod.add_metadata_line(format!("!{} = !{{{}}}", list_id, others.join(", ")));
        }
    }
}

/// Emit the string pool into the module: byte-array constants, handle
/// globals, and the `__perry_init_strings_<prefix>` function that
/// allocates + NaN-boxes + GC-roots each handle exactly once at startup.
///
/// The string pool was constructed with a `module_prefix`, so every
/// `entry.bytes_global` / `entry.handle_global` is already prefixed.
/// Emission uses those names directly — no extra prefixing here.
fn emit_string_pool(
    llmod: &mut LlModule,
    strings: &StringPool,
    module_prefix: &str,
    class_keys_init_data: &[(String, String, u32)],
    class_ids: &HashMap<String, u32>,
    classes: &HashMap<String, &perry_hir::Class>,
    closure_rest_params: &HashMap<u32, usize>,
    closure_arities: &HashMap<u32, u32>,
    // Issue #653: wrappers (`__perry_wrap_<name>`) for top-level user functions
    // that declare a rest param. Each entry is `(wrapper_symbol, fixed_arity)`
    // — the runtime side-table is keyed on the wrapper's func_ptr, NOT the
    // underlying user function, because that's what `js_closure_alloc_singleton`
    // stores in the ClosureHeader. Without this registration, calling a user
    // function as a value through `js_closure_call_apply_with_spread` fed the
    // raw spread elements into the wrapper's flat `(this, a0, a1)` signature
    // instead of bundling args[fixed_arity..] into a real array — the rest
    // param then read a single element's bits as if it were the rest array.
    user_fn_wrapper_rest: &[(String, usize)],
    // Refs #915 (gap 1 from #899): subset of `closure_rest_params` whose
    // rest param is the HIR-synthesized `arguments` array. These need
    // `js_register_closure_synthetic_arguments` so the runtime bundles
    // ALL passed args (not just the trailing tail) into the rest slot —
    // matching JS spec semantics for `arguments.length`.
    closure_synthetic_arguments: &std::collections::HashSet<u32>,
    // Mirror of `closure_synthetic_arguments` for the top-level user-fn
    // wrapper path: each entry is `wrapper_symbol` whose underlying
    // function has its synthesized `arguments` rest param.
    user_fn_wrapper_synthetic_arguments: &std::collections::HashSet<String>,
    // Declared param count for every top-level user-function wrapper
    // (`__perry_wrap_<original_name>`) — used to register the wrapper's
    // arity in the runtime's `CLOSURE_ARITY_REGISTRY` so reads of
    // `fn.length` on a closure value return the spec-correct
    // declared-param count. Entries for wrappers also present in
    // `user_fn_wrapper_rest` are skipped (those go through the rest
    // registry which already pins arity).
    user_fn_wrapper_arity: &[(String, u32)],
) {
    for entry in strings.iter() {
        // .rodata bytes — `[N+1 x i8]` because we include the null terminator.
        llmod.add_named_string_constant(&entry.bytes_global, entry.byte_len + 1, &entry.escaped_ir);
        llmod.add_internal_global(&entry.handle_global, DOUBLE, "0.0");
    }

    // Per-class packed-keys constants (rodata) — referenced by the
    // js_build_class_keys_array call below at module init.
    // Naming: `@perry_class_keys_packed_<modprefix>__<idx>` so we
    // don't collide with anything else.
    let mut packed_global_names: Vec<String> = Vec::with_capacity(class_keys_init_data.len());
    for (idx, (_global_name, packed, _fc)) in class_keys_init_data.iter().enumerate() {
        if packed.is_empty() {
            packed_global_names.push(String::new());
            continue;
        }
        let bytes = packed.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 8);
        lit.push_str("c\"");
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");
        let name = format!("perry_class_keys_packed_{}__{}", module_prefix, idx);
        llmod.add_named_string_constant(&name, bytes.len() + 1, &lit);
        packed_global_names.push(name);
    }

    let init_name = format!("__perry_init_strings_{}", module_prefix);
    let init_fn = llmod.define_function(&init_name, VOID, vec![]);
    let _ = init_fn.create_block("entry");
    let blk = init_fn.block_mut(0).unwrap();

    for entry in strings.iter() {
        let bytes_ref = format!("@{}", entry.bytes_global);
        let handle_ref = format!("@{}", entry.handle_global);
        let len_str = entry.byte_len.to_string();

        let init_fn = if entry.is_wtf8 {
            "js_string_from_wtf8_bytes"
        } else {
            "js_string_from_bytes"
        };
        let handle = blk.call(I64, init_fn, &[(PTR, &bytes_ref), (I32, &len_str)]);
        let nanboxed = blk.call(DOUBLE, "js_nanbox_string", &[(I64, &handle)]);
        blk.store(DOUBLE, &nanboxed, &handle_ref);
        let addr_i64 = blk.ptrtoint(&handle_ref, I64);
        blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
    }

    // Build per-class keys arrays via js_build_class_keys_array,
    // store the result in the per-class keys global. Done ONCE at
    // module init; every `new ClassName()` call from then on does a
    // single global load + inline allocator call (no SHAPE_CACHE
    // lookup, no js_build_class_keys_array overhead).
    for (idx, (global_name, packed, field_count)) in class_keys_init_data.iter().enumerate() {
        // Resolve class id from the global name. The global name is
        // `perry_class_keys_<modprefix>__<class>` so we strip the
        // prefix to recover the sanitized class name and look up
        // the id by walking class_ids. Since multiple classes might
        // have the same sanitized name (rare but possible), we just
        // pick the first matching one — class_ids is keyed by the
        // pre-sanitized name so a direct lookup works for ASCII.
        let prefix = format!("perry_class_keys_{}__", module_prefix);
        let sanitized_class = global_name.strip_prefix(&prefix).unwrap_or("");
        let class_id = class_ids
            .iter()
            .find(|(k, _)| sanitize(k) == sanitized_class)
            .map(|(_, &v)| v)
            .unwrap_or(0);

        let cid_str = class_id.to_string();
        let fc_str = field_count.to_string();
        let packed_ref = if packed.is_empty() {
            "null".to_string()
        } else {
            format!("@{}", packed_global_names[idx])
        };
        let len_str = packed.len().to_string();
        let arr = blk.call(
            I64,
            "js_build_class_keys_array",
            &[
                (I32, &cid_str),
                (I32, &fc_str),
                (PTR, &packed_ref),
                (I32, &len_str),
            ],
        );
        blk.store(I64, &arr, &format!("@{}", global_name));
    }

    // Register the parent-class chain for every class with a parent.
    // The runtime allocators do this on every alloc; the inline
    // bump allocator skips it. Without this one-time call, the
    // CLASS_REGISTRY misses the `child → parent` edge and walks of
    // the inheritance chain (e.g. `instanceof Shape` on a `Square`
    // where `Square extends Rectangle extends Shape`) terminate
    // prematurely. We emit one call per inheriting class, sorted by
    // class id for deterministic ordering.
    let mut parent_pairs: Vec<(u32, u32)> = Vec::new();
    for (name, &cid) in class_ids.iter() {
        if let Some(class) = classes.get(name) {
            if let Some(parent_name) = &class.extends_name {
                if let Some(&parent_cid) = class_ids.get(parent_name) {
                    if parent_cid != 0 {
                        parent_pairs.push((cid, parent_cid));
                    }
                }
            }
        }
    }
    parent_pairs.sort_unstable();
    for (cid, parent_cid) in parent_pairs {
        blk.call_void(
            "js_register_class_parent",
            &[(I32, &cid.to_string()), (I32, &parent_cid.to_string())],
        );
    }

    // Issue #392: register every user class method in the runtime
    // VTABLE_REGISTRY so cross-module callers can dispatch via
    // `js_native_call_method` even when the codegen of the calling
    // module can't see the class definition. Same-module calls
    // already resolve through the static idispatch tower in
    // `lower_call.rs` (which iterates `ctx.classes` to find
    // implementors); cross-module calls fall through to
    // `js_native_call_method`, which reads the receiver's class_id
    // and looks up the vtable.
    //
    // Only register classes DEFINED in this module — `class_ids` may
    // include imported classes (Changeset imported from `shared.ts`
    // into `main.ts` for `new Changeset()`), but the `perry_method_*`
    // symbols for those live in the defining module's object file.
    // Each module's init registers its own classes; the linker
    // ensures all init functions run before main.
    let mut method_triples: Vec<(u32, String, String, u32)> = Vec::new();
    for (class_name, class) in classes.iter() {
        // Refs #486: skip alias keys (class_table now contains both the
        // canonical name and self-binding aliases like `_X` from
        // `var X = class _X`); the symbol emission iterates by canonical
        // class.name. Without this skip the alias key generates bogus
        // symbol names like `perry_method_<mod>___X__method` (extra
        // leading underscore from sanitize("_X")) that don't resolve at
        // link time.
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name) {
            Some(&c) if c != 0 => c,
            _ => continue,
        };
        for method in &class.methods {
            let llvm_name = format!(
                "perry_method_{}__{}__{}",
                module_prefix,
                sanitize(class_name),
                sanitize(&method.name),
            );
            method_triples.push((
                cid,
                method.name.clone(),
                llvm_name,
                method.params.len() as u32,
            ));
        }
    }
    method_triples.sort_unstable();
    for (cid, method_name, llvm_name, param_count) in method_triples {
        // The pre-intern pass before `emit_string_pool` ensured every
        // method name has a string pool entry; look it up here without
        // mutating the pool.
        let entry = match strings.iter().find(|e| e.value == method_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        // Cast the method function pointer to i64 via ptrtoint so the
        // runtime can store it as a `usize` in the VTABLE_REGISTRY
        // entry. The `inttoptr` round-trip in `call_vtable_method`
        // restores it for the indirect call.
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        blk.call_void(
            "js_register_class_method",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
                (I64, &param_count.to_string()),
            ],
        );
    }

    // Refs #618 / #420: register every class id with the runtime so
    // `js_value_typeof` can distinguish a class ref (NaN-boxed INT32 with
    // class_id payload) from a real int32 numeric value. Without this,
    // `typeof <class>` returns "number" for classes that don't define any
    // methods (the existing `js_register_class_method` loop only fires
    // for classes with at least one method body). drizzle's `class
    // FakePrimitiveParam { static [entityKind] = "FakePrimitiveParam" }`
    // and similar method-less marker classes hit this.
    {
        let mut all_class_ids: Vec<u32> = Vec::new();
        let mut anon_shape_ids: Vec<u32> = Vec::new();
        for (class_name, class) in classes.iter() {
            if *class_name != class.name {
                continue;
            }
            let cid = match class_ids.get(class_name).copied() {
                Some(c) if c != 0 => c,
                _ => continue,
            };
            all_class_ids.push(cid);
            // date-fns / drizzle / lodash plain-object duck-checks need
            // `({ x: 1 }).constructor === Object` to hold. The HIR
            // synthesizes an `__AnonShape_<hash>` class per literal
            // shape; mark each such class id so the runtime
            // `js_object_get_field_by_name` resolves `.constructor` to
            // the global `Object` constructor instead of the synthetic
            // class ref.
            if class_name.starts_with("__AnonShape_") {
                anon_shape_ids.push(cid);
            }
        }
        all_class_ids.sort_unstable();
        all_class_ids.dedup();
        for cid in all_class_ids {
            blk.call_void(
                "js_register_class_id",
                &[(crate::types::I32, &cid.to_string())],
            );
        }
        anon_shape_ids.sort_unstable();
        anon_shape_ids.dedup();
        for cid in anon_shape_ids {
            blk.call_void(
                "js_register_anon_shape_class_id",
                &[(crate::types::I32, &cid.to_string())],
            );
        }
    }

    // Refs #486 (hono logger middleware): also register every class
    // getter in the runtime VTABLE_REGISTRY. Without this, cross-module
    // `obj.prop` reads (where `obj` is statically typed `any` so the
    // codegen has no static dispatch info) fall through `js_get_object_field_by_name`
    // past the `vtable.getters.get(prop)` lookup at value.rs:2268 — the
    // map is always empty — and into the field-by-name dispatcher,
    // which returns `undefined` for properties that exist only as
    // getters. Hono's `Context.get req()` is the canonical breakage:
    // the logger middleware reads `c.req.url` from a JS-bundled hono
    // dist via `compilePackages`, and pre-fix `c.req` always returned
    // `undefined`.
    let mut getter_pairs: Vec<(u32, String, String)> = Vec::new();
    for (class_name, class) in classes.iter() {
        // Refs #486: skip alias keys (see method-emission loop above).
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
        for (prop, getter_fn) in &class.getters {
            // The local-emit path at codegen.rs:1858 prepends `__get_`
            // to the HIR-assigned getter name (`get_<prop>`), giving
            // the LLVM symbol `perry_method_<modprefix>__<class>__<sanitize(__get_get_<prop>)>`.
            // Use the same mangling here so the registered func_ptr
            // matches the actual emitted body.
            let inner = format!("__get_{}", getter_fn.name);
            let llvm_name = format!(
                "perry_method_{}__{}__{}",
                module_prefix,
                sanitize(class_name),
                sanitize(&inner),
            );
            getter_pairs.push((cid, prop.clone(), llvm_name));
        }
    }
    getter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name) in getter_pairs {
        let entry = match strings.iter().find(|e| e.value == prop_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        blk.call_void(
            "js_register_class_getter",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
            ],
        );
    }

    // Refs #486 (hono): parallel registration for class setters. Without
    // this, `c.res = response` (where `c` is `any`-typed) bypasses hono
    // Context's `set res(_res) { …; this.finalized = true; }` and writes
    // directly to a regular field slot. `this.finalized = true` never
    // executes, hono-base sees `c.finalized = false` and throws "Context
    // is not finalized" on every request through compose. Mirror's the
    // getter-pairs loop above; emission mangling matches the
    // setter-method-emission path at codegen.rs:2041 (renamed.name =
    // "__set_<prop>" → LLVM symbol perry_method_<mp>__<class>____set_<f.name>).
    let mut setter_pairs: Vec<(u32, String, String)> = Vec::new();
    for (class_name, class) in classes.iter() {
        if *class_name != class.name {
            continue;
        }
        // Imported class stubs carry id == 0 (they're typed-name
        // placeholders for cross-module dispatch; the defining module's init
        // registers their methods). Skip them here so we don't re-emit the
        // registration. Previously this filter was `method.body.is_empty()`;
        // the id check is equivalent for stubs and also catches getter/setter
        // and property-decorator init that legitimately has an empty body.
        if class.id == 0 {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
        for (prop, setter_fn) in &class.setters {
            let inner = format!("__set_{}", setter_fn.name);
            let llvm_name = format!(
                "perry_method_{}__{}__{}",
                module_prefix,
                sanitize(class_name),
                sanitize(&inner),
            );
            setter_pairs.push((cid, prop.clone(), llvm_name));
        }
    }
    setter_pairs.sort_unstable();
    for (cid, prop_name, llvm_name) in setter_pairs {
        let entry = match strings.iter().find(|e| e.value == prop_name) {
            Some(e) => e,
            None => continue,
        };
        let bytes_global = format!("@{}", entry.bytes_global);
        let len_str = entry.byte_len.to_string();
        let func_ref = format!("@{}", llvm_name);
        let func_i64 = blk.ptrtoint(&func_ref, I64);
        let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
        blk.call_void(
            "js_register_class_setter",
            &[
                (I64, &cid.to_string()),
                (I64, &bytes_i64),
                (I64, &len_str),
                (I64, &func_i64),
            ],
        );
    }

    // Issue #493: register each rest-bearing closure body's func_ptr ->
    // fixed_arity in the runtime's closure-rest side table. `js_closure_callN`
    // consults it to bundle trailing args at call sites where codegen
    // doesn't know the closure's arity statically (e.g. `obj.cb(a, b, c)`
    // where `cb` is a class field holding `(...args) => …`). Without this
    // entry the closure body sees the first arg as the rest param itself,
    // not the bundled array — `args.length` reads `1` against the string
    // value, and trailing args are dropped. Static call sites (named fns,
    // `Expr::FuncRef`, `let f = (...args)=>…; f(a,b,c)`) keep their
    // existing call-site bundling and never enter this dispatch path.
    let mut sorted_rest: Vec<(u32, usize)> = closure_rest_params
        .iter()
        .map(|(fid, ri)| (*fid, *ri))
        .collect();
    sorted_rest.sort_unstable();
    for (fid, fixed_arity) in sorted_rest {
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        // Refs #915 (gap 1 from #899): closures whose rest param is the
        // synthesized `arguments` use the synthetic-arguments registration
        // so the runtime bundles ALL args into the rest slot.
        let runtime_fn = if closure_synthetic_arguments.contains(&fid) {
            "js_register_closure_synthetic_arguments"
        } else {
            "js_register_closure_rest"
        };
        blk.call_void(
            runtime_fn,
            &[(PTR, &func_ref), (I32, &fixed_arity.to_string())],
        );
    }

    // Refs #421: register every non-rest closure's declared param count so
    // `js_native_call_value` can pad missing trailing args with TAG_UNDEFINED
    // when a closure stored as a class field is invoked method-style on an
    // any-typed receiver with fewer args than declared. Rest-bearing closures
    // are already handled by the closure-rest registry above (which pads
    // internally via `dispatch_rest_bundled`).
    let mut sorted_arities: Vec<(u32, u32)> = closure_arities
        .iter()
        .map(|(fid, arity)| (*fid, *arity))
        .collect();
    sorted_arities.sort_unstable();
    for (fid, arity) in sorted_arities {
        let closure_sym = format!("perry_closure_{}__{}", module_prefix, fid);
        let func_ref = format!("@{}", closure_sym);
        blk.call_void(
            "js_register_closure_arity",
            &[(PTR, &func_ref), (I32, &arity.to_string())],
        );
    }

    // Issue #653: register `__perry_wrap_<name>` wrappers for top-level user
    // functions whose source signature includes a rest param. Mirrors the
    // closure-rest loop above but keyed on the wrapper's symbol rather than
    // the closure body. See `user_fn_wrapper_rest` doc on this fn's signature.
    let mut sorted_wrappers: Vec<(String, usize)> = user_fn_wrapper_rest.to_vec();
    sorted_wrappers.sort();
    let rest_wrapper_names: std::collections::HashSet<String> =
        sorted_wrappers.iter().map(|(s, _)| s.clone()).collect();
    for (wrap_sym, fixed_arity) in sorted_wrappers {
        let func_ref = format!("@{}", wrap_sym);
        // Refs #915 (gap 1 from #899): wrappers whose underlying function
        // declared a synthesized `arguments` rest param need the
        // synthetic-arguments registration.
        let runtime_fn = if user_fn_wrapper_synthetic_arguments.contains(&wrap_sym) {
            "js_register_closure_synthetic_arguments"
        } else {
            "js_register_closure_rest"
        };
        blk.call_void(
            runtime_fn,
            &[(PTR, &func_ref), (I32, &fixed_arity.to_string())],
        );
    }

    // Register declared param count for `__perry_wrap_<name>` wrappers of
    // every non-rest top-level user function. Mirrors the closure-arity
    // loop above (which registered inline closures) and the rest-wrapper
    // loop just above. The runtime's `.length` property accessor on a
    // closure value reads from this registry — ramda's
    // `converge(<fn>, [filter, reject])` IIFE feeds
    // `pluck('length', fns)` → `reduce(max, 0, …)` → `curryN(N, …)` →
    // `_arity(N, …)` at module init; without the wrappers registering
    // their arity, `pluck('length', [filter, reject])` came back as
    // `[undefined, undefined]`, `reduce(max, 0, …)` evaluated to `NaN`,
    // and `_arity(NaN, …)` threw
    // `First argument to _arity must be a non-negative integer no greater
    // than ten` before R.add / R.sum was ever called.
    let mut sorted_wrapper_arities: Vec<(String, u32)> = user_fn_wrapper_arity
        .iter()
        .filter(|(name, _)| !rest_wrapper_names.contains(name))
        .cloned()
        .collect();
    sorted_wrapper_arities.sort();
    for (wrap_sym, arity) in sorted_wrapper_arities {
        let func_ref = format!("@{}", wrap_sym);
        blk.call_void(
            "js_register_closure_arity",
            &[(PTR, &func_ref), (I32, &arity.to_string())],
        );
    }

    blk.ret_void();
}

/// Compile a static class method as a top-level LLVM function with
/// no `this` parameter. Mostly identical to `compile_function` but
/// the LLVM symbol name is `perry_static_<modprefix>__<class>__<method>`
/// instead of `perry_fn_<modprefix>__<name>`.
#[allow(clippy::too_many_arguments)]
fn compile_static_method(
    llmod: &mut LlModule,
    class_name: &str,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = format!(
        "perry_static_{}__{}__{}",
        module_prefix,
        sanitize(class_name),
        sanitize(&f.name),
    );

    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    let local_types: HashMap<u32, perry_types::Type> =
        f.params.iter().map(|p| (p.id, p.ty.clone())).collect();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let integer_locals = crate::collectors::collect_integer_locals(
        &f.body,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );
    let index_used_locals = crate::collectors::collect_index_used_locals(&f.body);
    let strictly_i32_bounded_locals = crate::collectors::collect_strictly_i32_bounded_locals(
        &f.body,
        &integer_locals,
        &cross_module.flat_const_arrays.keys().copied().collect(),
        &clamp_fn_ids,
    );

    let static_boxed_vars = module_boxed_vars.clone();
    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &f.body,
        &static_boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_arrays =
        crate::collectors::collect_non_escaping_arrays(&f.body, &static_boxed_vars, module_globals);
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body,
        &static_boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        locals,
        local_types,
        current_block: 0,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack: Vec::new(),
        // Static methods have no `this` but they CAN reference
        // sibling static methods/fields via the class name (which
        // they handle via StaticFieldGet/StaticMethodCall, not via
        // `this`). The class_stack is empty here.
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: static_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &integer_locals,
        shadow_slot_map: std::collections::HashMap::new(),
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        i32_counter_slots: HashMap::new(),
        index_used_locals: &index_used_locals,
        strictly_i32_bounded_locals: &strictly_i32_bounded_locals,
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news,
        non_escaping_arrays,
        non_escaping_object_literals,
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        was_unrolled: f.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_alias_base,
    };
    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of static '{}::{}'", class_name, f.name))?;

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if f.is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &undef);
        }
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    drop(ctx);
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}

/// Register every module-level global's ADDRESS with the runtime GC
/// root scanner. Emitted at the top of each module's `main` / `__init`
/// function, right after `js_gc_init` and the strings-init prelude.
///
/// Background (issue #36): module globals are just LLVM globals of type
/// `double` that store NaN-boxed JSValues. Before this fix the GC had
/// no way to learn about their addresses — only string-handle globals
/// were registered via `js_gc_register_global_root` (codegen.rs ~2217).
/// That was fine for programs whose module-level state was reachable
/// through the conservative stack scan at every GC cycle, but broke
/// any program where a Map / Array / user-class instance lived only in
/// a module `const X = new Map(...)` and a GC fired at a moment when
/// no stack variable held the pointer. The pg driver's CONN_STATES
/// Map is the canonical victim — after v0.5.25 made `gc_malloc`
/// trigger GC, the Map was reliably swept mid-decode and the next
/// `CONN_STATES.get(id)` returned a dangling header.
///
/// Registering the global's *address* (not its current value) means
/// the GC reads the up-to-date pointer every cycle, so reassignments
/// are followed correctly. `mark_global_roots` handles both NaN-boxed
/// (POINTER_TAG / STRING_TAG / BIGINT_TAG) and raw-i64 interpretations,
/// and both fall through the `valid_ptrs` filter, so it's safe to
/// register every global regardless of its declared type — number /
/// boolean / undefined bits simply don't match any live heap pointer
/// and get discarded.
fn register_module_globals_as_gc_roots(
    ctx: &mut crate::expr::FnCtx<'_>,
    module_globals: &HashMap<u32, String>,
) {
    // Sort by id for deterministic emit order (helps with diff-testing
    // the generated IR and matches the existing `class_keys` pattern).
    let mut entries: Vec<(&u32, &String)> = module_globals.iter().collect();
    entries.sort_by_key(|(id, _)| **id);
    for (_, global_name) in entries {
        let addr = ctx.block().ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }
}

/// Early static-field setup: registrations that don't read any
/// module-level binding's value (Error-extending classes, well-known
/// symbol method hooks). Safe to emit before `stmt::lower_stmts` —
/// values referenced are either compile-time constants (class ids,
/// function pointers) or computed entirely from `hir` metadata.
///
/// The split (early vs. late) was introduced for issue #894 (effect's
/// `make()` factory's `static [TypeId] = variance` — both the key and
/// the init reference module-level lets that haven't been initialized
/// at the point the old combined `init_static_fields` ran).
fn init_static_fields_early(ctx: &mut crate::expr::FnCtx<'_>, hir: &HirModule) -> Result<()> {
    // Phase C.3: register user classes that extend the built-in Error
    // (or any of its subclasses) with the runtime, so `instanceof Error`
    // walks the chain and returns true. Without this, `new HttpError(...)
    // instanceof Error` returns false because the runtime's
    // `EXTENDS_ERROR_REGISTRY` is empty for user classes.
    for c in &hir.classes {
        // Walk this class's extends_name chain; if any ancestor is a
        // built-in error subclass, register this class's id.
        let mut cur: Option<String> = c.extends_name.clone();
        let mut extends_error = false;
        let mut depth = 0usize;
        while let Some(name) = cur {
            if matches!(
                name.as_str(),
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "AggregateError"
            ) {
                extends_error = true;
                break;
            }
            // Walk user-defined ancestor chain.
            if let Some(parent) = ctx.classes.get(&name) {
                cur = parent.extends_name.clone();
                depth += 1;
                if depth > 32 {
                    break;
                }
            } else {
                cur = None;
            }
        }
        if extends_error {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                let cid_str = cid.to_string();
                ctx.block().call_void(
                    "js_register_class_extends_error",
                    &[(crate::types::I32, &cid_str)],
                );
            }
        }
    }
    // Well-known symbol class hooks: HIR lifts `static [Symbol.hasInstance]`
    // and `get [Symbol.toStringTag]` to top-level functions with the
    // prefixes `__perry_wk_hasinstance_<class>` / `__perry_wk_tostringtag_<class>`.
    // Scan `hir.functions`, compute the LLVM symbol via `scoped_fn_name`,
    // and emit `js_register_class_<hook>(class_id, ptrtoint(@func, i64))`
    // at module init so the runtime's `js_instanceof` / `js_object_to_string`
    // can dispatch through them.
    let module_prefix = ctx.strings.module_prefix().to_string();
    for f in &hir.functions {
        let (registrar, class_name): (&str, &str) =
            if let Some(rest) = f.name.strip_prefix("__perry_wk_hasinstance_") {
                ("js_register_class_has_instance", rest)
            } else if let Some(rest) = f.name.strip_prefix("__perry_wk_tostringtag_") {
                ("js_register_class_to_string_tag", rest)
            } else {
                continue;
            };
        let Some(&cid) = ctx.class_ids.get(class_name) else {
            continue;
        };
        let cid_str = cid.to_string();
        let llvm_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&f.name));
        let func_ref = format!("@{}", llvm_sym);
        let blk = ctx.block();
        let func_ptr_i64 = blk.ptrtoint(&func_ref, I64);
        blk.call_void(
            registrar,
            &[(crate::types::I32, &cid_str), (I64, &func_ptr_i64)],
        );
    }
    Ok(())
}

/// Late static-field setup: per-class static-field initializer evaluation,
/// computed-Symbol-key registration, and static-block invocation. Must
/// run AFTER `stmt::lower_stmts` so module-level lets referenced by
/// these initializers (e.g. `static [TypeId] = variance` where both
/// `TypeId` and `variance` are top-level `const`s) read their populated
/// global slots rather than the zero default.
///
/// Issue #894: effect's `function make(ast) { return class { static
/// [TypeId] = variance } }` factory pattern hit this; the `TypeId`
/// symbol and `variance` value were both top-level module lets, and
/// the pre-#894 combined `init_static_fields` ran before user init,
/// so `js_class_register_static_symbol(class_id, 0.0, 0.0)` registered
/// nothing reachable. `isSchema(C)` then returned false on a class
/// returned from `make`, dual()'s predicate failed, and the failing
/// `.annotations({...})` chain eventually fed `undefined` to a `make`
/// call that read `ast._tag` → `TypeError: Cannot read properties of
/// undefined (reading '_tag')` during Schema.ts module init.
fn init_static_fields_late(ctx: &mut crate::expr::FnCtx<'_>, hir: &HirModule) -> Result<()> {
    // Issue #685: nested classes (declared as expressions inside a
    // factory function body, e.g. `return class X extends Y { static
    // params = params.slice() }` in effect's `TemplateLiteralParser`)
    // are hoisted into `module.classes` by HIR lowering, but their
    // static-field initializers may reference parameters of the
    // enclosing function — those LocalIds aren't in the module-init
    // scope. The fallback at `expr.rs::LocalGet` returns `0.0`, so the
    // hoisted init becomes `(0.0).slice()` and throws
    // `TypeError: (number).slice is not a function` deep in
    // `<module>__init`, before any user code runs.
    //
    // Skip such inits at module level — the static field's storage
    // remains the zero default, which is wrong but harmless (the class
    // is built fresh on each factory invocation and the static slot
    // would need re-emitting per-invocation to be correct). The full
    // fix is to emit the init at the class-expression site inside the
    // factory body; tracking the eager-eval-of-inner-class-statics
    // separately.
    let mut module_local_scope: std::collections::HashSet<u32> =
        ctx.module_globals.keys().copied().collect();
    // Top-level `let` / `const` bindings may not appear in
    // `module_globals` (the global table only includes vars referenced
    // from inner functions or exported). For the purpose of "is this
    // LocalId in the module's own scope," count every top-level
    // `Stmt::Let` id too — otherwise a valid
    // `static foo = topLevelConst` would be wrongly skipped.
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, .. } = s {
            module_local_scope.insert(*id);
        }
    }
    let init_references_out_of_scope_local = |init_expr: &perry_hir::Expr| -> bool {
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        crate::collectors::collect_ref_ids_in_expr(init_expr, &mut refs);
        refs.iter().any(|id| !module_local_scope.contains(id))
    };
    for c in &hir.classes {
        for sf in &c.static_fields {
            // Computed-key static fields go through the class-static-symbol
            // side table. Refs #420 — drizzle's `static [entityKind] =
            // "Table"` is consulted by `Object.prototype.hasOwnProperty.call(
            // type, entityKind)` in drizzle's `is(value, type)`.
            if let (Some(key_expr), Some(init_expr)) = (sf.key_expr.as_ref(), sf.init.as_ref()) {
                if init_references_out_of_scope_local(init_expr)
                    || init_references_out_of_scope_local(key_expr)
                {
                    continue;
                }
                let Some(&class_id) = ctx.class_ids.get(&c.name) else {
                    continue;
                };
                let key_v = crate::expr::lower_expr(ctx, key_expr)?;
                let val_v = crate::expr::lower_expr(ctx, init_expr)?;
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
                continue;
            }
            let key = (c.name.clone(), sf.name.clone());
            let Some(global_name) = ctx.static_field_globals.get(&key).cloned() else {
                continue;
            };
            if let Some(init_expr) = &sf.init {
                if init_references_out_of_scope_local(init_expr) {
                    continue;
                }
                let v = crate::expr::lower_expr(ctx, init_expr)?;
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &v, &g_ref);
            }
        }
    }
    // Static blocks — emitted as synthetic static methods with the
    // name prefix `__perry_static_init_`. Call them in registration
    // order for each class, after that class's static fields are
    // initialized, so they can reference those fields.
    for c in &hir.classes {
        for sm in &c.static_methods {
            if !sm.name.starts_with("__perry_static_init_") {
                continue;
            }
            let key = (c.name.clone(), sm.name.clone());
            if let Some(llvm_name) = ctx.methods.get(&key).cloned() {
                ctx.block().call(DOUBLE, &llvm_name, &[]);
            }
        }
    }
    Ok(())
}

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::boxed_vars::{collect_boxed_vars, collect_let_types_in_stmts};
use crate::collectors::{collect_closures_in_stmts, collect_let_ids, collect_ref_ids_in_stmts};

/// Mangle a HIR function name into an LLVM symbol, scoped by module prefix.
///
/// `perry_fn_<modprefix>__<funcname>`. The double-underscore between
/// module prefix and function name is the delimiter — picked because
/// JS identifiers can't contain `__` in user-visible code, so it can't
/// collide with sanitized user names.
/// Issue #100: emit the IR that populates this module's
/// `@__perry_ns_<module_prefix>` global from the resolved namespace
/// entry list. Called at the end of `__perry_init_<prefix>` (or `main`
/// for the entry module) AFTER the module's top-level statements have
/// finished — at that point every local export's binding is set, and
/// every dependency's `__init` has already run (topo-sort guarantees
/// `ExportAll` / `ReExport` sources are initialised first), so
/// cross-module getters are safe to call.
///
/// The IR sequence per call:
///
///   1. Alloca three parallel stack arrays sized `[N x ?]` — keys (ptr),
///      key_lens (i32), values (double).
///   2. For each entry i in `namespace_entries`:
///      - Store `getelementptr inbounds [L x i8], ptr @.strK, i64 0, i64 0`
///        into `keys[i]` and `L` into `key_lens[i]`.
///      - Compute the value JSValue per `NamespaceEntryKind` and store
///        into `values[i]`.
///   3. Call `js_create_namespace(N, ptr keys, ptr key_lens, ptr values)`.
///   4. Store the result into `@__perry_ns_<module_prefix>`.
///
/// Always emits the `js_create_namespace` call + store, even when
/// `entries` is empty. This is required for Issue #842 (side-effect-only
/// dynamic-import targets — no exports, but the consumer still needs a
/// non-NaN `@__perry_ns_<prefix>` to load). The runtime tolerates
/// `n == 0` and returns an empty NaN-boxed object. The caller is
/// responsible for ensuring `key_globals.len() == entries.len()`.
fn emit_namespace_populator(
    ctx: &mut crate::expr::FnCtx<'_>,
    entries: &[NamespaceEntry],
    key_globals: &[(String, usize)],
    module_prefix: &str,
) {
    debug_assert_eq!(entries.len(), key_globals.len());
    // Issue #842: side-effect-only dynamic-import targets land here
    // with `entries.is_empty()`. The runtime `js_create_namespace`
    // tolerates `n == 0` and returns a fresh empty object — exactly
    // what an export-less module's namespace should look like. We
    // still alloca minimum-size buffers (`[1 x ?]`) and pass the
    // pointers + n=0 so the runtime never dereferences them; the
    // per-entry loop simply doesn't execute.
    let n = entries.len();
    let buf_len = n.max(1);
    let blk = ctx.block();

    // Alloca the three parallel buffers.
    let keys_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x ptr]", keys_buf, buf_len));
    let lens_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x i32]", lens_buf, buf_len));
    let vals_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x double]", vals_buf, buf_len));

    // Per-entry: store key ptr + len + value.
    for (i, entry) in entries.iter().enumerate() {
        let (key_global, key_len) = &key_globals[i];
        let idx_str = format!("{}", i);
        let blk = ctx.block();

        // keys[i] = @<key_global> as ptr
        let key_slot = blk.gep(PTR, &keys_buf, &[(I64, &idx_str)]);
        blk.store(PTR, &format!("@{}", key_global), &key_slot);

        // key_lens[i] = byte_len
        let len_slot = blk.gep(I32, &lens_buf, &[(I64, &idx_str)]);
        blk.store(I32, &format!("{}", key_len), &len_slot);

        // Materialise the value per kind. We drop the `blk` borrow so
        // each sub-emission can re-borrow ctx mutably for runtime calls
        // / declares; then re-acquire for the store.
        let val_str = match &entry.kind {
            NamespaceEntryKind::LocalVar { global_name } => {
                ctx.block().load(DOUBLE, &format!("@{}", global_name))
            }
            NamespaceEntryKind::LocalFunction { wrap_symbol } => {
                let blk = ctx.block();
                let handle = blk.call(
                    I64,
                    "js_closure_alloc_singleton",
                    &[(PTR, &format!("@{}", wrap_symbol))],
                );
                crate::expr::nanbox_pointer_inline(blk, &handle)
            }
            NamespaceEntryKind::LocalClass { class_id } => {
                // INT32-tagged class-id NaN-box: 0x7FFE_0000_0000_0000 |
                // (class_id & 0xFFFFFFFF). Matches `Expr::ClassRef`.
                let bits = crate::nanbox::INT32_TAG | (*class_id as u64 & 0xFFFF_FFFF);
                crate::nanbox::double_literal(f64::from_bits(bits))
            }
            NamespaceEntryKind::ForeignVar {
                source_prefix,
                source_local,
            } => {
                let getter = format!("perry_fn_{}__{}", source_prefix, sanitize(source_local));
                ctx.pending_declares.push((getter.clone(), DOUBLE, vec![]));
                ctx.block().call(DOUBLE, &getter, &[])
            }
            NamespaceEntryKind::ForeignFunction {
                source_prefix,
                source_local,
                param_count,
            } => {
                // Mirror the consumer-side `__perry_wrap_extern_*` pattern
                // that the top of compile_module emits when this module
                // imports the function the regular way. We can re-emit
                // it inline here (DCE removes duplicates if the same
                // import is also referenced by `import { x } from`).
                let target_name = format!("perry_fn_{}__{}", source_prefix, sanitize(source_local));
                let param_types: Vec<crate::types::LlvmType> =
                    std::iter::repeat_n(DOUBLE, *param_count).collect();
                ctx.pending_declares
                    .push((target_name.clone(), DOUBLE, param_types));
                // Singleton-allocate against the closure global for this
                // foreign function. The global is emitted by the
                // `import_function_prefixes` block if this module also
                // statically imports the function; otherwise we declare
                // it as external and rely on the source module's
                // export-side closure ptr (if any). For correctness in
                // the dynamic-import-only case, fall back to the safer
                // path: alloc a fresh per-callsite ClosureHeader via a
                // dedicated wrapper. Simplest implementation: emit our
                // own internal wrapper here under a unique name.
                let wrapper_name = format!(
                    "__perry_wrap_ns_extern_{}__{}__{}",
                    module_prefix,
                    source_prefix,
                    sanitize(source_local)
                );
                // Lazy-add the wrapper to pending_declares so we don't
                // try to define it twice if the namespace populates
                // multiple times in the same module (it shouldn't, but
                // defensive). The wrapper actually needs to be DEFINED
                // not declared — but pending_declares only declares.
                // To keep this PR focused, fall back to calling the
                // getter even for functions: returns the f64 value
                // (closure pointer if the source side emitted a
                // wrapper-returning getter; undefined otherwise). For
                // proper function-value re-export, the static-import
                // path already covers it.
                ctx.pending_declares.push((wrapper_name, DOUBLE, vec![]));
                ctx.block().call(DOUBLE, &target_name, &[])
            }
            NamespaceEntryKind::NestedNamespace { source_prefix } => ctx
                .block()
                .load(DOUBLE, &format!("@__perry_ns_{}", source_prefix)),
        };

        let blk = ctx.block();
        let val_slot = blk.gep(DOUBLE, &vals_buf, &[(I64, &idx_str)]);
        blk.store(DOUBLE, &val_str, &val_slot);
    }

    // Call `js_create_namespace(n, keys, key_lens, values)` and store
    // the result into the namespace global. The result is a NaN-boxed
    // POINTER_TAG ObjectHeader; the global is already GC-rooted by
    // `register_module_globals_as_gc_roots` is NOT — namespace globals
    // aren't in `module_globals`. Register the address as a root here
    // so the object survives subsequent GC cycles.
    let n_str = format!("{}", n);
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_create_namespace",
        &[
            (I32, &n_str),
            (PTR, &keys_buf),
            (PTR, &lens_buf),
            (PTR, &vals_buf),
        ],
    );
    let ns_name = format!("__perry_ns_{}", module_prefix);
    blk.store(DOUBLE, &result, &format!("@{}", ns_name));
    let addr_i64 = blk.ptrtoint(&format!("@{}", ns_name), I64);
    blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
}

fn scoped_fn_name(module_prefix: &str, hir_name: &str) -> String {
    format!("perry_fn_{}__{}", module_prefix, sanitize(hir_name))
}

/// Walk a function body looking for `Return(Some(expr))` shapes that
/// identify the function as a factory returning a class. Sets
/// `*produced` to the resolved class name when the first qualifying
/// return is seen; sets `*disqualified` when a return points at
/// something we can't classify as a class. Used by
/// `func_returns_class_map` fixed-point in `compile_module` to recognise
/// Effect's `Literal` / `makeLiteralClass` / `make` factories. Refs
/// #915 (gap 3 / #321 follow-up).
///
/// Recognised return shapes:
///   - `Return(Some(ClassRef(name)))` — direct class literal return.
///   - `Return(Some(Call { callee: FuncRef(other_fid), .. }))` — call
///     to another already-tagged factory (transitive).
///   - `Return(Some(Conditional { then, else, .. }))` — both branches
///     must independently resolve to the same class. Effect's
///     `Literal(...)` has this shape — the body is
///     `array_.isNonEmptyReadonlyArray(literals) ? makeLiteralClass(literals) : Never`.
///   - `Return(Some(Sequence([..., ClassRef(name)])))` — the HIR's
///     inliner sometimes collapses a factory call to
///     `Sequence([RegisterClassParentDynamic, ClassRef(name)])`. Treat
///     the trailing class as the produced value.
///
/// Anything else inside a `Return(Some(_))` disqualifies the function:
/// we'd rather miss a factory than mis-classify a non-factory.
/// Returns inside nested closures are SKIPPED — those belong to the
/// inner function (the walker doesn't recurse into Expr).
fn collect_return_class(
    stmts: &[perry_hir::Stmt],
    produced: &mut Option<String>,
    disqualified: &mut bool,
    func_returns_class: &std::collections::HashMap<u32, String>,
) {
    use perry_hir::{Expr, Stmt};

    fn resolve_class(
        expr: &perry_hir::Expr,
        func_returns_class: &std::collections::HashMap<u32, String>,
    ) -> Option<String> {
        match expr {
            Expr::ClassRef(name) => Some(name.clone()),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
                _ => None,
            },
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let lhs = resolve_class(then_expr, func_returns_class)?;
                let rhs = resolve_class(else_expr, func_returns_class)?;
                if lhs == rhs {
                    Some(lhs)
                } else {
                    None
                }
            }
            Expr::Sequence(exprs) => exprs
                .last()
                .and_then(|e| resolve_class(e, func_returns_class)),
            _ => None,
        }
    }

    for stmt in stmts {
        if *disqualified {
            return;
        }
        match stmt {
            Stmt::Return(Some(expr)) => {
                let resolved = resolve_class(expr, func_returns_class);
                match resolved {
                    Some(name) => match produced {
                        None => *produced = Some(name),
                        Some(existing) if *existing == name => {}
                        Some(_) => {
                            // Mixed return shapes — bail.
                            *disqualified = true;
                        }
                    },
                    None => {
                        *disqualified = true;
                    }
                }
            }
            Stmt::Return(None) => {
                // Returning undefined — disqualify (caller can't
                // depend on the receiver being a class).
                *disqualified = true;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_class(then_branch, produced, disqualified, func_returns_class);
                if let Some(eb) = else_branch {
                    collect_return_class(eb, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_return_class(body, produced, disqualified, func_returns_class);
                if let Some(cc) = catch {
                    collect_return_class(&cc.body, produced, disqualified, func_returns_class);
                }
                if let Some(blk) = finally {
                    collect_return_class(blk, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_return_class(&case.body, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Labeled { body, .. } => {
                let slice = std::slice::from_ref(body.as_ref());
                collect_return_class(slice, produced, disqualified, func_returns_class);
            }
            _ => {}
        }
    }
}

/// Mangle a class method name into an LLVM symbol, scoped by module
/// prefix and class name.
///
/// `perry_method_<modprefix>__<class>__<method>`.
fn scoped_method_name(module_prefix: &str, class_name: &str, method_name: &str) -> String {
    format!(
        "perry_method_{}__{}__{}",
        module_prefix,
        sanitize(class_name),
        sanitize(method_name)
    )
}

/// Sanitize a name for use in an LLVM symbol — replace anything that isn't
/// `[A-Za-z0-9_]` with an underscore. LLVM IR identifiers cannot start with
/// a digit, so prefix with `_` if the first character would be one (this
/// happens with module names like `05_fibonacci.ts`).
fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, '_');
    }
    s
}

/// Host default triple.
/// Host-default LLVM target triple. Used when `CompileOptions.target`
/// is `None`. Also re-exposed via `pub(crate)` so `linker.rs` can pin
/// clang's `-target` even on host builds — without that pin a clang
/// whose own default triple is GNU/MinGW silently overrides the IR's
/// stated msvc triple and emits a `__main` libgcc reference that
/// lld-link/link.exe can't resolve. (The bug used to surface as
/// `LNK2019: unresolved external symbol __main referenced in
/// function main` even though the .ll says `target triple =
/// "x86_64-pc-windows-msvc"`.)
pub(crate) fn default_target_triple() -> String {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu".to_string()
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc".to_string()
    } else {
        "arm64-apple-macosx15.0.0".to_string()
    }
}

/// Map a Perry `--target <name>` string to the LLVM triple used by
/// `clang -target <triple>` / `llc -mtriple=<triple>`. The short
/// names are the public `--target` surface exposed by the CLI;
/// returning `None` leaves the triple to the host default.
///
/// Supported:
///  * `ios`, `ios-simulator`           → aarch64-apple-ios
///  * `visionos`, `visionos-simulator` → arm64-apple-xros1.0{,-simulator}
///  * `watchos`                        → arm64_32-apple-watchos (ILP32)
///  * `watchos-simulator`              → arm64-apple-watchos10.0-simulator
///  * `tvos`, `tvos-simulator`         → aarch64-apple-tvos
///  * `android`                        → aarch64-unknown-linux-android
///  * `linux` (x86_64 alias)           → x86_64-unknown-linux-gnu
///  * `linux-aarch64`                  → aarch64-unknown-linux-gnu
///  * `macos` (aarch64 alias)          → arm64-apple-macosx15.0.0
///  * `macos-x86_64`                   → x86_64-apple-macosx15.0.0
///  * `windows`                        → x86_64-pc-windows-msvc
///  * anything else                    → None (use host default)
pub fn resolve_target_triple(name: &str) -> Option<String> {
    match name {
        "ios" => Some("aarch64-apple-ios".to_string()),
        "ios-simulator" => Some("arm64-apple-ios17.0-simulator".to_string()),
        "visionos" => Some("arm64-apple-xros1.0".to_string()),
        "visionos-simulator" => Some("arm64-apple-xros1.0-simulator".to_string()),
        "watchos" => Some("arm64_32-apple-watchos".to_string()),
        "watchos-simulator" => Some("arm64-apple-watchos10.0-simulator".to_string()),
        "tvos" => Some("aarch64-apple-tvos".to_string()),
        "tvos-simulator" => Some("arm64-apple-tvos17.0-simulator".to_string()),
        "harmonyos" => Some("aarch64-unknown-linux-ohos".to_string()),
        "harmonyos-simulator" => Some("x86_64-unknown-linux-ohos".to_string()),
        "android" => Some("aarch64-unknown-linux-android".to_string()),
        "linux" => Some("x86_64-unknown-linux-gnu".to_string()),
        "linux-aarch64" => Some("aarch64-unknown-linux-gnu".to_string()),
        "macos" => Some("arm64-apple-macosx15.0.0".to_string()),
        "macos-x86_64" => Some("x86_64-apple-macosx15.0.0".to_string()),
        "windows" => Some("x86_64-pc-windows-msvc".to_string()),
        _ => None,
    }
}
