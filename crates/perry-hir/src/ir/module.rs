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
        }
    }
}
