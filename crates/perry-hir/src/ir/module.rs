//! `Module` HIR struct + constructor. Re-exported from `super`.

use super::*;
use perry_types::{FuncId, Type};

/// A complete HIR module (corresponds to one TypeScript file)
#[derive(Debug, Clone)]
pub struct Module {
    /// Module name/path
    pub name: String,
    /// Imports from other modules
    pub imports: Vec<Import>,
    /// Exports from this module
    pub exports: Vec<Export>,
    /// Class definitions
    pub classes: Vec<Class>,
    /// Interface definitions
    pub interfaces: Vec<Interface>,
    /// Type alias definitions
    pub type_aliases: Vec<TypeAlias>,
    /// Enum definitions
    pub enums: Vec<Enum>,
    /// Global variable declarations
    pub globals: Vec<Global>,
    /// Function definitions
    pub functions: Vec<Function>,
    /// #5579: names + FuncIds of bare top-level `function` declarations, in
    /// source order. For a *Script* (a non-ESM entry program), these become
    /// own properties of the global object per GlobalDeclarationInstantiation,
    /// so `Object.prototype.hasOwnProperty.call(globalThis, name)` is true.
    /// ESM modules (imports/exports/top-level await) do NOT reflect. Codegen
    /// consumes this in the entry-module branch to emit the `globalThis[name]
    /// = <fn>` stores. Only `hir.functions` carries nested closures and
    /// object-literal methods too, which must NOT be reflected — hence this
    /// dedicated list populated only at the top-level statement site.
    pub script_global_functions: Vec<(String, FuncId)>,
    /// #5579: true iff this module's source references the `globalThis`
    /// identifier. Codegen reflects `script_global_functions` onto the global
    /// object only when this holds — otherwise the reflection is unobservable
    /// (no code reads the global object's function properties) and would only
    /// add dynamic-property-helper calls to module init for pure programs. Set
    /// at lowering from the installed module source.
    pub references_global_this: bool,
    /// #5848: names of Annex B block-nested top-level `function`
    /// declarations (`{ function f(){} }`, `if (c) function f(){}`,
    /// `switch (x) { case 1: function f(){} }`, directly in sloppy global
    /// code) that need an early `undefined`-valued, non-configurable global
    /// property — GlobalDeclarationInstantiation's `CreateGlobalVarBinding`
    /// (B.3.3.2 step 5.b.i) runs before any top-level statement executes, so
    /// the property must already exist (as `undefined`) ahead of the block
    /// that later assigns the real function. Excludes names already covered
    /// by a same-named bare top-level function declaration (already
    /// reflected with its real value via `script_global_functions`). Codegen
    /// emits these unconditionally for a non-ESM entry program (not gated on
    /// `references_global_this`: the pattern is rare enough that the extra
    /// reflection call is not a meaningful per-program cost).
    pub annexb_global_undefined_names: Vec<String>,
    /// Top-level statements to execute
    pub init: Vec<Stmt>,
    /// Exported native module instances: (export_name, module_name, class_name)
    /// This tracks variables like `export const pool = new Pool(...)` from pg
    pub exported_native_instances: Vec<(String, String, String)>,
    /// Exported functions that return native module instances: (func_name, module_name, class_name)
    /// e.g., `export function getRedis(): Promise<Redis>` -> ("getRedis", "ioredis", "Redis")
    pub exported_func_return_native_instances: Vec<(String, String, String)>,
    /// Exported object literals: export_name
    /// This tracks variables like `export const config = { ... }`
    pub exported_objects: Vec<String>,
    /// Exported functions that need globals for cross-module value passing
    /// This tracks functions like `export function foo() { ... }` or `export async function bar() { ... }`
    /// that may be imported and used as values (not just called) by other modules
    pub exported_functions: Vec<(String, FuncId)>,
    /// Widget extension declarations (perry/widget)
    pub widgets: Vec<WidgetDecl>,
    /// Whether this module uses fetch() — requires perry-stdlib for js_fetch_with_options
    pub uses_fetch: bool,
    /// Whether this module references `WebAssembly.*` (issue #76). Drives
    /// auto-link of `libperry_wasm_host.a` so users don't have to remember
    /// `--enable-wasm-runtime` when they actually use the API.
    pub uses_webassembly: bool,
    /// External FFI function declarations (name, param_types, return_type)
    /// Populated from `declare function` statements with no body.
    pub extern_funcs: Vec<(String, Vec<Type>, Type)>,
    /// Set to `true` by `perry_transform::unroll_static_loops` when any
    /// for-loop in `init` got unrolled. Mirrors `Function::was_unrolled`
    /// for top-level statements (which don't belong to a Function).
    /// Image_convolution puts its blur kernel directly at module init,
    /// not inside a function, so the codegen-side channel-vector SIMD
    /// gate consults this flag for module.init lowering.
    pub init_was_unrolled: bool,
    /// Issue #100: true iff this module's top-level `init` contains an
    /// `await` expression OUTSIDE any function/closure body. Drives the
    /// deferred-import dispatch to chain the init promise rather than
    /// returning a pre-resolved namespace.
    pub has_top_level_await: bool,
    /// Issue #100: eager vs deferred init. Modules reachable from the
    /// entry over only static-import edges init at program start (Eager).
    /// Modules only reachable through dynamic `import()` init lazily on
    /// the first dispatch (Deferred). Populated during `collect_modules`
    /// after the import graph is fully built.
    pub init_kind: ModuleInitKind,
    /// Issue #1021: closure func_ids whose body has been rewritten by
    /// `transform_async_to_generator` from a plain async closure into a
    /// generator + async-step driver. `compile_closure` consults this set
    /// to decide whether the closure body is already a state machine
    /// returning a Promise (no busy-wait pump needed). Populated by the
    /// transform pass; consumed by codegen.
    pub async_step_closures: std::collections::HashSet<perry_types::FuncId>,
    /// Issue #2076: display name overrides for `fn.name`/`console.log`.
    /// Populated at lowering for two cases the binding-name registration
    /// path can't see:
    ///   • named function expressions (`const x = function f(){}` → `"f"`)
    ///   • object-literal shorthand/method properties (`{m(){}}` → `"m"`)
    /// Keyed by the closure/function's HIR FuncId; consumed by codegen
    /// when emitting `js_register_function_name` calls.
    pub closure_display_names: std::collections::HashMap<perry_types::FuncId, String>,
    /// #5592: user-visible `.name` overrides for classes whose HIR
    /// registration key differs from their JS name. The only producer
    /// today is a second anonymous class expression assigned to a binding
    /// already claimed by an earlier one (`C = class {}; C = class {}`):
    /// both infer the name `"C"`, so the second is registered under a
    /// uniquified key to get its own ClassId, and its true `.name` (`"C"`)
    /// is recorded here. Keyed by ClassId; consumed by codegen when
    /// emitting `js_register_class_name`.
    pub class_display_names: std::collections::HashMap<crate::ClassId, String>,
    /// #4101: original source text for each user function, keyed by HIR
    /// FuncId. Populated at lowering by slicing the module source against the
    /// AST span of every function declaration / expression / arrow. Consumed
    /// by codegen to emit `js_register_function_source` so `fn.toString()`
    /// (and `Function.prototype.toString.call(fn)`) reconstruct the source
    /// instead of returning the generic `"[object Object]"`.
    pub closure_source_text: std::collections::HashMap<perry_types::FuncId, String>,
    /// #3664: func_ids of `async function*` declarations and `async function*(){}`
    /// expressions. The generator transform clears `is_async`/`is_generator`
    /// before codegen, erasing the async-vs-sync distinction (both lower to a
    /// `{next,return,throw}` wrapper). This set preserves it so codegen can
    /// register async-generator closures in the runtime's dedicated async-
    /// generator registry, which drives `%AsyncGeneratorFunction%` / `%Async
    /// Generator%` intrinsic resolution. Populated by `transform_generators`.
    pub async_generator_funcs: std::collections::HashSet<perry_types::FuncId>,
    /// Number of leading parameter-prologue statements (default-param guards +
    /// destructuring binding stmts) in each generator / async-generator
    /// function body, keyed by func_id. Per spec, generator parameter binding
    /// (FunctionDeclarationInstantiation) runs *synchronously* when the
    /// generator function is called — before the generator object is created —
    /// so an iterator/RequireObjectCoercible/TDZ error during param binding
    /// throws at call time. Lowering prepends the prologue to the body; the
    /// generator transform reads this count to lift the prologue back into the
    /// outer wrapper (run-at-call) instead of state 0 of `.next()`. Absent /
    /// zero means no prologue (the common case — fully inert). Populated by
    /// lowering (`lower_fn_decl` / `lower_fn_expr`).
    pub gen_param_prologue_len: std::collections::HashMap<perry_types::FuncId, usize>,
}

impl Module {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            imports: Vec::new(),
            exports: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            type_aliases: Vec::new(),
            enums: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            script_global_functions: Vec::new(),
            references_global_this: false,
            annexb_global_undefined_names: Vec::new(),
            init: Vec::new(),
            exported_native_instances: Vec::new(),
            exported_func_return_native_instances: Vec::new(),
            exported_objects: Vec::new(),
            exported_functions: Vec::new(),
            widgets: Vec::new(),
            uses_fetch: false,
            uses_webassembly: false,
            extern_funcs: Vec::new(),
            init_was_unrolled: false,
            has_top_level_await: false,
            init_kind: ModuleInitKind::Eager,
            async_step_closures: std::collections::HashSet::new(),
            closure_display_names: std::collections::HashMap::new(),
            class_display_names: std::collections::HashMap::new(),
            closure_source_text: std::collections::HashMap::new(),
            async_generator_funcs: std::collections::HashSet::new(),
            gen_param_prologue_len: std::collections::HashMap::new(),
        }
    }
}
