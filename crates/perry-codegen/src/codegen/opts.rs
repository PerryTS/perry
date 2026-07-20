//! Public configuration and per-module options consumed by `compile_module`,
//! plus the internal `CrossModuleCtx` it folds them into.
//!
//! Split out of `codegen.rs` (now `codegen/mod.rs`) to keep that file small.
//! All types here are re-exported from `crate::codegen` so the public path
//! (`perry_codegen::AppMetadata`, etc.) is unchanged.

/// Per-application metadata read from `perry.toml` by the CLI and baked into
/// compile-time system APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMetadata {
    pub version: String,
    pub build_number: i64,
    pub bundle_id: String,
    /// iOS / macOS App Group suite name from `[ios] app_group` (or
    /// `[macos] app_group`) in perry.toml. Baked into the entry module's
    /// `main` prelude as a `perry_app_group_init(suite, len)` call so
    /// `appGroupSet/Get/Delete` can resolve `UserDefaults(suiteName:)` at
    /// runtime without re-reading the manifest. `None` means
    /// `app_group_*` calls fall through to the runtime's "not configured"
    /// stub-warn diagnostic. Refs #1178.
    pub app_group: Option<String>,
}

impl Default for AppMetadata {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            build_number: 1,
            bundle_id: "com.perry.app".to_string(),
            app_group: None,
        }
    }
}

/// Controls LLVM floating-point contraction independently from broad
/// fast-math reassociation. LLVM IR has a single `contract` FMF bit, so
/// `On` and `Fast` currently emit the same per-instruction flag while
/// remaining distinct user-facing/cache-key modes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FpContractMode {
    #[default]
    Off,
    On,
    Fast,
}

impl FpContractMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Fast => "fast",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }

    pub fn permits_contract(self) -> bool {
        !matches!(self, Self::Off)
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
    /// Issue #5621: ergonomic camelCase binding → snake_case `js_<pkg>_*`
    /// FFI symbol, for `perry.nativeLibrary` packages that expose
    /// spec-faithful camelCase exports (`requestAdapter`) over their
    /// manifest symbols (`js_webgpu_request_adapter`). `lower_call`
    /// rewrites the binding to its manifest symbol before consulting
    /// `ffi_signatures`, so the call is emitted against the real FFI
    /// symbol. Built by the CLI driver in the per-specifier import loop;
    /// empty for the byte-for-byte exact-match convention
    /// (`@perryts/storekit`'s raw ambient `js_storekit_*` exports).
    pub import_function_ffi_aliases: std::collections::HashMap<String, String>,
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
    /// Issue #5924 (companion to #680/#678): per-namespace origin-name
    /// resolution. Keyed by `(namespace_local_name, member_name)` →
    /// `origin_name`. `import_function_origin_names` is a flat map, so when
    /// two namespaces imported into the same file both have a member with
    /// the same name and only one of them is a re-export rename, the
    /// rename's origin-name override clobbers the other namespace's
    /// (correct, unrenamed) suffix — `import { Effect, Context } from
    /// "effect"` broke `Context.Service` because `Effect`'s own re-exported
    /// `Service` clobbered the flat map first.
    pub namespace_member_origin_names: std::collections::HashMap<(String, String), String>,
    /// When true, `compile_module` returns the textual LLVM IR (`.ll`)
    /// as bytes instead of invoking `clang -c` to produce an object file.
    /// Used by the bitcode-link path (`PERRY_LLVM_BITCODE_LINK=1`).
    pub emit_ir_only: bool,
    /// Run the native-representation verifier after lowering. This is
    /// intentionally explicit because it checks compiler invariants and must
    /// force real lowering rather than accepting cached object bytes.
    pub verify_native_regions: bool,
    /// Disable native Buffer/Uint8Array direct-load/store lowering. This is a
    /// benchmarking/debug switch; callers fall back to the generic buffer
    /// helpers because buffer access lowering returns `None`.
    pub disable_buffer_fast_path: bool,

    // ── Cross-module import plumbing ──
    /// Locals that are namespace imports (`import * as X from "./mod"`).
    /// Codegen uses this to know that `X.foo()` should be dispatched as
    /// a cross-module call rather than an object method call.
    pub namespace_imports: Vec<String>,
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
    /// #1816 — imported function names whose trailing param is the synthesized
    /// `arguments` rest. The cross-module call bundles ALL args into it (not
    /// just trailing), matching `arguments.length` semantics.
    pub imported_func_synthetic_arguments: std::collections::HashSet<String>,
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
    /// Native library FFI from `package.json` — function name, typed
    /// parameter descriptors, and typed return descriptor.
    pub native_library_functions: Vec<(
        String,
        Vec<perry_api_manifest::NativeAbiType>,
        perry_api_manifest::NativeAbiType,
    )>,
    /// i18n translation table snapshot — `(translations, key_count,
    /// locale_count, locale_codes, default_locale_idx, currencies)`. The
    /// `default_locale_idx` is the row index used at compile time to
    /// resolve `Expr::I18nString` to the right translation — without
    /// it, the lowering would have to either pick locale 0 blindly or
    /// fall back to the verbatim key. `currencies` is the sorted
    /// `[i18n.currencies]` map (`locale → ISO 4217 code`) baked into the
    /// entry `main` prelude's `perry_i18n_set_currencies` call.
    /// Tier 4.6 (v0.5.336): wrapped in `Arc` so the per-module clone
    /// in the `compile_module` rayon worker is a cheap reference bump
    /// instead of duplicating the (potentially large) `Vec<String>` of
    /// every translated string.
    #[allow(clippy::type_complexity)]
    pub i18n_table: Option<
        std::sync::Arc<(
            Vec<String>,
            usize,
            usize,
            Vec<String>,
            usize,
            Vec<(String, String)>,
        )>,
    >,

    /// When true, emit LLVM `reassoc` per-instruction fast-math flags on
    /// every f64 op. Off by default — Perry produces bit-exact output
    /// with Node's f64 arithmetic. On (via `--fast-math`,
    /// `PERRY_FAST_MATH=1`, or `perry.fastMath: true` in package.json),
    /// the optimizer is permitted to reassociate FP chains, producing
    /// observable 1-ULP differences from Node in exchange for better
    /// vectorization of tight reductions. See `fp_contract_mode` for the
    /// independent FMA contraction knob. Included in the object cache key.
    pub fast_math: bool,
    /// Explicit floating-point contraction mode from `--fp-contract`.
    /// Off preserves independent multiply/add rounding. On/Fast permit
    /// LLVM `contract` on f64 instructions so FMA-shaped code may lower
    /// to fused multiply-add instructions without also enabling
    /// reassociation. Included in the object cache key.
    pub fp_contract_mode: FpContractMode,
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

    /// Next.js wall 54 (part 2): `(absolute_source_path, sanitized_prefix)` for
    /// every Deferred `.next/server/**` module. The entry's `main` emits a
    /// `js_register_path_init(path, &<prefix>__init)` for each so a runtime
    /// `require(absolutePath)` can lazily trigger the module's init. Only
    /// populated for the entry module; empty otherwise.
    pub nextjs_path_init_modules: Vec<(String, String)>,

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

    /// #5247: emit source-location tracking for the dynamic call-dispatch
    /// throw path (`X is not a function`). Gated by the CLI `--debug-symbols`
    /// flag; default `false` so release builds are byte-identical and incur
    /// no per-call overhead. When `true` (and `module_source` is present),
    /// each dynamic method-call dispatch is preceded by a
    /// `js_set_call_location(file, line)` so the thrown TypeError's `.stack`
    /// shows `at <file>:<line>`.
    pub debug_locations: bool,
    /// #5247: this module's original source text, used at codegen to resolve a
    /// `Call`'s `byte_offset` to a 1-based line number. Only set when
    /// `debug_locations` is on (avoids cloning source for every module in the
    /// common build). `None` falls back to the `<anonymous>` frame.
    pub module_source: Option<String>,
    /// #5247 (CJS-wrap coordinate skew): for a CommonJS module rewritten by
    /// `cjs_wrap`, `module_source` is the WRAPPED text and `byte_offset`s are in
    /// wrapped coordinates. This is the number of newlines the injected wrapper
    /// prefix added before the original body; codegen subtracts it from the
    /// wrapped line so the rendered location is in original-source coordinates.
    /// `0` for non-wrapped modules and the entire default build (offsets inside
    /// the preamble — wrapped line `<=` this — resolve to no location).
    pub debug_source_line_offset: u32,
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
    /// Whether the source class declared its own constructor body.
    pub has_own_constructor: bool,
    /// Whether the source class's constructor's last declared parameter is
    /// `...rest`. Symmetric to `method_has_rest` but for the constructor: the
    /// source module compiled `<class>_constructor(this, arg0, …)` expecting
    /// the rest slot to receive a PACKED ARRAY of the trailing args. Without
    /// this flag the cross-module `new C(a, b, c)` dispatch passed the args
    /// positionally, so `arg0 = a` (raw) and `b`/`c` were dropped — a
    /// `constructor(...args)` saw `args = a`, length 1.
    pub constructor_has_rest: bool,
    /// Whether the source class has instance fields that require initializer replay.
    pub has_instance_fields: bool,
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

/// Constructor metadata for a class imported from another module.
#[derive(Debug, Clone)]
pub(crate) struct ImportedCtor {
    pub symbol: String,
    pub param_count: usize,
    pub has_own_constructor: bool,
    pub has_instance_fields: bool,
    /// True when the constructor's last declared param is `...rest`. Tells
    /// the cross-module `new` dispatch to pack the trailing args into an
    /// array for the rest slot rather than passing them positionally.
    pub has_rest: bool,
}

impl ImportedCtor {
    /// True when constructor resolution must stop at this imported class even
    /// when its standalone constructor takes zero user parameters.
    pub(crate) fn stops_constructor_walk(&self) -> bool {
        self.param_count > 0 || self.has_own_constructor || self.has_instance_fields
    }
}

/// Cross-module import context, bundled into a single struct to avoid
/// adding five more individual parameters to every compile_* function.
/// Built once in `compile_module` from `CompileOptions`.
pub(crate) struct CrossModuleCtx {
    pub namespace_imports: std::collections::HashSet<String>,
    /// Issue #680: per-namespace member resolution. See doc on
    /// `CompileOptions::namespace_member_prefixes`.
    pub namespace_member_prefixes: std::collections::HashMap<(String, String), String>,
    /// Issue #5924: per-namespace origin-name resolution. See doc on
    /// `CompileOptions::namespace_member_origin_names`.
    pub namespace_member_origin_names: std::collections::HashMap<(String, String), String>,
    pub imported_async_funcs: std::collections::HashSet<String>,
    /// FuncIds of locally-defined async functions in this module. Populated
    /// from `hir.functions.is_async`. Used by `is_promise_expr` to refine
    /// `let p = asyncFn();` to `Promise(_)` so subsequent `p.then(cb)`
    /// chains route through `js_promise_then`.
    pub local_async_funcs: std::collections::HashSet<u32>,
    /// FuncIds of locally-defined generator functions after generator lowering.
    /// `Function.is_generator` is cleared by the transform, so this is built
    /// from the lowered iterator-return body shape and used by call lowering
    /// to attach instances to the closure-owned `g.prototype`.
    pub local_generator_funcs: std::collections::HashSet<u32>,
    /// FuncIds of closures that were `async` in the source and got CPS-
    /// rewritten into async-step state machines. The rewrite clears
    /// `Expr::Closure.is_async`, so the flag alone can't identify them at
    /// codegen time. Copied from `hir.async_step_closures`. Used by the
    /// perry/thread worker-closure safety check (#6185): an async worker
    /// closure's await machinery would drain the process-global
    /// completion/timer queues on the worker thread and alias the main
    /// thread's heap.
    pub async_step_closures: std::collections::HashSet<u32>,
    /// FuncIds of locally-defined plain functions whose body reads the
    /// dynamic `this` binding (directly or via a this-capturing arrow).
    /// Bare `f()` call sites to these must reset the runtime IMPLICIT_THIS
    /// slot to `undefined` for the duration of the call so the callee's
    /// sloppy/strict `this` resolution sees "no receiver" instead of a
    /// leaked receiver from an enclosing method dispatch (#3576).
    pub funcs_reading_dynamic_this: std::collections::HashSet<u32>,
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
    /// #1816 — imported function names whose trailing param is the synthesized
    /// `arguments` rest. The cross-module call bundles ALL args into it (not
    /// just trailing), matching `arguments.length` semantics.
    pub imported_func_synthetic_arguments: std::collections::HashSet<String>,
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
    /// Issue #26 / #321: authoritative total inline-field count per class,
    /// computed by the same source-prefix-disambiguated chain walk that
    /// builds `class_keys_globals`. `lower_new` consults this so its
    /// allocation size + header `field_count` match the keys-array length,
    /// instead of recomputing via the name-keyed `ctx.classes` walk (which
    /// mis-resolves same-named cross-module parents like effect's `Type`).
    pub class_field_counts: std::collections::HashMap<String, u32>,
    /// Issue #26 / #321: authoritative, source-prefix-disambiguated ancestor
    /// chain per class (root → leaf, `(class_name, fields)`), matching the
    /// keys-global layout. `apply_field_initializers_recursive` walks this
    /// instead of the name-keyed `ctx.classes` chain, so constructor
    /// field-init writes exactly the inherited fields the keys array
    /// describes — not a colliding same-named cross-module parent's fields.
    pub class_init_chains:
        std::collections::HashMap<String, Vec<(String, Vec<perry_hir::ClassField>)>>,
    /// Imported class constructor function names. Maps class_name →
    /// full constructor symbol (e.g. "Editor" → "hone_editor_...__Editor_constructor").
    /// Populated from `opts.imported_classes`.
    pub imported_class_ctors: std::collections::HashMap<String, ImportedCtor>,
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
    /// Compile-time constant values for module globals. Maps LocalId → f64
    /// for variables like `__platform__` whose value is known at compile time.
    /// Used by `lower_if` to constant-fold platform checks and skip emitting
    /// dead branches (which may reference FFI functions that don't exist on
    /// the current target).
    pub compile_time_constants: std::collections::HashMap<u32, f64>,
    /// Effective LLVM target triple for this compile. Expression lowering uses
    /// this for narrow Node surface differences that are platform-dependent
    /// even when the manifest entry is platform-neutral.
    pub target_triple: String,
    /// App metadata backing compile-time `perry/system` introspection APIs.
    pub app_metadata: AppMetadata,
    /// Issue #5872: module-wide record of the prototype mutations that can
    /// change what `obj.method` resolves to. Scalar replacement consults this
    /// before it lets a summarized method call keep its receiver off the heap —
    /// the mutation usually lives in a *different* function (or a constructor,
    /// or a field initializer) than the `new`, so a per-function walk misses it.
    pub module_dispatch: crate::collectors::ModuleDispatchFacts,
    /// Functions with a 3-param clamp pattern: fid → true. Call sites
    /// emit `@llvm.smax.i32` + `@llvm.smin.i32` instead of a function call.
    pub clamp3_functions: std::collections::HashSet<u32>,
    /// Functions with clampU8 pattern (1 param, clamp to [0, 255]).
    pub clamp_u8_functions: std::collections::HashSet<u32>,
    /// Functions that always return integer (all returns end with `| 0` etc).
    pub returns_int_functions: std::collections::HashSet<u32>,
    /// Single-argument integer helpers that return the argument coerced to i32.
    pub i32_identity_functions: std::collections::HashSet<u32>,
    /// User functions that have a generated internal typed-f64 clone. The
    /// public wrapper keeps the JSValue ABI; direct numeric call sites may call
    /// the clone.
    pub typed_f64_functions: std::collections::HashSet<u32>,
    /// User functions that have a generated internal typed-i32 clone. The
    /// public wrapper keeps the JSValue ABI; direct call sites may call the
    /// clone when every argument is proven and guarded as Int32-compatible.
    pub typed_i32_functions: std::collections::HashSet<u32>,
    /// User functions that have a generated internal typed-i1 clone. The public
    /// wrapper keeps the JSValue ABI; direct call sites may call the clone when
    /// the caller can prove every argument matches the clone's native
    /// parameter reps.
    pub typed_i1_functions: std::collections::HashSet<u32>,
    /// User functions that have a generated internal typed-string clone. The
    /// public wrapper keeps the JSValue ABI; the clone passes raw
    /// `StringHeader*` handles as i64 and boxes only at the boundary.
    pub typed_string_functions: std::collections::HashSet<u32>,
    /// Per-function typed-i1 clone parameter reps. This lets same-module direct
    /// calls target mixed native predicate clones such as
    /// `i1(double, double)` without routing through the public JSValue wrapper.
    pub typed_i1_function_param_reps:
        std::collections::HashMap<u32, Vec<super::typed_abi::TypedParamRep>>,
    /// Own instance methods that have a generated internal typed-f64 clone.
    /// Runtime vtables still register only the generic method symbols; direct
    /// same-module call lowering may select these clones after receiver/method
    /// and numeric argument guards pass.
    pub typed_f64_methods: std::collections::HashSet<(String, String)>,
    /// Own instance methods that have a generated internal typed-i32 clone.
    /// Public method symbols remain JSValue trampolines; exact own-method
    /// direct calls may select these clones after Int32 argument guards pass.
    pub typed_i32_methods: std::collections::HashSet<(String, String)>,
    /// Own instance methods that have a generated internal typed-i1 clone.
    /// Runtime vtables still register only the generic method symbols; exact
    /// own-method direct calls may select these clones after receiver/method
    /// and per-representation typed argument guards pass.
    pub typed_i1_methods: std::collections::HashSet<(String, String)>,
    /// Own instance methods that have a generated internal typed-string clone.
    /// Public method symbols remain JSValue trampolines; exact own-method
    /// direct calls may select these clones after receiver/method and string
    /// argument guards pass.
    pub typed_string_methods: std::collections::HashSet<(String, String)>,
    /// Per-method typed-i1 clone parameter reps. This lets exact same-module
    /// method calls target mixed native predicate clones such as
    /// `i1(double, double)` without routing through the public JSValue wrapper.
    pub typed_i1_method_param_reps:
        std::collections::HashMap<(String, String), Vec<super::typed_abi::TypedParamRep>>,
    /// Own instance methods whose body reads raw numeric fields from the exact
    /// receiver and has a generated `typed_f64_recv` clone. Call sites must
    /// prove both method identity and every raw-f64 receiver-field layout before
    /// calling the clone.
    pub typed_f64_receiver_methods:
        std::collections::HashMap<(String, String), super::typed_abi::TypedReceiverMethodInfo>,
    /// Inline closure bodies that have a generated internal typed-f64 clone.
    /// Only statically-known local closure calls may select these clones after
    /// closure identity/arity and numeric argument guards pass.
    pub typed_f64_closures: std::collections::HashSet<u32>,
    /// Inline closure bodies that have a generated internal typed-i32 clone.
    /// Only statically-known local closure calls may select these clones after
    /// closure identity/arity and Int32 argument guards pass.
    pub typed_i32_closures: std::collections::HashSet<u32>,
    /// Inline closure bodies that have a generated internal typed-i1 clone.
    /// Only statically-known local closure calls may select these clones after
    /// closure identity/arity and per-representation argument guards pass.
    pub typed_i1_closures: std::collections::HashSet<u32>,
    /// Inline closure bodies that have a generated internal typed-string clone.
    /// Only statically-known local closure calls may select these clones after
    /// closure identity/arity, string argument guards, and any required string
    /// capture guards pass.
    pub typed_string_closures: std::collections::HashSet<u32>,
    /// Number of immutable string captures consumed by each typed-string
    /// closure clone. Direct local call sites use this to guard capture slots
    /// before entering the raw string ABI.
    pub typed_string_closure_capture_counts: std::collections::HashMap<u32, usize>,
    /// Per-closure typed-i1 clone parameter reps. This lets direct local
    /// closure calls target mixed native predicate clones such as
    /// `i1(i64 closure, double, double)` without routing through the public
    /// JSValue wrapper.
    pub typed_i1_closure_param_reps:
        std::collections::HashMap<u32, Vec<super::typed_abi::TypedParamRep>>,
    /// Compiler-generated async/generator control locals that can use
    /// primitive heap cells while preserving closure-shared lifetime.
    pub compiler_private_async_i32_control_locals: std::collections::HashSet<u32>,
    pub compiler_private_async_i1_control_locals: std::collections::HashSet<u32>,
    /// Debug/benchmark switch that forces Buffer/Uint8Array accesses through
    /// the generic helper path.
    pub disable_buffer_fast_path: bool,
    /// #6405: set when this module assigns a Buffer numeric read-method name as
    /// an own property (`buf.readUInt8 = fn`). Threaded into every FnCtx so the
    /// inline read intrinsic deopts to own-prop-aware runtime dispatch.
    pub program_shadows_buffer_read_method: bool,
    /// (Issue #50) Module-level `const` 2D int arrays folded into flat
    /// `[N x i32]` LLVM constants. Maps local_id → info. Populated by
    /// scanning `hir.init`; threaded through every FnCtx so the IndexGet
    /// lowering can intercept `X[i][j]` / `krow[j]` patterns.
    pub flat_const_arrays: std::collections::HashMap<u32, crate::expr::FlatConstInfo>,
    /// FFI manifest signatures from `package.json`'s `nativeLibrary.functions`.
    /// Maps function name → (param descriptors, return descriptor). Without this map,
    /// `lower_call` falls back to a heuristic that puts all numeric args/returns
    /// into d-registers (DOUBLE) — incorrect for handle-returning C functions
    /// like `hone_editor_create() -> *mut EditorView` whose actual ABI returns
    /// the pointer in `x0`, not `d0`. The manifest tells us when to use
    /// `i64`/`I64` so the LLVM declaration matches the platform C ABI.
    pub ffi_signatures: std::collections::HashMap<
        String,
        (
            Vec<perry_api_manifest::NativeAbiType>,
            perry_api_manifest::NativeAbiType,
        ),
    >,
    /// Issue #5621: ergonomic camelCase binding → manifest `js_<pkg>_*`
    /// symbol. `lower_call` rewrites a binding through this map before its
    /// `ffi_signatures` lookups so a camelCase native-library export
    /// (`requestAdapter`) routes to its real symbol
    /// (`js_webgpu_request_adapter`). Mirror of
    /// `CompileOptions::import_function_ffi_aliases`.
    pub ffi_aliases: std::collections::HashMap<String, String>,
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
    /// Per-module mapping: local alias → original imported export name, for
    /// named imports where the binding was renamed (`import { AsyncLocalStorage
    /// as xQ5 } from "async_hooks"` records `xQ5 -> "AsyncLocalStorage"`). Built
    /// once in `compile_module` from `hir.imports`. Lets `lower_new` recover the
    /// real export name so the built-in constructor arms in `lower_builtin_new`
    /// (keyed on the canonical name like `"AsyncLocalStorage"`) still fire when
    /// a minified bundle aliases the import. Without this, `new xQ5()` fell
    /// through to the empty-object placeholder and the instance had no
    /// `.getStore`/`.run` methods (`TypeError: getStore is not a function`).
    pub imported_class_original_names: std::collections::HashMap<String, String>,
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
    /// Next.js wall 54 (part 2): `(absolute_source_path, sanitized_prefix)` for
    /// every Deferred `.next/server/**` module — see [`CompileOptions`]. The
    /// entry's `main` emits one `js_register_path_init` per entry.
    pub nextjs_path_init_modules: Vec<(String, String)>,
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
