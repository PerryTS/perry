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
use crate::types::{DOUBLE, I32, I64, I8, LlvmType, PTR, VOID};

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
    /// When true, `compile_module` returns the textual LLVM IR (`.ll`)
    /// as bytes instead of invoking `clang -c` to produce an object file.
    /// Used by the bitcode-link path (`PERRY_LLVM_BITCODE_LINK=1`).
    pub emit_ir_only: bool,

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
    pub imported_async_funcs: std::collections::HashSet<String>,
    /// FuncIds of locally-defined async functions in this module. Populated
    /// from `hir.functions.is_async`. Used by `is_promise_expr` to refine
    /// `let p = asyncFn();` to `Promise(_)` so subsequent `p.then(cb)`
    /// chains route through `js_promise_then`.
    pub local_async_funcs: std::collections::HashSet<u32>,
    pub type_aliases: std::collections::HashMap<String, perry_types::Type>,
    pub imported_func_param_counts: std::collections::HashMap<String, usize>,
    pub imported_func_return_types: std::collections::HashMap<String, perry_types::Type>,
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
}

/// Compile a Perry HIR module to an object file via LLVM IR.
pub fn compile_module(hir: &HirModule, opts: CompileOptions) -> Result<Vec<u8>> {
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
    let mut class_table: HashMap<String, &perry_hir::Class> = hir
        .classes
        .iter()
        .map(|c| (c.name.clone(), c))
        .collect();

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
    let mut class_ids: HashMap<String, u32> = hir
        .classes
        .iter()
        .map(|c| (c.name.clone(), c.id))
        .collect();

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
        let class_id = ic.source_class_id.unwrap_or_else(|| next_class_id + (idx as u32));
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);

        // Skip if already defined locally (local definition takes precedence).
        if class_table.contains_key(effective_name) {
            continue;
        }

        // Assign a class id for dispatch / instanceof.
        class_ids.insert(effective_name.to_string(), class_id);
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
            fields: ic.field_names.iter().enumerate().map(|(i, name)| perry_hir::ClassField {
                name: name.clone(),
                // Use the real declared type when the source-side
                // populated `field_types`; fall back to `Any` otherwise.
                // Real types let `receiver_class_name`'s `PropertyGet`
                // recursion identify chained imported-class field
                // dispatch (e.g. `vm.viewport.scroll.scrollTop`).
                ty: ic.field_types.get(i).cloned().unwrap_or(perry_types::Type::Any),
                init: None,
                is_private: false,
                is_readonly: false,
            }).collect(),
            constructor: None,
            methods: ic.method_names.iter().map(|m| perry_hir::Function {
                id: 0,
                name: m.clone(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: perry_types::Type::Any,
                body: Vec::new(),
                is_async: false,
                is_generator: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            }).collect(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            is_exported: false,
        };
        imported_class_stubs.push(stub);
    }
    // Add imported class stubs to the class_table (references into the
    // Vec we just built — the Vec lives for the remainder of compile_module).
    // Also build a map from class name → source module prefix so method
    // dispatch generates the correct cross-module symbol name.
    let mut imported_class_prefix: HashMap<String, String> = HashMap::new();
    for ic in &opts.imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        imported_class_prefix.insert(effective_name.to_string(), ic.source_prefix.clone());
    }
    for stub in &imported_class_stubs {
        class_table.entry(stub.name.clone()).or_insert(stub);
    }

    // Local async function FuncIds — populated below from `hir.functions`
    // (the per-function loop further down). Built here so the CrossModuleCtx
    // construction is complete before the FnCtx instances reference it.
    let mut local_async_funcs: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    for f in &hir.functions {
        if f.is_async {
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
        let global_name = format!(
            "perry_class_keys_{}__{}",
            module_prefix,
            sanitize(&c.name),
        );
        llmod.add_internal_global(&global_name, I64, "0");

        // Build the packed-keys string. Format: each field name
        // followed by `\0`. Parent classes contribute their fields
        // first (walking from deepest ancestor down) so the slot
        // order matches `class_field_global_index`'s assumption.
        let mut packed_keys = String::new();
        let mut total_field_count = c.fields.len() as u32;
        let mut parent_chain: Vec<String> = Vec::new();
        let mut p = c.extends_name.clone();
        while let Some(parent_name) = p {
            if let Some(parent) = hir.classes.iter().find(|cls| cls.name == parent_name) {
                parent_chain.push(parent_name.clone());
                total_field_count += parent.fields.len() as u32;
                p = parent.extends_name.clone();
            } else {
                break;
            }
        }
        // Walk from deepest ancestor to direct parent.
        for parent_name in parent_chain.iter().rev() {
            if let Some(parent) = hir.classes.iter().find(|cls| cls.name == *parent_name) {
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
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
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
        let global_name = format!(
            "perry_class_keys_{}__{}",
            module_prefix,
            sanitize(&c.name),
        );
        llmod.add_internal_global(&global_name, I64, "0");
        class_keys_globals_map.insert(c.name.clone(), global_name.clone());
        let mut packed_keys = String::new();
        for f in &c.fields {
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        class_keys_init_data.push((global_name, packed_keys, c.fields.len() as u32));
    }

    // Derive __platform__ number from target triple:
    //   0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux,
    //   5 = Web, 6 = tvOS, 7 = watchOS, 8 = visionOS, 9 = HarmonyOS
    let platform_number: f64 = {
        let t = triple.to_lowercase();
        // HarmonyOS check must precede the plain `linux` arm: the OHOS triple is
        // `*-unknown-linux-ohos`, so a naive `contains("linux")` would classify it as 4.
        if t.contains("ohos") { 9.0 }
        else if t.contains("visionos") || t.contains("xros") { 8.0 }
        else if t.contains("watchos") { 7.0 }
        else if t.contains("ios") { 1.0 }
        else if t.contains("tvos") { 6.0 }
        else if t.contains("android") { 2.0 }
        else if t.contains("windows") || t.contains("mingw") || t.contains("msvc") { 3.0 }
        else if t.contains("linux") { 4.0 }
        else if t.contains("wasm") || t.contains("emscripten") { 5.0 }
        else { 0.0 } // macOS / darwin default
    };
    // Pre-scan hir.init for compile-time constant variables. These are
    // `declare const __platform__: number` / `declare const __plugins__: number`
    // that other backends (JS, WASM) inject at build time. The LLVM backend
    // uses these to constant-fold platform checks in `lower_if`, eliminating
    // dead branches that reference extern FFI functions absent on the target.
    let mut compile_time_constants: HashMap<u32, f64> = HashMap::new();
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, name, init: None, .. } = s {
            match name.as_str() {
                "__platform__" => { compile_time_constants.insert(*id, platform_number); }
                "__plugins__" => { compile_time_constants.insert(*id, 0.0); }
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
    for cls in &hir.classes {
        for m in &cls.methods {
            method_param_counts.insert((cls.name.clone(), m.name.clone()), m.params.len());
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
        }
    }

    // Build the cross-module context bundle from CompileOptions.
    let cross_module = CrossModuleCtx {
        namespace_imports: opts.namespace_imports.iter().cloned().collect(),
        imported_async_funcs: opts.imported_async_funcs,
        local_async_funcs,
        type_aliases: opts.type_aliases,
        imported_func_param_counts: opts.imported_func_param_counts,
        imported_func_return_types: opts.imported_func_return_types,
        method_param_counts,
        class_keys_globals: class_keys_globals_map,
        imported_class_ctors: opts.imported_classes.iter().map(|ic| {
            let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
            let ctor_name = format!("{}__{}_constructor", ic.source_prefix, ic.name);
            (effective_name.to_string(), (ctor_name, ic.constructor_param_count))
        }).collect(),
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
        clamp3_functions: hir.functions.iter()
            .filter_map(|f| crate::collectors::detect_clamp3(f).map(|_| f.id))
            .collect(),
        clamp_u8_functions: hir.functions.iter()
            .filter(|f| crate::collectors::detect_clamp_u8(f))
            .map(|f| f.id)
            .collect(),
        returns_int_functions: hir.functions.iter()
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
                    id, init: Some(init), mutable: false, ..
                } = s
                {
                    if let Some((rows, cols, vals)) =
                        crate::expr::try_flat_const_2d_int(init)
                    {
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
                            map.insert(*id, crate::expr::FlatConstInfo {
                                global_name: gname,
                                rows,
                                cols,
                            });
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
            .map(|(name, params, ret)| {
                (name.clone(), (params.clone(), ret.clone()))
            })
            .collect(),
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
        let mut seen: std::collections::HashSet<perry_types::FuncId> = std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
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
                let is_also_function = hir.functions.iter().any(|f| f.is_exported && f.name == *name);
                if is_exported && !is_also_function {
                    let fn_name = format!(
                        "perry_fn_{}__{}",
                        module_prefix,
                        sanitize(name),
                    );
                    let getter = llmod.define_function(&fn_name, DOUBLE, vec![]);
                    let _ = getter.create_block("entry");
                    let blk = getter.block_mut(0).unwrap();
                    let val = blk.load(DOUBLE, &format!("@{}", global_name));
                    blk.ret(DOUBLE, &val);
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
            let name = format!(
                "perry_static_{}__{}__{}",
                module_prefix,
                sanitize(&c.name),
                sanitize(&sf.name),
            );
            llmod.add_internal_global(&name, DOUBLE, "0.0");
            static_field_globals.insert((c.name.clone(), sf.name.clone()), name);
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
        let class_prefix = imported_class_prefix
            .get(&c.name)
            .unwrap_or(&module_prefix);
        for m in &c.methods {
            method_names.insert(
                (c.name.clone(), m.name.clone()),
                scoped_method_name(class_prefix, &c.name, &m.name),
            );
        }
        // Constructor: register as a method so compile_method can find it.
        // Emitted for ALL classes (even without explicit constructors)
        // so cross-module `new` can call the constructor.
        {
            let ctor_method_name = format!("{}_constructor", c.name);
            method_names.insert(
                (c.name.clone(), ctor_method_name.clone()),
                format!("{}__{}_constructor", class_prefix, c.name),
            );
        }
        // Getters: register under the property name with a `__get_`
        // prefix to avoid colliding with a regular method of the same
        // name. The dispatch site for `obj.prop` checks the getter
        // map first, then falls back to the regular method registry.
        for (prop, f) in &c.getters {
            method_names.insert(
                (c.name.clone(), format!("__get_{}", prop)),
                scoped_method_name(class_prefix, &c.name, &format!("__get_{}", f.name)),
            );
        }
        for (prop, f) in &c.setters {
            method_names.insert(
                (c.name.clone(), format!("__set_{}", prop)),
                scoped_method_name(class_prefix, &c.name, &format!("__set_{}", f.name)),
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
                    sanitize(&c.name),
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
            let arity = ic.method_param_counts.get(method_idx).copied()
                .map(|n| n + 1)
                .unwrap_or(6);
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat(DOUBLE).take(arity).collect();
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
        let ctor_fn = format!(
            "{}__{}_constructor",
            sanitize(src),
            sanitize(&ic.name),
        );
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
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat(DOUBLE).take(6).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }
    }

    // Resolve user function names up-front so body lowering can emit
    // forward/recursive calls without worrying about emission order.
    // Names are scoped by module prefix to avoid cross-module collisions.
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool, bool)> = HashMap::new();
    for f in &hir.functions {
        func_names.insert(f.id, scoped_fn_name(&module_prefix, &f.name));
        let has_rest = f.params.iter().any(|p| p.is_rest);
        let returns_number = matches!(f.return_type, perry_types::Type::Number | perry_types::Type::Int32);
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
    let mut module_boxed_vars: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
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
        let mut seen: std::collections::HashSet<perry_types::FuncId> = std::collections::HashSet::new();
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
                    perry_hir::Stmt::Return(Some(perry_hir::Expr::LocalGet(id))) => mg.contains_key(id),
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
                    .params.iter().map(|p| (DOUBLE, format!("%arg{}", p.id))).collect();
                let wrapper = llmod.define_function(llvm_name, DOUBLE, params);
                wrapper.force_inline = true;
                let _ = wrapper.create_block("entry");
                let blk = wrapper.block_mut(0).unwrap();
                let mut i64_args: Vec<(LlvmType, String)> = Vec::new();
                for p in &f.params {
                    let i64_v = blk.fptosi(DOUBLE, &format!("%arg{}", p.id), I64);
                    i64_args.push((I64, i64_v));
                }
                let refs: Vec<(LlvmType, &str)> = i64_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
                let i64_result = blk.call(I64, &i64_name, &refs);
                let f64_result = blk.sitofp(I64, &i64_result, DOUBLE);
                blk.ret(DOUBLE, &f64_result);
                i64_specialized.insert(f.id);
            }
        }
    }

    // Lower each user function into the module (skip i64-specialized ones).
    for f in &hir.functions {
        if i64_specialized.contains(&f.id) { continue; }
        compile_function(&mut llmod, f, &func_names, &mut strings, &class_table, &method_names, &module_globals, &module_global_types, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
            .with_context(|| format!("lowering function '{}'", f.name))?;
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
            compile_method(&mut llmod, class, method, &func_names, &mut strings, &class_table, &method_names, &module_globals, &module_global_types, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;
        }
        // Getters and setters are also methods, just registered under
        // a __get_/__set_ prefix in the registry. Emit their bodies
        // with the same prefix as the LLVM function name.
        for (prop, getter_fn) in &class.getters {
            let mut renamed = getter_fn.clone();
            renamed.name = format!("__get_{}", prop);
            compile_method(&mut llmod, class, &renamed, &func_names, &mut strings, &class_table, &method_names, &module_globals, &module_global_types, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering getter '{}::{}'", class.name, prop))?;
        }
        for (prop, setter_fn) in &class.setters {
            let mut renamed = setter_fn.clone();
            renamed.name = format!("__set_{}", prop);
            compile_method(&mut llmod, class, &renamed, &func_names, &mut strings, &class_table, &method_names, &module_globals, &module_global_types, &opts.import_function_prefixes, &enum_table, &static_field_globals, &class_ids, &func_signatures, &module_boxed_vars, &closure_rest_params, &cross_module)
                .with_context(|| format!("lowering setter '{}::{}'", class.name, prop))?;
        }
        // Emit standalone constructor for cross-module use.
        // Compiled like a method: takes (i64 this, double arg0, ...) → void.
        // The constructor name matches the import declaration:
        // `<prefix>__<class>_constructor`.
        {
            let ctor_body = class.constructor.as_ref()
                .map(|c| (c.params.clone(), c.body.clone(), c.captures.clone()))
                .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new()));
            let ctor_as_method = perry_hir::Function {
                id: 0,
                name: format!("{}_constructor", class.name),
                type_params: Vec::new(),
                params: ctor_body.0,
                return_type: perry_types::Type::Void,
                body: ctor_body.1,
                is_async: false,
                is_generator: false,
                is_exported: false,
                captures: ctor_body.2,
                decorators: Vec::new(),
            };
            compile_method(
                &mut llmod, class, &ctor_as_method, &func_names, &mut strings,
                &class_table, &method_names, &module_globals, &module_global_types,
                &opts.import_function_prefixes, &enum_table,
                &static_field_globals, &class_ids, &func_signatures,
                &module_boxed_vars, &closure_rest_params, &cross_module,
            ).with_context(|| format!("lowering constructor for '{}'", class.name))?;
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
        let mut wrap_params: Vec<(LlvmType, String)> =
            vec![(I64, "%this_closure".to_string())];
        for i in 0..arity {
            wrap_params.push((DOUBLE, format!("%a{}", i)));
        }
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with just the arg doubles.
        let call_args: Vec<(LlvmType, &str)> = (0..arity)
            .map(|i| (DOUBLE, if i == 0 { "%a0" } else if i == 1 { "%a1" }
                else if i == 2 { "%a2" } else if i == 3 { "%a3" } else { "%a4" }))
            .collect();
        let result = blk.call(DOUBLE, &original_name, &call_args);
        blk.ret(DOUBLE, &result);
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
        let mut imports: Vec<(&String, &String)> =
            opts.import_function_prefixes.iter().collect();
        imports.sort_by(|a, b| a.0.cmp(b.0));
        for (name, source_prefix) in imports {
            let is_class = imported_class_names.contains(name);
            let wrapper_name =
                format!("__perry_wrap_extern_{}__{}", source_prefix, name);
            if !emitted_wrappers.insert(wrapper_name.clone()) {
                continue;
            }
            if is_class {
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
                let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                blk.ret(DOUBLE, &undef);
                let global_name =
                    format!("__perry_extern_closure_{}__{}", source_prefix, name);
                let init = format!(
                    "{{ ptr @{}, i32 0, i32 1129074515 }}",
                    wrapper_name
                );
                llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
                continue;
            }
            let target_name = format!("perry_fn_{}__{}", source_prefix, name);
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
                std::iter::repeat(DOUBLE).take(param_count).collect();
            llmod.declare_function(&target_name, DOUBLE, &param_types);
            // Wrapper: `define internal double @__perry_wrap_extern_<src>__<name>(
            //              i64 %this_closure, double %a0, …, double %aN-1)`
            // discards the closure pointer and forwards the doubles to
            // `perry_fn_<src>__<name>`.
            let mut wrap_params: Vec<(LlvmType, String)> =
                Vec::with_capacity(param_count + 1);
            wrap_params.push((I64, "%this_closure".to_string()));
            for i in 0..param_count {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrapper_name, DOUBLE, wrap_params);
            wf.linkage = "internal".to_string();
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let arg_names: Vec<String> =
                (0..param_count).map(|i| format!("%a{}", i)).collect();
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
            let global_name =
                format!("__perry_extern_closure_{}__{}", source_prefix, name);
            let init = format!(
                "{{ ptr @{}, i32 0, i32 1129074515 }}",
                wrapper_name
            );
            llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
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
        &module_prefix,
        opts.is_entry_module,
        &opts.non_entry_module_prefixes,
        &module_boxed_vars,
        &closure_rest_params,
        &cross_module,
        &opts.output_type,
    )
    .with_context(|| format!("lowering entry of module '{}'", hir.name))?;

    // After all user code is lowered, the string pool's contents are final.
    // Emit the bytes globals, handle globals, and the
    // `__perry_init_strings_<prefix>` function that runs once at startup.
    // The function name is scoped by module prefix so multiple modules
    // can each have their own string-pool init without colliding.
    emit_string_pool(&mut llmod, &strings, &module_prefix, &class_keys_init_data, &class_ids, &class_table);

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
    *CACHED.get_or_init(|| !matches!(
        std::env::var("PERRY_SHADOW_STACK").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    ))
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
    *CACHED.get_or_init(|| matches!(
        std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    ))
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
    // for vectorizing clamp helpers and similar patterns.
    if f.body.len() <= 8 && !f.is_async && !f.is_generator {
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
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
        .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
    let integer_locals = crate::collectors::collect_integer_locals(&f.body, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
    // Issue #140 gate: locals that appear in an `arr[i]` / `uint8[i]` / `arr.at(i)`
    // index subtree. Pure accumulators skip the Let-site i32 shadow so the body
    // stays a single-f64-alloca chain that LLVM's autovectorizer can widen.
    let index_used_locals = crate::collectors::collect_index_used_locals(&f.body);

    // Pre-walk: which `let x = new Class(...)` locals never escape?
    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &f.body, &boxed_vars, module_globals, classes,
    );
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        &f.body, &boxed_vars, module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body, &boxed_vars, module_globals,
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
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
        i18n: &cross_module.i18n,
        local_class_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
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
        if !is_buffer_typed { continue; }
        if ctx.boxed_vars.contains(&p.id) { continue; }
        if crate::collectors::has_any_mutation(&f.body, p.id) { continue; }
        let Some(param_slot) = ctx.locals.get(&p.id).cloned() else { continue };
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

    // Defensive: a well-typed numeric function always returns via an
    // explicit `return`, but we emit `ret double 0.0` as a fallback so
    // the LLVM verifier doesn't reject a missing terminator. For
    // async functions, the fallback also wraps in a resolved promise
    // so callers can await the result.
    if !ctx.block().is_terminated() {
        if f.is_async {
            let zero = "0.0".to_string();
            let handle = ctx.block().call(I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, "0.0");
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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
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
        } => (params, body, captures, *captures_this, enclosing_class.clone(), *is_async),
        _ => return Err(anyhow!("compile_closure: expected Expr::Closure")),
    };

    let llvm_name = format!("perry_closure_{}__{}", module_prefix, func_id);

    // Param list: i64 this_closure, then each param as double.
    let mut llvm_params: Vec<(LlvmType, String)> =
        Vec::with_capacity(params.len() + 1);
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
    let mut local_types: HashMap<u32, perry_types::Type> = params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();
    for (id, ty) in module_local_types.iter() {
        local_types.entry(*id).or_insert_with(|| ty.clone());
    }

    // Build the capture map: each captured LocalId gets the index it
    // occupies in the closure's capture array. Identical logic to the
    // `compute_auto_captures` helper used by the closure creation site
    // — they MUST agree on the slot indices, otherwise the body reads
    // captures from the wrong slots. Sorting the auto-detected ids
    // gives deterministic indexing across both call sites.
    let mut auto_captures: Vec<u32> = captures.clone();
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

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
        .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
    let integer_locals = crate::collectors::collect_integer_locals(body, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
    let index_used_locals = crate::collectors::collect_index_used_locals(body);

    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        body, &closure_boxed_vars, module_globals, classes,
    );
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        body, &closure_boxed_vars, module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        body, &closure_boxed_vars, module_globals,
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
        boxed_vars: closure_boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
        i18n: &cross_module.i18n,
        local_class_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
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
        if is_async {
            let zero = "0.0".to_string();
            let handle = ctx.block().call(I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, "0.0");
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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
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

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
        .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
    let integer_locals = crate::collectors::collect_integer_locals(&method.body, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
    let index_used_locals = crate::collectors::collect_index_used_locals(&method.body);

    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &method.body, &method_boxed_vars, module_globals, classes,
    );
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        &method.body, &method_boxed_vars, module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &method.body, &method_boxed_vars, module_globals,
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
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: method.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        boxed_vars: method_boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
        i18n: &cross_module.i18n,
        local_class_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
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
        crate::lower_call::apply_field_initializers_recursive_pub(&mut ctx, &class.name)
            .with_context(|| format!("applying field initializers for '{}' constructor", class.name))?;
    }

    stmt::lower_stmts(&mut ctx, &method.body)
        .with_context(|| format!("lowering body of method '{}::{}'", class.name, method.name))?;

    if !ctx.block().is_terminated() {
        ctx.block().ret(DOUBLE, "0.0");
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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
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
    module_prefix: &str,
    is_entry: bool,
    non_entry_module_prefixes: &[String],
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    output_type: &str,
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
            for prefix in non_entry_module_prefixes {
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
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
            .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
        let main_integer_locals = crate::collectors::collect_integer_locals(&hir.init, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
        let main_index_used_locals = crate::collectors::collect_index_used_locals(&hir.init);
        let main_non_escaping_news = crate::collectors::collect_non_escaping_news(
            &hir.init, &main_boxed_vars, module_globals, classes,
        );
        let main_non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
            &hir.init, &main_boxed_vars, module_globals,
        );
        let main_non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
            &hir.init, &main_boxed_vars, module_globals,
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
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            boxed_vars: main_boxed_vars,
            closure_rest_params: &closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
            imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
            i18n: &cross_module.i18n,
            local_class_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: &cross_module.compile_time_constants,
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
        init_static_fields(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of module '{}'", hir.name))?;

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
                ctx.block().br(&header_label);

                // loop_header: check if there's any reason to keep running
                ctx.current_block = header_idx;
                let has_timers = ctx.block().call(I32, "js_timer_has_pending", &[]);
                let has_callbacks = ctx.block().call(I32, "js_callback_timer_has_pending", &[]);
                let has_intervals = ctx.block().call(I32, "js_interval_timer_has_pending", &[]);
                let has_stdlib = ctx.block().call(I32, "js_stdlib_has_active_handles", &[]);
                let any1 = ctx.block().or(I32, &has_timers, &has_callbacks);
                let any2 = ctx.block().or(I32, &has_intervals, &has_stdlib);
                let any = ctx.block().or(I32, &any1, &any2);
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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    } else {
        let init_name = format!("{}__init", module_prefix);
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
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
            .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
        let init_integer_locals = crate::collectors::collect_integer_locals(&hir.init, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
        let init_index_used_locals = crate::collectors::collect_index_used_locals(&hir.init);
        let init_non_escaping_news = crate::collectors::collect_non_escaping_news(
            &hir.init, &init_boxed_vars, module_globals, classes,
        );
        let init_non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
            &hir.init, &init_boxed_vars, module_globals,
        );
        let init_non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
            &hir.init, &init_boxed_vars, module_globals,
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
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            boxed_vars: init_boxed_vars,
            closure_rest_params: &closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            namespace_imports: &cross_module.namespace_imports,
            imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
            imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
            i18n: &cross_module.i18n,
            local_class_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: &cross_module.compile_time_constants,
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
        init_static_fields(&mut ctx, hir)?;
        stmt::lower_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of non-entry module '{}'", hir.name))?;

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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
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
        let handle = blk.call(
            I64,
            init_fn,
            &[(PTR, &bytes_ref), (I32, &len_str)],
        );
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
            &[(I32, &cid_str), (I32, &fc_str), (PTR, &packed_ref), (I32, &len_str)],
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

    let local_types: HashMap<u32, perry_types::Type> = f
        .params
        .iter()
        .map(|p| (p.id, p.ty.clone()))
        .collect();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module.clamp3_functions
        .union(&cross_module.clamp_u8_functions).chain(cross_module.returns_int_functions.iter()).copied().collect();
    let integer_locals = crate::collectors::collect_integer_locals(&f.body, &cross_module.flat_const_arrays.keys().copied().collect(), &clamp_fn_ids);
    let index_used_locals = crate::collectors::collect_index_used_locals(&f.body);

    let static_boxed_vars = module_boxed_vars.clone();
    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &f.body, &static_boxed_vars, module_globals, classes,
    );
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        &f.body, &static_boxed_vars, module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body, &static_boxed_vars, module_globals,
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
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
            imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        boxed_vars: static_boxed_vars,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        method_param_counts: &cross_module.method_param_counts,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
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
        i18n: &cross_module.i18n,
        local_class_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
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
        if f.is_async {
            let zero = "0.0".to_string();
            let handle = ctx.block().call(I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, "0.0");
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
        llmod.add_raw_global(format!("@{} = private global [2 x i64] zeroinitializer", ic_name));
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
        let addr = ctx
            .block()
            .ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }
}

/// Initialize each class's static fields with their declared init
/// expressions. Called at the top of compile_module_entry's main /
/// __init function. The static field globals were registered in
/// compile_module — this just emits the per-field "store init value
/// to global" sequence.
fn init_static_fields(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
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
        let Some(&cid) = ctx.class_ids.get(class_name) else { continue };
        let cid_str = cid.to_string();
        let llvm_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&f.name));
        let func_ref = format!("@{}", llvm_sym);
        let blk = ctx.block();
        let func_ptr_i64 = blk.ptrtoint(&func_ref, I64);
        blk.call_void(
            registrar,
            &[
                (crate::types::I32, &cid_str),
                (I64, &func_ptr_i64),
            ],
        );
    }
    for c in &hir.classes {
        for sf in &c.static_fields {
            let key = (c.name.clone(), sf.name.clone());
            let Some(global_name) = ctx.static_field_globals.get(&key).cloned() else {
                continue;
            };
            if let Some(init_expr) = &sf.init {
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
use crate::collectors::{
    collect_closures_in_stmts, collect_let_ids, collect_ref_ids_in_stmts,
};
use crate::boxed_vars::{collect_boxed_vars, collect_let_types_in_stmts};

/// Mangle a HIR function name into an LLVM symbol, scoped by module prefix.
///
/// `perry_fn_<modprefix>__<funcname>`. The double-underscore between
/// module prefix and function name is the delimiter — picked because
/// JS identifiers can't contain `__` in user-visible code, so it can't
/// collide with sanitized user names.
fn scoped_fn_name(module_prefix: &str, hir_name: &str) -> String {
    format!("perry_fn_{}__{}", module_prefix, sanitize(hir_name))
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
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
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
