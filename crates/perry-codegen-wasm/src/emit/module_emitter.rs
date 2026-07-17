//! `WasmModuleEmitter` struct definition plus its small accessors
//! (`new`, `intern_string`, `get_type_idx`).
//!
//! The huge `compile()` driver lives in `compile.rs`. Other helpers
//! (`compile_function`, `compile_class_method`, …) live in `function.rs`.
//!
//! Pure code-movement from `mod.rs`.

use super::*;

pub(super) struct WasmModuleEmitter {
    /// String literal table: content → (string_id, offset, length)
    pub(super) string_table: Vec<(String, u32, u32)>, // (content, offset, len)
    pub(super) string_map: BTreeMap<String, u32>, // content → string_id
    pub(super) string_data: Vec<u8>,              // packed string bytes
    /// Type section entries: (params, results)
    pub(super) types: Vec<(Vec<ValType>, Vec<ValType>)>,
    pub(super) type_map: BTreeMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// Function index mapping: FuncId → wasm function index
    pub(super) func_map: BTreeMap<FuncId, u32>,
    /// Reverse table map: wasm function index → table index
    pub(super) func_to_table_idx: BTreeMap<u32, u32>,
    /// Import count (import functions come first in the index space)
    pub(super) num_imports: u32,
    /// Runtime import indices
    pub(super) rt: Option<RuntimeImports>,
    /// Global variable mapping: GlobalId → wasm global index
    pub(super) global_map: BTreeMap<GlobalId, u32>,
    pub(super) num_globals: u32,
    /// Module-level Let bindings promoted to WASM globals: (mod_idx, LocalId) → wasm global idx.
    /// Module-level `let`/`const` declarations live in module.init as Stmt::Let, but
    /// are accessed by functions in the same module via LocalGet. They need to be
    /// stored in WASM globals so cross-function references work, and so module-init
    /// LocalIds don't collide with other modules' identical LocalIds.
    pub(super) module_let_globals: BTreeMap<(usize, LocalId), u32>,
    /// Current module index when compiling functions/methods, so LocalGet can resolve
    /// module-level Lets to the correct WASM global.
    pub(super) current_mod_idx: usize,
    /// Class constructor map: class_name → wasm function index
    pub(super) class_ctor_map: BTreeMap<String, u32>,
    /// Class method map: class_name → {method_name → wasm function index}
    pub(super) class_method_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Class static method map: class_name → {method_name → wasm function index}
    pub(super) class_static_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Function name → wasm function index (for cross-module ExternFuncRef resolution)
    pub(super) func_name_map: BTreeMap<String, u32>,
    /// FFI imports: (name, param_count, has_return) — registered as WASM imports under "ffi" namespace
    pub(super) ffi_imports: Vec<(String, usize, bool)>,
    /// Class parent map: child_class_name → parent_class_name
    pub(super) class_parent_map: BTreeMap<String, String>,
    /// Enum member values: (enum_name, member_name) → numeric value or string
    pub(super) enum_values: BTreeMap<(String, String), EnumResolvedValue>,
    /// Global index for NaN-safe temp storage (global.set/get may preserve NaN in Firefox)
    pub(super) nan_temp_global: u32,
    /// Async function names (compiled to JS, not WASM)
    pub(super) async_func_imports: Vec<(String, u32, usize)>, // (name, import_idx, param_count)
    /// Generated JS code for async functions
    pub(super) async_js_code: Vec<String>,
    /// Per-module func_map snapshots: FuncRef(id) is only unique within a module,
    /// so each module needs its own FuncId→wasm_idx mapping.
    pub(super) module_func_maps: Vec<BTreeMap<FuncId, u32>>,
    /// Set of WASM function indices that return void (no return value).
    /// Used to push TAG_UNDEFINED after calling void functions via FuncRef.
    pub(super) void_funcs: std::collections::BTreeSet<u32>,
    /// WASM function index → expected parameter count.
    /// Used to pad missing arguments with TAG_UNDEFINED for optional params.
    pub(super) func_param_counts: BTreeMap<u32, usize>,
    /// Issue #1071: cross-module imported VARIABLE resolution.
    /// Maps `(consumer_mod_idx, imported_local_name)` → WASM global index
    /// of the source module's `Stmt::Let` that backs the export. Pre-fix
    /// `Expr::ExternFuncRef { name }` for an imported `const`/`let` (rather
    /// than a function) fell through to a `TAG_UNDEFINED` constant because
    /// `func_name_map` only carries function names. With this map populated
    /// from each consumer's `Import` × source's `Export::Named` × source's
    /// module-let globals, the ExternFuncRef value path resolves to a
    /// `GlobalGet(gidx)` reading the live module-let slot, matching the
    /// LLVM target's `perry_fn_<src>__<name>()` getter path.
    pub(super) imported_var_globals: BTreeMap<(usize, String), u32>,
    /// Namespace-import member FUNCTIONS: `(consumer_module_idx, "W.fn")` →
    /// wasm function index. Companion to the dotted-key entries in
    /// `imported_var_globals`: `import * as W from "./mod"` followed by
    /// `W.fn(args)` resolves to a direct call (calls.rs), and `W.fn` as a
    /// value to a zero-capture closure (objects.rs) — the same two shapes a
    /// named import gets via ExternFuncRef.
    pub(super) imported_ns_funcs: BTreeMap<(usize, String), u32>,
    /// Named-import FUNCTIONS, per consumer: `(consumer_module_idx, local)`
    /// → wasm function index, resolved through re-export chains. Consulted
    /// BEFORE the whole-program `func_name_map`, whose bare-name keys
    /// collide the moment two modules define a same-named function (a local
    /// serializer helper `vec3(v): string` must not capture the math
    /// library's `vec3(x,y,z)` for every caller in the program).
    pub(super) imported_func_indices: BTreeMap<(usize, String), u32>,
}

impl WasmModuleEmitter {
    pub(super) fn new() -> Self {
        Self {
            string_table: Vec::new(),
            string_map: BTreeMap::new(),
            string_data: Vec::new(),
            types: Vec::new(),
            type_map: BTreeMap::new(),
            func_map: BTreeMap::new(),
            func_to_table_idx: BTreeMap::new(),
            num_imports: 0,
            rt: None,
            global_map: BTreeMap::new(),
            num_globals: 0,
            module_let_globals: BTreeMap::new(),
            current_mod_idx: 0,
            class_ctor_map: BTreeMap::new(),
            class_method_map: BTreeMap::new(),
            class_static_map: BTreeMap::new(),
            func_name_map: BTreeMap::new(),
            ffi_imports: Vec::new(),
            class_parent_map: BTreeMap::new(),
            enum_values: BTreeMap::new(),
            nan_temp_global: 0, // set during compile()
            async_func_imports: Vec::new(),
            module_func_maps: Vec::new(),
            void_funcs: std::collections::BTreeSet::new(),
            func_param_counts: BTreeMap::new(),
            async_js_code: Vec::new(),
            imported_var_globals: BTreeMap::new(),
            imported_ns_funcs: BTreeMap::new(),
            imported_func_indices: BTreeMap::new(),
        }
    }

    /// Intern a string literal, returning its string_id.
    pub(super) fn intern_string(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.string_map.get(s) {
            return id;
        }
        let id = self.string_table.len() as u32;
        let offset = self.string_data.len() as u32;
        let bytes = s.as_bytes();
        let len = bytes.len() as u32;
        self.string_data.extend_from_slice(bytes);
        self.string_table.push((s.to_string(), offset, len));
        self.string_map.insert(s.to_string(), id);
        id
    }

    /// Get or create a function type index for the given signature.
    pub(super) fn get_type_idx(&mut self, params: Vec<ValType>, results: Vec<ValType>) -> u32 {
        let key = (params.clone(), results.clone());
        if let Some(&idx) = self.type_map.get(&key) {
            return idx;
        }
        let idx = self.types.len() as u32;
        self.types.push((params, results));
        self.type_map.insert(key, idx);
        idx
    }
}
