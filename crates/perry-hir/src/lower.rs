//! AST to HIR lowering
//!
//! Converts SWC's TypeScript AST into our HIR representation.

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use crate::ir::*;

// Tier 2.3 (v0.5.337-338): incremental extraction of `lower_expr` arms
// into `lower/expr_*.rs` sub-modules. Same pattern as Tier 2.1
// (compile.rs split) and 2.2 (ui_styling extracted from lower_call.rs).
//
// - `expr_misc.rs` (v0.5.337): 8 small variants (Cond, Await,
//   SuperProp, Update, Tpl, Seq, MetaProp, Yield).
// - `expr_function.rs` (v0.5.338): Arrow + Fn expression closures,
//   sharing the closure-capture analysis helper.
// - `expr_object.rs` (v0.5.338): Object literal lowering (479 LOC,
//   the largest single arm extracted so far).
// - `expr_member.rs` / `expr_assign.rs` / `expr_new.rs` (v0.5.339):
//   property access, assignment, and `new C()` constructor calls.
mod expr_assign;
mod expr_call;
mod expr_function;
mod expr_member;
mod expr_misc;
mod expr_new;
mod expr_object;
mod unimpl_hints;

/// Context for lowering, tracks variable bindings
pub struct LoweringContext {
    /// Counter for generating unique local IDs
    pub(crate) next_local_id: LocalId,
    /// Counter for generating unique global IDs
    pub(crate) next_global_id: GlobalId,
    /// Counter for generating unique function IDs
    pub(crate) next_func_id: FuncId,
    /// Counter for generating unique class IDs
    pub(crate) next_class_id: ClassId,
    /// Counter for generating unique enum IDs
    pub(crate) next_enum_id: EnumId,
    /// Counter for generating unique interface IDs
    pub(crate) next_interface_id: InterfaceId,
    /// Counter for generating unique type alias IDs
    pub(crate) next_type_alias_id: TypeAliasId,
    /// Current scope's local variables: name -> (id, type)
    pub(crate) locals: Vec<(String, LocalId, Type)>,
    /// Global variables: name -> (id, type)
    pub(crate) globals: Vec<(String, GlobalId, Type)>,
    /// Functions: name -> id
    pub(crate) functions: Vec<(String, FuncId)>,
    /// Function parameter defaults: func_id -> (defaults, param_local_ids)
    /// Per-function param-default info used by the call-site fill pass:
    /// `(func_id, [Option<default> per param], [LocalId per param], Option<rest_param_index>)`.
    /// The rest-param index (if any) is the position of `...rest`; the fill
    /// loop must stop before it because rest params get bundled at runtime
    /// from trailing positional args, not filled with `undefined`.
    pub(crate) func_defaults: Vec<(FuncId, Vec<Option<Expr>>, Vec<LocalId>, Option<usize>)>,
    /// Classes: name -> id
    pub(crate) classes: Vec<(String, ClassId)>,
    /// Static members of classes: class_name -> (static_field_names, static_method_names)
    pub(crate) class_statics: Vec<(String, Vec<String>, Vec<String>)>,
    /// Instance field names per class: class_name -> list of DECLARED field names (from
    /// ClassProp and parameter properties, NOT inferred from constructor body `this.x = ...`).
    /// Used by the "infer fields from ctor body" pass to skip fields inherited from parents,
    /// avoiding the creation of shadow fields that cause later index shift bugs after
    /// inheritance resolution in codegen.
    pub(crate) class_field_names: Vec<(String, Vec<String>)>,
    /// Issue #665 (sixth pass): per-class set of getter+setter property names.
    /// Used by the "infer fields from ctor body `this.x = ...`" pass to avoid
    /// mis-categorising a setter assignment as an own data field — the
    /// rate-limiter-flexible `set points(v)` / `this.points = opts.points`
    /// shape, where the bare-`this` field-detection block in
    /// `lower_class_decl` would otherwise allocate an `Object.keys`-visible
    /// `points` slot that shadows the inherited getter at runtime.
    /// Populated alongside `register_class_field_names`; looked up via
    /// `lookup_class_accessor_names` and walked across the parent chain when
    /// processing a subclass's ctor body.
    pub(crate) class_accessor_names: Vec<(String, Vec<String>)>,
    /// Issue #562: class name → `(module, class)` tuple from
    /// `native_extends`. Populated when lowering each class, consumed by
    /// `destructuring.rs` to register `let x = new SubclassOfStream()`
    /// locals under the parent stream module so subsequent
    /// `x.pipeTo(...)` / `x.getWriter()` etc. dispatch through the
    /// streams arms in `lower_call.rs`.
    pub(crate) class_native_extends: Vec<(String, String, String)>,
    /// Issue #302 (v0.5.388): instance field types per class so the
    /// for-of arm can detect `for (const [k, v] of this.someMap)` —
    /// the iterable is an `ast::Expr::Member { obj: This, prop: "someMap" }`,
    /// not an `ast::Expr::Ident`, so the existing `lookup_local_type`
    /// path doesn't apply. Parallel to `class_field_names` but stores
    /// `(field_name, declared_type)` pairs. Populated by
    /// `register_class_field_types` next to `register_class_field_names`.
    pub(crate) class_field_types: Vec<(String, Vec<(String, Type)>)>,
    /// Enums: name -> (id, members with values)
    pub(crate) enums: Vec<(String, EnumId, Vec<(String, EnumValue)>)>,
    /// Interfaces: name -> id
    pub(crate) interfaces: Vec<(String, InterfaceId)>,
    /// Type aliases: name -> (id, type_params, aliased_type)
    pub(crate) type_aliases: Vec<(String, TypeAliasId, Vec<TypeParam>, Type)>,
    /// Issue #179 typed-parse: interface name → field names in AST
    /// source order. Populated alongside `interfaces` during
    /// `lower_interface_decl`. `ObjectType::properties` is a HashMap
    /// that loses source order; this side table preserves it so
    /// `JSON.parse<Item[]>` codegen can emit a shape hint whose order
    /// matches typical `JSON.stringify` output (source order ≈
    /// insertion order ≈ what we see on the wire). Lost order would
    /// still be correct, just not fast-path friendly.
    pub(crate) interface_source_keys: std::collections::HashMap<String, Vec<String>>,
    /// Issue #179 typed-parse: interface name → resolved `ObjectType`.
    /// `resolve_typed_parse_ty` uses this so `JSON.parse<Item[]>`
    /// lowers to `Array<Object{fields}>` instead of `Array<Named("Item")>`.
    /// Without this, codegen sees only `Named` and can't extract the
    /// shape, so the specialized parse path never fires.
    pub(crate) interface_object_types: std::collections::HashMap<String, perry_types::ObjectType>,
    /// Imported functions: local_name -> original_name (the exported name in the source module)
    pub(crate) imported_functions: Vec<(String, String)>,
    /// Native module imports: local_name -> (module_name, method_name)
    /// For namespace imports (import * as x), method_name is None
    /// For named imports (import { v4 as uuid }), method_name is Some("v4")
    pub(crate) native_modules: Vec<(String, String, Option<String>)>,
    /// Built-in module aliases from require(): local_name -> module_name (e.g., "myFs" -> "fs")
    pub(crate) builtin_module_aliases: Vec<(String, String)>,
    /// Stack of type parameter scopes (for nested generics)
    pub(crate) type_param_scopes: Vec<HashSet<String>>,
    /// Native class instances: local_name -> (module_name, class_name)
    /// Tracks variables that hold instances of native module classes (e.g., EventEmitter)
    pub(crate) native_instances: Vec<(String, String, String)>,
    /// Current class being lowered (for arrow function `this` capture)
    pub(crate) current_class: Option<String>,
    /// Extern function types: name -> (param_types, return_type)
    /// Stores type information for declare function statements (FFI)
    pub(crate) extern_func_types: Vec<(String, Vec<Type>, Type)>,
    /// Source file path (for import.meta.url)
    pub(crate) source_file_path: String,
    /// Variables that hold closures or other values needing cross-module export globals
    /// (arrow functions, object literals, call expressions, arrays, new expressions)
    pub(crate) exportable_object_vars: HashSet<String>,
    /// Functions created during expression lowering (e.g., object literal methods)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_functions: Vec<Function>,
    /// Functions that return native module instances: func_name -> (module_name, class_name)
    /// Tracks user-defined functions whose return type annotation is a native module type
    /// (e.g., initializePool(): mysql.Pool -> ("mysql2/promise", "Pool"))
    pub(crate) func_return_native_instances: Vec<(String, String, String)>,
    /// Classes created during expression lowering (e.g., class expressions in `new (class extends X {})()`)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_classes: Vec<Class>,
    /// Function return types: func_name -> return_type
    /// Tracks return types of user-defined functions for call-site type inference
    pub(crate) func_return_types: Vec<(String, Type)>,
    /// Resolved types from external type checker (tsgo): byte_position -> Type
    /// Populated before lowering when --type-check is enabled
    pub resolved_types: Option<std::collections::HashMap<u32, Type>>,
    /// Module-level variable names pre-registered in the forward-declaration pass.
    /// Used to avoid duplicate define_local calls when the actual declaration is lowered.
    pub(crate) pre_registered_module_vars: HashSet<String>,
    /// LocalIds that are defined at module top level (outside any function or
    /// block). Closure `captures` referencing these IDs are filtered out at
    /// lowering time because codegen loads module-level bindings from their
    /// global data slot inside the closure body — passing them via the
    /// capture-slot would race with self-referential `const f = () => f(...)`
    /// and double-book state shared between sibling closures.
    pub(crate) module_level_ids: HashSet<LocalId>,
    /// Current function/closure nesting depth (`enter_scope` bumps this,
    /// `exit_scope` decrements). 0 == still at module top level.
    pub(crate) scope_depth: usize,
    /// Block scope nesting counter (for bare `{}`, `if`, loops, try/finally).
    /// A local only counts as module-level when both `scope_depth == 0` and
    /// `inside_block_scope == 0`; `const captured = i` inside a top-level for
    /// loop must still be per-iteration box, not a shared global slot.
    pub(crate) inside_block_scope: usize,
    /// Namespace exported variables: (namespace_name, member_name, local_id)
    /// Used to resolve Namespace.member access to module-level LocalGet
    pub(crate) namespace_vars: Vec<(String, String, LocalId)>,
    /// Current namespace being lowered (for resolving internal function calls as StaticMethodCall)
    pub(crate) current_namespace: Option<String>,
    /// Module-level native instances that survive scope exits.
    /// Used for variables assigned from native calls inside functions (e.g., `mongoClient = await MongoClient.connect(uri)`).
    pub(crate) module_native_instances: Vec<(String, String, String)>,
    /// Whether this module uses fetch() — requires perry-stdlib
    pub(crate) uses_fetch: bool,
    /// Issue #76 — set when any `WebAssembly.*` HIR variant is lowered.
    pub(crate) uses_webassembly: bool,
    pub(crate) var_hoisted_ids: HashSet<LocalId>,
    /// Shadow index: function name -> index in `functions` Vec (last entry for shadowing)
    pub(crate) functions_index: HashMap<String, usize>,
    /// Shadow index: class name -> index in `classes` Vec
    pub(crate) classes_index: HashMap<String, usize>,
    /// Shadow index: local import name -> index in `imported_functions` Vec
    pub(crate) imported_functions_index: HashMap<String, usize>,
    /// Shadow index: local alias name -> index in `builtin_module_aliases` Vec
    pub(crate) builtin_module_aliases_index: HashMap<String, usize>,
    /// Local names whose value is a `WeakRef` instance (so `x.deref()` routes to
    /// `Expr::WeakRefDeref`). Pragmatic tracking — populated when lowering
    /// `let/const x = new WeakRef(...)`. Cleared on scope exit.
    pub(crate) weakref_locals: HashSet<String>,
    /// Local names whose value is a `FinalizationRegistry` instance (so
    /// `x.register(...)` / `x.unregister(...)` route to the dedicated HIR variants).
    pub(crate) finreg_locals: HashSet<String>,
    /// Local names whose value is a `WeakMap` instance — used to route
    /// `x.set/get/has/delete` to the existing Map HIR variants and to throw
    /// on primitive keys.
    pub(crate) weakmap_locals: HashSet<String>,
    /// Local names whose value is a `WeakSet` instance.
    pub(crate) weakset_locals: HashSet<String>,
    /// Names of functions declared with `function*` — used to detect generator
    /// calls in `for...of` so the iterator protocol loop is emitted instead of
    /// the array-index loop.
    pub(crate) generator_func_names: HashSet<String>,
    /// Subset of `generator_func_names` that were `async function*`. Used by
    /// the for-of generator-call path so it can wrap `__iter.next()` in
    /// `await` (async generators always return `Promise<{value, done}>`).
    pub(crate) async_generator_func_names: HashSet<String>,
    /// Classes that define `*[Symbol.iterator]()`. Maps class name →
    /// `FuncId` of the synthesized top-level generator function that
    /// takes `this` as its first parameter. Consumed by `for...of` to
    /// dispatch through the iterator protocol via a direct FuncRef call.
    pub(crate) iterator_func_for_class: std::collections::HashMap<String, perry_types::FuncId>,
    /// Local names whose value was assigned from `regex.exec(...)`. Used to
    /// route `local.index` / `local.groups` to the bare RegExpExecIndex/Groups
    /// HIR variants which read the runtime's thread-local exec metadata.
    pub(crate) regex_exec_locals: HashSet<String>,
    pub(crate) proxy_locals: HashSet<String>,
    /// Issue #76 — locals known to hold a WebAssembly instance handle (i.e.
    /// `const x = WebAssembly.instantiate(...)`). Used to route
    /// `x.exports.<method>(...)` to `Expr::WebAssemblyCallExport` only when
    /// the receiver is a tracked instance, avoiding false matches against
    /// CJS-style `module.exports.foo()` patterns.
    pub(crate) wasm_instance_locals: HashSet<String>,
    /// #809: locals whose initializer is an object literal or
    /// `Object.create(...)` — i.e. provably a plain object, never a Date.
    /// Consulted by `static_receiver_class` so `obj.toJSON()` /
    /// `obj.toString()` / `obj.valueOf()` etc. don't get rewritten to the
    /// Date intrinsics (which would read the object pointer's bits as a
    /// timestamp and print `1970-01-01T00:00:00.000Z`).
    pub(crate) plain_object_locals: HashSet<String>,
    pub(crate) proxy_revoke_locals: HashMap<String, String>,
    /// For `const p = new Proxy(ClassName, handler)`, record the class name
    /// so `new p(args)` can fold to `new ClassName(args)` (pragmatic — lets
    /// the test's construct trap see the expected value).
    pub(crate) proxy_target_classes: HashMap<String, String>,
    /// Alias map for class expressions: `const MyClass = class { ... }`
    /// binds the local `MyClass` to the synthetic class name created
    /// by `lower_class_from_ast`. The `new MyClass(...)` lowering looks
    /// up this map to resolve the alias to the real (synthetic) class
    /// name, so the New expression points at a real HIR class.
    pub(crate) class_expr_aliases: HashMap<String, String>,
    /// Mixin functions: `function withName<T>(B: Constructor<T>) { return class extends B { ... } }`.
    /// Maps mixin name → (param_name, captured class AST). Stub field
    /// added to satisfy in-tree references; full mixin support is a
    /// separate workstream.
    pub(crate) mixin_funcs: HashMap<String, (String, Box<swc_ecma_ast::Class>)>,
    /// Set to the class name when lowering inside a class constructor body.
    /// Used to resolve `new.target` to a placeholder object whose `.name`
    /// returns the class name. None outside any constructor.
    pub(crate) in_constructor_class: Option<String>,
    /// Issue #562 — set to the parent class identifier (e.g. `"WritableStream"`,
    /// `"ReadableStream"`, `"TransformStream"`, or any ident from `class X
    /// extends Y`) when lowering inside a class declaration. Used by the
    /// `super({...})` pre-scan in `expr_call.rs` to register the
    /// `start`/`pull`/`transform`/`flush` callback's controller param as
    /// a `readable_stream` native instance — same shape the
    /// `new TransformStream({...})` pre-scan in `expr_new.rs` does.
    /// Saved/restored across nested class declarations.
    pub(crate) current_class_super_ident: Option<String>,
    /// Phase 3 anon-class registry for closed-shape object literals: shape key
    /// (canonical field-name + type-tag joined) -> synthetic class name. Lets
    /// identical-shape literals within the same module share one synthesized
    /// class — shared class_id, shared keys_array global, shared direct-GEP
    /// field layout. Dedup is per-module only; cross-module dedup would need
    /// a stable hash and is deferred.
    pub(crate) anon_shape_classes: HashMap<String, String>,
    /// Counter for generating anon-class names (`__AnonShape_N`).
    pub(crate) next_anon_shape_id: u32,
    /// Phase 4.1: method return types registry keyed by (class_name,
    /// method_name). Populated as methods are lowered so call-site inference
    /// (`infer_call_return_type`'s Member arm) can resolve `obj.method()` to
    /// the method's declared or inferred return type when `obj`'s type is
    /// `Type::Named(class_name)`. Mirrors `func_return_types` but for the
    /// method-dispatch path.
    pub(crate) class_method_return_types: Vec<(String, String, Type)>,
    /// Issue #212: classes nested inside a function whose method bodies
    /// reference enclosing-scope locals. `lower_class_decl` adds hidden
    /// `__perry_cap_<id>` fields, prepends `let id = this.__perry_cap_<id>`
    /// to each capturing instance method, extends the constructor with one
    /// synthesized param per captured id, and registers the captured ids
    /// here so the `Expr::New { class_name }` lowering can append
    /// `LocalGet(id)` for each captured id at every construction site.
    pub(crate) class_captures: Vec<(String, Vec<LocalId>)>,
    /// Issue #740: `let_name → class_name` for `let/const/var <name> = <ClassRef>`
    /// initializers. Lets `Expr::New { class_name }` (where `class_name` is
    /// the source-level identifier of an alias binding) resolve to the
    /// underlying class so its `class_captures` (if any) get appended as
    /// ctor args at the `new` site. Mirrors codegen's `local_class_aliases`,
    /// but built at HIR-lowering time so the captured-arg LocalGets land in
    /// the HIR (where codegen consumes them) rather than being patched in
    /// after lowering.
    pub(crate) let_class_aliases: Vec<(String, String)>,
    /// Issue #838: locals whose initializer is `<ClassName>.prototype`.
    /// Lets the assignment lowering recognise `proto.method = fn` as a
    /// prototype-method assignment on the underlying class (rather than
    /// a write to a regular object). dayjs's minified shape is
    /// `var m = M.prototype; m.parse = function(){…}; m.init = function(){…};`
    /// — every subsequent assignment through the alias has to route to
    /// the class's prototype-method registry, otherwise the methods
    /// land in an orphaned object literal and instance reads fall back
    /// to undefined.
    pub(crate) prototype_aliases: HashMap<LocalId, String>,
    /// Issue #838 followup (b): parallel to `prototype_aliases`, but
    /// keyed by FuncId for the function-declaration variant. dayjs's
    /// minified `function M(){…}; var m = M.prototype; m.parse = …`
    /// path stores `m → fn_id` here so the assignment recogniser in
    /// `lower/expr_assign.rs` can route to
    /// `Expr::RegisterFunctionPrototypeMethod` (synthetic class id
    /// allocated at runtime against the closure's bits) instead of
    /// dropping the assignment as a no-op PropertySet.
    pub(crate) prototype_function_aliases: HashMap<LocalId, FuncId>,
    /// Issue #838 followup (b): set of locals whose initialiser is a
    /// `Closure` or `FuncRef` — i.e. local-scope bindings that hold a
    /// callable value at runtime. Babel's class-from-function emit
    /// pattern (and dayjs's minified bundle) puts the inner constructor
    /// as a nested `function M(){}` which the HIR lowers to a
    /// `Let { name: "M", init: Some(Closure{…}) }`. Subsequent
    /// `M.prototype.x = fn` or `var m = M.prototype; m.x = fn` patterns
    /// resolve `M` as a `LocalGet(M_id)` — this set is the test that
    /// gate the function-classic prototype-method route.
    pub(crate) function_valued_locals: HashSet<LocalId>,
    /// Issue #838 followup (b): parallel to `prototype_aliases` /
    /// `prototype_function_aliases`, but for the local-scope shape
    /// (`var m = M.prototype` where `M` is a `Let M = Closure{…}`).
    /// Stores `m_id → M_id` so the assignment recogniser can emit
    /// `RegisterFunctionPrototypeMethod { func: LocalGet(M_id), … }`
    /// — codegen then dispatches through the same singleton-closure
    /// path the matching `new M(args)` site uses.
    pub(crate) prototype_function_locals: HashMap<LocalId, LocalId>,
    /// Issue #886: locals whose initializer is `Object.<staticMethod>`
    /// (e.g. `const __defProp = Object.defineProperty;`). esbuild's
    /// CJS-bundle prelude aliases the static method to a short local
    /// and calls it indirectly (`__defProp(target, name, descriptor)`).
    /// Pre-fix the indirect call fell through to a generic
    /// `LocalGet(__defProp)(...)` dispatch where the captured value
    /// resolves to undefined (the recogniser only fires on the literal
    /// `Object.<method>` AST shape at the call site) and throws
    /// `TypeError: value is not a function`. Stores `id → "defineProperty"`
    /// etc. so the call lowering can synthesize the matching dedicated
    /// HIR variant when the callee is `LocalGet(id)`. Only populated
    /// for methods that already have a dedicated recogniser arm — see
    /// `lower/expr_call.rs` for the dispatch list.
    pub(crate) object_static_method_aliases: HashMap<LocalId, String>,
    /// Issue #444: true when this module is the user-supplied entry file.
    /// Drives `import.meta.main` — Node 24+ / Bun semantics where the entry
    /// module reports `true` and every imported module reports `false`. Set
    /// by `lower_module_with_class_id_types_seed_and_entry`; default false.
    pub(crate) is_entry_module: bool,
    /// Issue #668: true when this module was reached via an npm-package import
    /// (file lives under a `node_modules/` segment in either the canonical or
    /// the un-canonical resolution path). External libraries are exempt from
    /// the user-facing `require(literal)` compile error in `lower_call.rs`,
    /// preserving the legacy fall-through behavior — packages like
    /// `@perryts/redis` deliberately use `require(literal)` to defer
    /// cycle-breaking imports inside method bodies that may never execute.
    pub(crate) is_external_module: bool,
}

impl LoweringContext {
    pub fn new(source_file_path: impl Into<String>) -> Self {
        Self::with_class_id_start(source_file_path, 1)
    }

    pub fn with_class_id_start(
        source_file_path: impl Into<String>,
        start_class_id: ClassId,
    ) -> Self {
        Self {
            next_local_id: 0,
            next_global_id: 0,
            next_func_id: 0,
            next_class_id: start_class_id, // Start from the provided ID to avoid collisions across modules
            next_enum_id: 0,
            next_interface_id: 0,
            next_type_alias_id: 0,
            locals: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            func_defaults: Vec::new(),
            classes: Vec::new(),
            class_statics: Vec::new(),
            class_field_names: Vec::new(),
            class_accessor_names: Vec::new(),
            class_native_extends: Vec::new(),
            class_field_types: Vec::new(),
            enums: Vec::new(),
            interfaces: Vec::new(),
            type_aliases: Vec::new(),
            interface_source_keys: std::collections::HashMap::new(),
            interface_object_types: std::collections::HashMap::new(),
            imported_functions: Vec::new(),
            native_modules: Vec::new(),
            builtin_module_aliases: Vec::new(),
            type_param_scopes: Vec::new(),
            native_instances: Vec::new(),
            current_class: None,
            extern_func_types: Vec::new(),
            source_file_path: source_file_path.into(),
            exportable_object_vars: HashSet::new(),
            pending_functions: Vec::new(),
            func_return_native_instances: Vec::new(),
            pending_classes: Vec::new(),
            func_return_types: Vec::new(),
            resolved_types: None,
            pre_registered_module_vars: HashSet::new(),
            module_level_ids: HashSet::new(),
            scope_depth: 0,
            inside_block_scope: 0,
            namespace_vars: Vec::new(),
            current_namespace: None,
            module_native_instances: Vec::new(),
            uses_fetch: false,
            uses_webassembly: false,
            var_hoisted_ids: HashSet::new(),
            functions_index: HashMap::new(),
            classes_index: HashMap::new(),
            imported_functions_index: HashMap::new(),
            builtin_module_aliases_index: HashMap::new(),
            weakref_locals: HashSet::new(),
            finreg_locals: HashSet::new(),
            weakmap_locals: HashSet::new(),
            weakset_locals: HashSet::new(),
            generator_func_names: HashSet::new(),
            async_generator_func_names: HashSet::new(),
            iterator_func_for_class: std::collections::HashMap::new(),
            regex_exec_locals: HashSet::new(),
            proxy_locals: HashSet::new(),
            wasm_instance_locals: HashSet::new(),
            plain_object_locals: HashSet::new(),
            proxy_revoke_locals: HashMap::new(),
            proxy_target_classes: HashMap::new(),
            class_expr_aliases: HashMap::new(),
            in_constructor_class: None,
            current_class_super_ident: None,
            mixin_funcs: HashMap::new(),
            anon_shape_classes: HashMap::new(),
            next_anon_shape_id: 0,
            class_method_return_types: Vec::new(),
            class_captures: Vec::new(),
            let_class_aliases: Vec::new(),
            prototype_aliases: HashMap::new(),
            prototype_function_aliases: HashMap::new(),
            function_valued_locals: HashSet::new(),
            prototype_function_locals: HashMap::new(),
            object_static_method_aliases: HashMap::new(),
            is_entry_module: false,
            is_external_module: false,
        }
    }

    pub(crate) fn fresh_interface(&mut self) -> InterfaceId {
        let id = self.next_interface_id;
        self.next_interface_id += 1;
        id
    }

    pub(crate) fn fresh_type_alias(&mut self) -> TypeAliasId {
        let id = self.next_type_alias_id;
        self.next_type_alias_id += 1;
        id
    }

    /// Enter a new type parameter scope (for generic function/class)
    pub(crate) fn enter_type_param_scope(&mut self, type_params: &[TypeParam]) {
        let scope: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        self.type_param_scopes.push(scope);
    }

    /// Exit the current type parameter scope
    pub(crate) fn exit_type_param_scope(&mut self) {
        self.type_param_scopes.pop();
    }

    /// Check if a name is a type parameter in the current scope
    pub(crate) fn is_type_param(&self, name: &str) -> bool {
        self.type_param_scopes
            .iter()
            .any(|scope| scope.contains(name))
    }

    /// Look up a type alias by name and return its resolved type (if found).
    /// This is used during type extraction to resolve type aliases like
    /// `type BlockTag = 'latest' | number | string` so the compiler sees
    /// the underlying Union type instead of Named("BlockTag").
    pub(crate) fn resolve_type_alias(&self, name: &str) -> Option<perry_types::Type> {
        self.type_aliases
            .iter()
            .find(|(alias_name, _, type_params, _)| alias_name == name && type_params.is_empty())
            .map(|(_, _, _, ty)| ty.clone())
    }
}

/// Issue #179 typed-parse: extract the field-name list in source
/// order from a `JSON.parse<T>` AST type argument. `T` may be:
/// - A type literal `{id: number, name: string}` — direct extraction
/// - `Array<T>` / `T[]` — recurse on element
/// - A named interface reference `Item` — resolve via ctx and re-walk
///   the interface declaration's member list
///
/// Returns None on any unresolved reference or unsupported shape. The
/// caller treats that as "no fast-path order available" and emits the
/// slow-path only (still correct, just slower).
pub(super) fn extract_typed_parse_source_order(
    ts_type: &swc_ecma_ast::TsType,
    ctx: &LoweringContext,
) -> Option<Vec<String>> {
    use swc_ecma_ast as ast;
    match ts_type {
        ast::TsType::TsArrayType(arr) => extract_typed_parse_source_order(&arr.elem_type, ctx),
        ast::TsType::TsTypeLit(lit) => {
            let mut keys = Vec::with_capacity(lit.members.len());
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        keys.push(ident.sym.to_string());
                    } else {
                        return None;
                    }
                }
            }
            if keys.is_empty() {
                None
            } else {
                Some(keys)
            }
        }
        ast::TsType::TsTypeRef(tref) => {
            // `Array<T>` — recurse on the element type argument.
            if let Some(type_params) = &tref.type_params {
                let name = match &tref.type_name {
                    ast::TsEntityName::Ident(i) => i.sym.as_ref(),
                    _ => return None,
                };
                if name == "Array" && type_params.params.len() == 1 {
                    return extract_typed_parse_source_order(&type_params.params[0], ctx);
                }
            }
            // Named interface reference — look up the source-order
            // field list recorded by `lower_interface_decl`.
            let name = match &tref.type_name {
                ast::TsEntityName::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            ctx.interface_source_keys.get(&name).cloned()
        }
        _ => None,
    }
}

/// Issue #179 typed-parse: fully resolve a `JSON.parse<T>` type argument
/// down to a structural form codegen can use (ObjectType with fields /
/// Array of object). Named/interface references are expanded via the
/// lowering context's type-alias table. Unresolvable references collapse
/// to `Type::Any` so the caller falls through to the generic parser.
pub(super) fn resolve_typed_parse_ty(ctx: &LoweringContext, ty: Type) -> Type {
    match ty {
        Type::Named(ref name) => {
            // Interface reference? Expand to ObjectType from the
            // typed-parse side table (populated by `lower_interface_decl`).
            if let Some(obj) = ctx.interface_object_types.get(name) {
                return Type::Object(obj.clone());
            }
            // Type alias? Expand and recurse.
            match ctx.resolve_type_alias(name) {
                Some(resolved) => resolve_typed_parse_ty(ctx, resolved),
                None => Type::Any,
            }
        }
        Type::Array(elem) => {
            let resolved = resolve_typed_parse_ty(ctx, *elem);
            Type::Array(Box::new(resolved))
        }
        Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            let resolved = resolve_typed_parse_ty(ctx, type_args.into_iter().next().unwrap());
            Type::Array(Box::new(resolved))
        }
        // Object/primitive/tuple types pass through unchanged.
        other => other,
    }
}

impl LoweringContext {
    pub(crate) fn fresh_local(&mut self) -> LocalId {
        let id = self.next_local_id;
        self.next_local_id += 1;
        id
    }

    pub(crate) fn fresh_func(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id += 1;
        id
    }

    /// If `ast_arg` is a bare `Boolean`, `Number`, or `String` identifier, wrap the
    /// already-lowered callback `cb` in a synthetic closure that calls the corresponding
    /// coerce expression.  Otherwise return `cb` unchanged.  This is needed because
    /// built-in constructors aren't first-class closure objects in Perry's runtime.
    pub(crate) fn maybe_wrap_builtin_callback(
        &mut self,
        cb: Expr,
        ast_arg: &swc_ecma_ast::ExprOrSpread,
    ) -> Expr {
        if let swc_ecma_ast::Expr::Ident(ident) = ast_arg.expr.as_ref() {
            let builtin = ident.sym.as_ref();
            if matches!(builtin, "Boolean" | "Number" | "String") {
                let func_id = self.fresh_func();
                let param_id = self.fresh_local();
                let coerce_body = match builtin {
                    "Boolean" => Expr::BooleanCoerce(Box::new(Expr::LocalGet(param_id))),
                    "Number" => Expr::NumberCoerce(Box::new(Expr::LocalGet(param_id))),
                    "String" => Expr::StringCoerce(Box::new(Expr::LocalGet(param_id))),
                    _ => unreachable!(),
                };
                return Expr::Closure {
                    func_id,
                    params: vec![Param {
                        id: param_id,
                        name: "__x".to_string(),
                        ty: Type::Any,
                        default: None,
                        decorators: Vec::new(),
                        is_rest: false,
                    }],
                    return_type: Type::Any,
                    body: vec![Stmt::Return(Some(coerce_body))],
                    captures: vec![],
                    mutable_captures: vec![],
                    captures_this: false,
                    enclosing_class: None,
                    is_async: false,
                };
            }
        }
        cb
    }

    pub(crate) fn fresh_class(&mut self) -> ClassId {
        let id = self.next_class_id;
        self.next_class_id += 1;
        id
    }

    pub(crate) fn fresh_enum(&mut self) -> EnumId {
        let id = self.next_enum_id;
        self.next_enum_id += 1;
        id
    }

    pub(crate) fn lookup_class(&self, name: &str) -> Option<ClassId> {
        self.classes_index.get(name).map(|&idx| self.classes[idx].1)
    }

    /// Issue #562: look up the `(module, class)` tuple from a class's
    /// `native_extends` clause (e.g. `class X extends WritableStream` →
    /// `Some(("writable_stream", "WritableStream"))`). Used by
    /// `destructuring.rs`'s `let x = new SubclassOfStream()` arm to
    /// route the local through the parent stream module's dispatch
    /// table.
    pub(crate) fn lookup_class_native_extends(&self, name: &str) -> Option<(&str, &str)> {
        self.class_native_extends
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, m, c)| (m.as_str(), c.as_str()))
    }

    /// Companion setter — populated when `lower_class_decl` /
    /// `lower_class_from_ast` sees a class with `native_extends` set.
    pub(crate) fn register_class_native_extends(
        &mut self,
        class_name: String,
        module: String,
        class: String,
    ) {
        if let Some(entry) = self
            .class_native_extends
            .iter_mut()
            .find(|(n, _, _)| *n == class_name)
        {
            entry.1 = module;
            entry.2 = class;
        } else {
            self.class_native_extends.push((class_name, module, class));
        }
    }

    /// Register declared instance field names for a class. Used by subclasses to skip
    /// re-declaring inherited fields when inferring from ctor body `this.x = ...` assignments.
    pub(crate) fn register_class_field_names(
        &mut self,
        class_name: String,
        field_names: Vec<String>,
    ) {
        // Replace existing entry if present; otherwise append.
        if let Some(entry) = self
            .class_field_names
            .iter_mut()
            .find(|(n, _)| *n == class_name)
        {
            entry.1 = field_names;
        } else {
            self.class_field_names.push((class_name, field_names));
        }
    }

    /// Look up the list of instance field names declared on a class (NOT including inherited).
    pub(crate) fn lookup_class_field_names(&self, class_name: &str) -> Option<&[String]> {
        self.class_field_names
            .iter()
            .find(|(n, _)| n == class_name)
            .map(|(_, f)| f.as_slice())
    }

    /// Issue #665: register the getter+setter property names for a class.
    /// Mirrors `register_class_field_names`; consumed by the ctor-body
    /// field-detection pass to skip names that are accessors. Stored as the
    /// own+inherited union so a child lookup sees the full chain in one hop.
    pub(crate) fn register_class_accessor_names(
        &mut self,
        class_name: String,
        accessor_names: Vec<String>,
    ) {
        if let Some(entry) = self
            .class_accessor_names
            .iter_mut()
            .find(|(n, _)| *n == class_name)
        {
            entry.1 = accessor_names;
        } else {
            self.class_accessor_names.push((class_name, accessor_names));
        }
    }

    /// Look up the accessor (getter+setter) property names registered for a
    /// class. The stored list includes inherited accessors (mirroring how
    /// `class_field_names` stores the own+inherited union), so callers do
    /// not need to walk the parent chain themselves.
    pub(crate) fn lookup_class_accessor_names(&self, class_name: &str) -> Option<&[String]> {
        self.class_accessor_names
            .iter()
            .find(|(n, _)| n == class_name)
            .map(|(_, f)| f.as_slice())
    }

    /// Issue #302: register declared field types for a class (parallel to
    /// `register_class_field_names`). Lets the for-of lowerer recognize
    /// `for (const [k, v] of this.someMap)` patterns that hit class instance
    /// fields rather than local variables.
    pub(crate) fn register_class_field_types(
        &mut self,
        class_name: String,
        field_types: Vec<(String, Type)>,
    ) {
        if let Some(entry) = self
            .class_field_types
            .iter_mut()
            .find(|(n, _)| *n == class_name)
        {
            entry.1 = field_types;
        } else {
            self.class_field_types.push((class_name, field_types));
        }
    }

    /// Pre-seed `class_field_types` (and `class_field_names`) with cross-module
    /// class info collected from already-lowered dependencies. Lets
    /// `infer_type_from_expr` resolve `someLocal.field` where `someLocal`'s
    /// declared type is a class defined in another module. Without this,
    /// `for (const x of changeset.removes)` (where `changeset:
    /// ComponentChangeset` from another module, `removes: Set<...>`) silently
    /// iterates 0 times because the iterable's static type is unknown and the
    /// SetValues wrap is skipped. See ECS demo-simple repro / #412.
    ///
    /// Only inserts entries that aren't already registered locally — the
    /// current module's own classes always win.
    pub fn seed_imported_class_fields(
        &mut self,
        seeds: &std::collections::HashMap<String, Vec<(String, Type)>>,
    ) {
        for (name, fields) in seeds {
            if !self.class_field_types.iter().any(|(n, _)| n == name) {
                self.class_field_types.push((name.clone(), fields.clone()));
            }
            if !self.class_field_names.iter().any(|(n, _)| n == name) {
                let names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                self.class_field_names.push((name.clone(), names));
            }
        }
    }

    /// Issue #302: look up the declared type of a single instance field on a
    /// class. Returns `None` if the class isn't registered or the field
    /// name doesn't appear in the class's declared field list.
    pub(crate) fn lookup_class_field_type(
        &self,
        class_name: &str,
        field_name: &str,
    ) -> Option<&Type> {
        self.class_field_types
            .iter()
            .find(|(n, _)| n == class_name)
            .and_then(|(_, fs)| fs.iter().find(|(n, _)| n == field_name).map(|(_, ty)| ty))
    }

    /// Issue #212: register the outer-scope LocalIds that a nested class
    /// captures. `lower_class_decl` calls this after extending the
    /// constructor; `Expr::New { class_name }` lowering looks it up and
    /// appends `LocalGet(id)` per captured id at every construction site.
    pub(crate) fn register_class_captures(&mut self, class_name: String, captures: Vec<LocalId>) {
        if let Some(entry) = self
            .class_captures
            .iter_mut()
            .find(|(n, _)| *n == class_name)
        {
            entry.1 = captures;
        } else {
            self.class_captures.push((class_name, captures));
        }
    }

    /// Look up the captured outer-scope LocalIds for a class. Returns `None`
    /// for plain (non-capturing) classes.
    pub(crate) fn lookup_class_captures(&self, class_name: &str) -> Option<&[LocalId]> {
        self.class_captures
            .iter()
            .find(|(n, _)| n == class_name)
            .map(|(_, c)| c.as_slice())
    }

    /// Issue #740: register a `let/const/var <let_name> = <ClassRef>` alias
    /// so `Expr::New { class_name: <let_name> }` can resolve to the
    /// underlying class for capture-forwarding purposes.
    pub(crate) fn register_let_class_alias(&mut self, let_name: String, class_name: String) {
        if let Some(entry) = self
            .let_class_aliases
            .iter_mut()
            .find(|(n, _)| *n == let_name)
        {
            entry.1 = class_name;
        } else {
            self.let_class_aliases.push((let_name, class_name));
        }
    }

    /// Look up the underlying class name for a let/const/var alias. Walks
    /// the alias chain (`const B = A; const C = B` → C resolves to A's
    /// underlying class) up to a small depth to avoid runaway loops.
    pub(crate) fn resolve_class_alias(&self, name: &str) -> Option<String> {
        let mut cur = name.to_string();
        for _ in 0..8 {
            let next = self
                .let_class_aliases
                .iter()
                .find(|(n, _)| n == &cur)
                .map(|(_, c)| c.clone());
            match next {
                Some(n) if n != cur => cur = n,
                _ => break,
            }
        }
        if cur != name {
            Some(cur)
        } else {
            None
        }
    }

    pub(crate) fn register_class_statics(
        &mut self,
        class_name: String,
        static_fields: Vec<String>,
        static_methods: Vec<String>,
    ) {
        self.class_statics
            .push((class_name, static_fields, static_methods));
    }

    pub(crate) fn has_static_field(&self, class_name: &str, field_name: &str) -> bool {
        self.class_statics
            .iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, fields, _)| fields.contains(&field_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn has_static_method(&self, class_name: &str, method_name: &str) -> bool {
        self.class_statics
            .iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, _, methods)| methods.contains(&method_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn lookup_namespace_var(&self, ns_name: &str, member_name: &str) -> Option<LocalId> {
        self.namespace_vars
            .iter()
            .find(|(ns, member, _)| ns == ns_name && member == member_name)
            .map(|(_, _, id)| *id)
    }

    pub(crate) fn define_enum(
        &mut self,
        name: String,
        id: EnumId,
        members: Vec<(String, EnumValue)>,
    ) {
        self.enums.push((name, id, members));
    }

    pub(crate) fn lookup_enum(&self, name: &str) -> Option<(EnumId, &[(String, EnumValue)])> {
        self.enums
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, members)| (*id, members.as_slice()))
    }

    pub(crate) fn lookup_enum_member(
        &self,
        enum_name: &str,
        member_name: &str,
    ) -> Option<&EnumValue> {
        self.enums
            .iter()
            .find(|(n, _, _)| n == enum_name)
            .and_then(|(_, _, members)| {
                members
                    .iter()
                    .find(|(m, _)| m == member_name)
                    .map(|(_, v)| v)
            })
    }

    pub(crate) fn define_local(&mut self, name: String, ty: Type) -> LocalId {
        let id = self.fresh_local();
        // Tag as module-level only when declared outside any function AND any
        // block. `scope_depth == 0` keeps us at module top, `inside_block_scope
        // == 0` keeps us out of `{}`/if/while/for bodies (so per-iteration
        // `const captured = i` inside a top-level for loop stays per-iteration).
        if self.scope_depth == 0 && self.inside_block_scope == 0 {
            self.module_level_ids.insert(id);
        }
        self.locals.push((name, id, ty));
        id
    }

    /// Drop module-level LocalIds from a closure's `captures` list. Module-
    /// level variables are loaded directly from their global data slot inside
    /// the closure body (see `closures.rs` auto-loading pass), so passing them
    /// through the capture-slot mechanism races with the not-yet-assigned
    /// binding for `const f = () => f(...)` and stomps on state shared between
    /// sibling closures.
    pub(crate) fn filter_module_level_captures(&self, captures: Vec<LocalId>) -> Vec<LocalId> {
        captures
            .into_iter()
            .filter(|id| !self.module_level_ids.contains(id))
            .collect()
    }

    pub(crate) fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, _)| *id)
    }

    pub(crate) fn lookup_local_type(&self, name: &str) -> Option<&Type> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, _, ty)| ty)
    }

    pub(crate) fn lookup_func(&self, name: &str) -> Option<FuncId> {
        self.functions_index
            .get(name)
            .map(|&idx| self.functions[idx].1)
    }

    pub(crate) fn register_func(&mut self, name: String, id: FuncId) {
        let idx = self.functions.len();
        self.functions_index.insert(name.clone(), idx);
        self.functions.push((name, id));
    }

    pub(crate) fn register_class(&mut self, name: String, id: ClassId) {
        let idx = self.classes.len();
        self.classes_index.insert(name.clone(), idx);
        self.classes.push((name, id));
    }

    /// Phase 3: synthesize (or retrieve) an anon class for a closed-shape object
    /// literal. `fields_with_types` is parallel to the literal's source-declared
    /// properties — source order is preserved so the anon class's field layout
    /// matches JS evaluation order. Returns the synthetic class name.
    ///
    /// The synthesized class has fields with `init: None`. Each literal's
    /// values are stored via per-literal `PropertySet` statements emitted
    /// after the allocation at the Object-arm call site (wrapped in an
    /// `Expr::Sequence`). This preserves the per-literal values under
    /// shape-deduplication — earlier versions put the init values on the
    /// class itself, which meant dedup'd classes silently kept only the
    /// FIRST literal's values (every subsequent `{name:"b",…}` saw the
    /// original `{name:"a",…}` inits — broke `arr.map(x => x.name)` into
    /// `[a, a, a, a]`).
    pub(crate) fn synthesize_anon_shape_class(
        &mut self,
        fields_with_types: &[(String, Type, Expr)],
    ) -> String {
        // Canonical shape key: each field as `name:tag` joined by ',' in source
        // order. Different declaration orders -> different classes (preserves
        // JS eval order). Type tag is a coarse primitive fingerprint so two
        // literals with identical names but Number vs String fields don't
        // share a misleading class.
        fn tag(ty: &Type) -> &'static str {
            match ty {
                Type::Number => "n",
                Type::Int32 => "i",
                Type::String => "s",
                Type::Boolean => "b",
                Type::BigInt => "B",
                Type::Null => "N",
                Type::Void => "v",
                Type::Array(_) => "a",
                Type::Object(_) => "o",
                Type::Function(_) => "f",
                Type::Named(_) => "c",
                Type::Promise(_) => "p",
                _ => "?",
            }
        }
        let mut shape_key = String::new();
        for (name, ty, _) in fields_with_types {
            shape_key.push_str(name);
            shape_key.push(':');
            shape_key.push_str(tag(ty));
            shape_key.push(',');
        }

        if let Some(existing) = self.anon_shape_classes.get(&shape_key) {
            return existing.clone();
        }

        // Content-addressed name: FNV-1a hash of the canonical shape_key.
        // Same shape across different modules produces the same name, so
        // cross-module method inlining (which copies a body verbatim into
        // a sibling module) doesn't accidentally bind to a same-named but
        // different-shaped class in the destination.
        //
        // The pre-fix scheme (`__AnonShape_<per-module-counter>`) collided
        // when two modules each minted a class for their own first
        // closed-shape literal — both got `__AnonShape_0` for unrelated
        // shapes, and the inliner's body-rewrite resolved the cross-module
        // reference to the destination's local `__AnonShape_0`. Symptom:
        // a 4-field command literal in `CommandBuffer.set` round-tripped
        // as a 2-field component literal `{ x, y }`, silently dropping
        // `entityId` / `componentType` and producing 0 entities post-sync.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in shape_key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let class_name = format!("__AnonShape_{:016x}", h);
        let class_id = self.fresh_class();

        // Fields have `init: None` — each literal's values are passed as
        // positional constructor args, so the class stays shape-only (no
        // per-literal state). See the method doc comment for why this
        // matters under shape-deduplication.
        let fields: Vec<ClassField> = fields_with_types
            .iter()
            .map(|(name, ty, _init_expr_unused)| ClassField {
                name: name.clone(),
                key_expr: None,
                ty: ty.clone(),
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            })
            .collect();

        // Synthesize a constructor `(f1, f2, ...) => { this.f1 = f1; this.f2 = f2; ... }`.
        // `Expr::New { args }` at call sites passes each literal's values
        // in field-declaration order; the constructor body assigns them.
        // PropertySet's direct-GEP path fires because `this` resolves to
        // the anon class via the usual class_stack/this_stack dance in
        // lower_call.rs::lower_new.
        let mut ctor_params: Vec<Param> = Vec::with_capacity(fields_with_types.len());
        let mut ctor_body: Vec<Stmt> = Vec::with_capacity(fields_with_types.len());
        for (name, ty, _value) in fields_with_types {
            let param_id = self.fresh_local();
            ctor_params.push(Param {
                id: param_id,
                name: name.clone(),
                ty: ty.clone(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
            });
            ctor_body.push(Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: name.clone(),
                value: Box::new(Expr::LocalGet(param_id)),
            }));
        }
        let constructor = Function {
            id: self.fresh_func(),
            name: "constructor".to_string(),
            type_params: Vec::new(),
            params: ctor_params,
            return_type: Type::Void,
            body: ctor_body,
            is_async: false,
            is_generator: false,
            was_plain_async: false,
            was_unrolled: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
        };

        // Register in the name->id index so lookup_class finds it, and push to
        // pending_classes so it flushes into module.classes after the enclosing
        // statement finishes lowering (same pattern as anonymous class
        // expressions — see `ast::Expr::Class` arm in lower_expr).
        self.register_class(class_name.clone(), class_id);
        self.pending_classes.push(Class {
            id: class_id,
            name: class_name.clone(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            fields,
            constructor: Some(constructor),
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
        });

        self.anon_shape_classes
            .insert(shape_key, class_name.clone());
        class_name
    }

    pub(crate) fn lookup_func_name(&self, func_id: FuncId) -> Option<&str> {
        self.functions
            .iter()
            .find(|(_, id)| *id == func_id)
            .map(|(name, _)| name.as_str())
    }

    pub(crate) fn lookup_func_defaults(
        &self,
        func_id: FuncId,
    ) -> Option<(&[Option<Expr>], &[LocalId], Option<usize>)> {
        self.func_defaults
            .iter()
            .find(|(id, _, _, _)| *id == func_id)
            .map(|(_, defaults, param_ids, rest_idx)| {
                (defaults.as_slice(), param_ids.as_slice(), *rest_idx)
            })
    }

    /// Substitute parameter references in a default expression.
    /// Replaces LocalGet(callee_param_id) with the corresponding caller argument expression.
    pub(crate) fn substitute_param_refs_in_default(
        expr: &Expr,
        param_map: &[(LocalId, Expr)],
    ) -> Expr {
        match expr {
            Expr::LocalGet(id) => {
                // Check if this LocalGet references one of the callee's parameters
                for (param_id, replacement) in param_map {
                    if id == param_id {
                        return replacement.clone();
                    }
                }
                // Not a parameter reference - keep as-is
                expr.clone()
            }
            Expr::Array(elements) => Expr::Array(
                elements
                    .iter()
                    .map(|e| Self::substitute_param_refs_in_default(e, param_map))
                    .collect(),
            ),
            Expr::Object(fields) => Expr::Object(
                fields
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Self::substitute_param_refs_in_default(v, param_map),
                        )
                    })
                    .collect(),
            ),
            Expr::Binary { op, left, right } => Expr::Binary {
                op: *op,
                left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
            },
            Expr::Compare { op, left, right } => Expr::Compare {
                op: *op,
                left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
            },
            Expr::Logical { op, left, right } => Expr::Logical {
                op: *op,
                left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
            },
            Expr::Unary { op, operand } => Expr::Unary {
                op: *op,
                operand: Box::new(Self::substitute_param_refs_in_default(operand, param_map)),
            },
            Expr::Call {
                callee,
                args,
                type_args,
            } => Expr::Call {
                callee: Box::new(Self::substitute_param_refs_in_default(callee, param_map)),
                args: args
                    .iter()
                    .map(|a| Self::substitute_param_refs_in_default(a, param_map))
                    .collect(),
                type_args: type_args.clone(),
            },
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => Expr::Conditional {
                condition: Box::new(Self::substitute_param_refs_in_default(condition, param_map)),
                then_expr: Box::new(Self::substitute_param_refs_in_default(then_expr, param_map)),
                else_expr: Box::new(Self::substitute_param_refs_in_default(else_expr, param_map)),
            },
            Expr::PropertyGet { object, property } => Expr::PropertyGet {
                object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                property: property.clone(),
            },
            Expr::IndexGet { object, index } => Expr::IndexGet {
                object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                index: Box::new(Self::substitute_param_refs_in_default(index, param_map)),
            },
            Expr::New {
                class_name,
                args,
                type_args,
            } => Expr::New {
                class_name: class_name.clone(),
                args: args
                    .iter()
                    .map(|a| Self::substitute_param_refs_in_default(a, param_map))
                    .collect(),
                type_args: type_args.clone(),
            },
            // Leaf expressions that don't contain LocalGet - return as-is
            _ => expr.clone(),
        }
    }

    pub(crate) fn lookup_imported_func(&self, name: &str) -> Option<&str> {
        self.imported_functions_index
            .get(name)
            .map(|&idx| self.imported_functions[idx].1.as_str())
    }

    pub(crate) fn register_imported_func(&mut self, local_name: String, original_name: String) {
        let idx = self.imported_functions.len();
        self.imported_functions_index
            .insert(local_name.clone(), idx);
        self.imported_functions.push((local_name, original_name));
    }

    pub(crate) fn register_extern_func_types(
        &mut self,
        name: String,
        param_types: Vec<Type>,
        return_type: Type,
    ) {
        self.extern_func_types
            .push((name, param_types, return_type));
    }

    pub(crate) fn lookup_extern_func_types(&self, name: &str) -> Option<(&Vec<Type>, &Type)> {
        self.extern_func_types
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, params, ret)| (params, ret))
    }

    pub(crate) fn register_native_module(
        &mut self,
        local_name: String,
        module_name: String,
        method_name: Option<String>,
    ) {
        self.native_modules
            .push((local_name, module_name, method_name));
    }

    pub(crate) fn lookup_native_module(&self, name: &str) -> Option<(&str, Option<&str>)> {
        self.native_modules
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, m, method)| (m.as_str(), method.as_ref().map(|s| s.as_str())))
    }

    pub(crate) fn register_builtin_module_alias(
        &mut self,
        local_name: String,
        module_name: String,
    ) {
        let idx = self.builtin_module_aliases.len();
        self.builtin_module_aliases_index
            .insert(local_name.clone(), idx);
        self.builtin_module_aliases.push((local_name, module_name));
    }

    pub(crate) fn lookup_builtin_module_alias(&self, name: &str) -> Option<&str> {
        self.builtin_module_aliases_index
            .get(name)
            .map(|&idx| self.builtin_module_aliases[idx].1.as_str())
    }

    pub(crate) fn register_native_instance(
        &mut self,
        local_name: String,
        module_name: String,
        class_name: String,
    ) {
        self.native_instances
            .push((local_name, module_name, class_name));
    }

    pub(crate) fn lookup_native_instance(&self, name: &str) -> Option<(&str, &str)> {
        // Check scoped instances first (function-local variables)
        self.native_instances
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
            .or_else(|| {
                // Check module-level instances (survive scope exits)
                self.module_native_instances
                    .iter()
                    .find(|(n, _, _)| n == name)
                    .map(|(_, module, class)| (module.as_str(), class.as_str()))
            })
    }

    pub(crate) fn lookup_func_return_native_instance(
        &self,
        func_name: &str,
    ) -> Option<(&str, &str)> {
        self.func_return_native_instances
            .iter()
            .find(|(n, _, _)| n == func_name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
    }
}

/// Map a function's declared return type to a native-instance class when it
/// matches a known stdlib pattern. Lets a wrapper function like
/// `function openSocket(host, port): Socket { ... }` advertise that calls
/// to it produce a Socket instance — call sites then register the local
/// via the user-factory consumer in the var-decl handler, so subsequent
/// `sock.on(...)` / `sock.write(...)` dispatches statically through the
/// NATIVE_MODULE_TABLE just like `const sock = net.createConnection(...)`.
///
/// Recognizes both `T` and `Promise<T>` return types so async wrappers
/// work without ceremony.
fn native_instance_from_return_type(ty: &Type) -> Option<(&'static str, &'static str)> {
    let inner = match ty {
        Type::Generic { base, type_args } if base == "Promise" => type_args.first().unwrap_or(ty),
        Type::Promise(inner) => inner.as_ref(),
        other => other,
    };
    if let Type::Named(name) = inner {
        return match name.as_str() {
            "Socket" => Some(("net", "Socket")),
            "Redis" => Some(("ioredis", "Redis")),
            "EventEmitter" => Some(("events", "EventEmitter")),
            "Pool" => Some(("mysql2/promise", "Pool")),
            "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
            "WebSocket" => Some(("ws", "WebSocket")),
            "WebSocketServer" => Some(("ws", "WebSocketServer")),
            _ => None,
        };
    }
    None
}

// Internal anchor — keeps the file's outer impl block intact while
// `native_instance_from_return_type` lives at module scope.
#[allow(dead_code)]
struct __PerryHirSentinel;
impl LoweringContext {
    #[allow(dead_code)]
    fn __perry_hir_sentinel(&self) {}

    pub(crate) fn register_func_return_type(&mut self, name: String, ty: Type) {
        self.func_return_types.push((name, ty));
    }

    pub(crate) fn lookup_func_return_type(&self, name: &str) -> Option<&Type> {
        self.func_return_types
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| ty)
    }

    /// Phase 4.1: register a method's return type so call-site inference can
    /// resolve `obj.method()` when `obj: Type::Named(class_name)`. Called
    /// from `lower_class_from_ast` right after each method's Function is
    /// built, so both declared annotations and Phase 4-expansion body
    /// inferences flow through. Extends-chain traversal happens at lookup
    /// time via `lookup_class_method_return_type`.
    pub(crate) fn register_class_method_return_type(
        &mut self,
        class_name: String,
        method_name: String,
        ty: Type,
    ) {
        self.class_method_return_types
            .push((class_name, method_name, ty));
    }

    /// Phase 4.1: lookup the return type of `class_name.method_name`.
    /// Does NOT walk the extends chain today — that needs the parent class
    /// name accessible from the context, which the current registry doesn't
    /// track. Callers handle inheritance externally if needed. Reverse
    /// iteration so the latest registration wins for shadowing (mirrors
    /// `lookup_func_return_type`).
    pub(crate) fn lookup_class_method_return_type(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<&Type> {
        self.class_method_return_types
            .iter()
            .rev()
            .find(|(c, m, _)| c == class_name && m == method_name)
            .map(|(_, _, ty)| ty)
    }

    pub(crate) fn enter_scope(&mut self) -> (usize, usize, usize) {
        // Function/closure boundary: new locals are no longer module-level.
        self.scope_depth += 1;
        (
            self.locals.len(),
            self.native_instances.len(),
            self.functions.len(),
        )
    }

    pub(crate) fn exit_scope(&mut self, mark: (usize, usize, usize)) {
        debug_assert!(self.scope_depth > 0, "exit_scope called at module depth");
        self.scope_depth = self.scope_depth.saturating_sub(1);
        self.locals.truncate(mark.0);
        self.native_instances.truncate(mark.1);
        // Remove index entries for functions being truncated, then restore any
        // earlier entries that were shadowed by the removed ones.
        for i in mark.2..self.functions.len() {
            let name = &self.functions[i].0;
            // Find if there's an earlier entry with the same name
            let mut earlier_idx = None;
            for j in (0..mark.2).rev() {
                if self.functions[j].0 == *name {
                    earlier_idx = Some(j);
                    break;
                }
            }
            if let Some(j) = earlier_idx {
                self.functions_index.insert(name.clone(), j);
            } else {
                self.functions_index.remove(name);
            }
        }
        self.functions.truncate(mark.2);
    }

    /// Enter a nested block scope for `{ ... }`, `if`/`else`, loop body, etc.
    /// Unlike `enter_scope` (function boundaries), this is designed for
    /// block-scoped `let`/`const`: `pop_block_scope` removes inner `let`/`const`
    /// bindings while preserving `var`-hoisted ones so they remain visible in
    /// the enclosing function scope.
    pub(crate) fn push_block_scope(&mut self) -> (usize, usize) {
        self.inside_block_scope += 1;
        (self.locals.len(), self.functions.len())
    }

    /// Exit a nested block scope introduced by `push_block_scope`. Inner
    /// `let`/`const` bindings are removed but any `var`-declared locals
    /// (tracked via `var_hoisted_ids`) are retained, since `var` is
    /// function-scoped in JS.
    pub(crate) fn pop_block_scope(&mut self, mark: (usize, usize)) {
        debug_assert!(
            self.inside_block_scope > 0,
            "pop_block_scope without matching push"
        );
        self.inside_block_scope = self.inside_block_scope.saturating_sub(1);
        let (locals_mark, functions_mark) = mark;

        // Preserve var-hoisted locals: move any hoisted entries defined after
        // the mark to the position just past the mark, then drop the rest.
        if self.locals.len() > locals_mark {
            let mut kept: Vec<(String, LocalId, Type)> = Vec::new();
            for entry in self.locals.drain(locals_mark..) {
                if self.var_hoisted_ids.contains(&entry.1) {
                    kept.push(entry);
                }
            }
            self.locals.extend(kept);
        }

        // Function declarations inside a block are block-scoped in ES6+.
        // Same pattern as exit_scope: remove/restore function index entries.
        for i in functions_mark..self.functions.len() {
            let name = &self.functions[i].0;
            let mut earlier_idx = None;
            for j in (0..functions_mark).rev() {
                if self.functions[j].0 == *name {
                    earlier_idx = Some(j);
                    break;
                }
            }
            if let Some(j) = earlier_idx {
                self.functions_index.insert(name.clone(), j);
            } else {
                self.functions_index.remove(name);
            }
        }
        self.functions.truncate(functions_mark);
    }
}

// Re-export extracted module functions
pub(crate) use crate::analysis::*;
pub(crate) use crate::destructuring::*;
pub(crate) use crate::jsx::*;
pub(crate) use crate::lower_decl::*;
pub(crate) use crate::lower_patterns::*;
pub(crate) use crate::lower_types::*;

pub fn lower_module(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
) -> Result<Module> {
    lower_module_with_class_id(ast_module, name, source_file_path, 1).map(|(module, _)| module)
}

/// Try to fold an `Expr::Call { callee: PropertyGet { object, property }, args }`
/// into an `Expr::Array<Method>` HIR variant for known array methods. Used by
/// the optional-chain Call lowering, which constructs `Expr::Call` directly
/// (bypassing the regular `lower_expr` array fast-path detection that would
/// otherwise catch `obj.map(cb)` etc. on an AST `MemberExpr` callee).
///
/// Returns `Some(rewritten_expr)` when the callee is a PropertyGet on a known
/// array method name and the arity matches; returns `None` otherwise so the
/// caller can fall back to the generic `Expr::Call` form.
pub(crate) fn try_fold_array_method_call(call: Expr) -> Expr {
    let (callee, args) = match call {
        Expr::Call { callee, args, .. } => (callee, args),
        other => return other,
    };
    let (object, property) = match *callee {
        Expr::PropertyGet { object, property } => (object, property),
        other => {
            return Expr::Call {
                callee: Box::new(other),
                args,
                type_args: Vec::new(),
            };
        }
    };
    // Helper to rebuild the original Call if we don't want to fold.
    let rebuild = |obj: Box<Expr>, prop: String, args: Vec<Expr>| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: obj,
            property: prop,
        }),
        args,
        type_args: Vec::new(),
    };
    match property.as_str() {
        "map" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayMap {
                array: object,
                callback: Box::new(cb),
            }
        }
        "filter" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFilter {
                array: object,
                callback: Box::new(cb),
            }
        }
        "forEach" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayForEach {
                array: object,
                callback: Box::new(cb),
            }
        }
        "find" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFind {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findIndex" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindIndex {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findLast" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLast {
                array: object,
                callback: Box::new(cb),
            }
        }
        "findLastIndex" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLastIndex {
                array: object,
                callback: Box::new(cb),
            }
        }
        "some" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArraySome {
                array: object,
                callback: Box::new(cb),
            }
        }
        "every" if !args.is_empty() => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayEvery {
                array: object,
                callback: Box::new(cb),
            }
        }
        _ => rebuild(object, property, args),
    }
}

/// Names of well-known `Object.<name>` static methods. Used by the typeof
/// fast path so `typeof Object.groupBy === "function"` evaluates to true
/// at compile time.
pub(crate) fn is_known_object_static_method(name: &str) -> bool {
    matches!(
        name,
        "keys"
            | "values"
            | "entries"
            | "fromEntries"
            | "assign"
            | "is"
            | "hasOwn"
            | "freeze"
            | "seal"
            | "preventExtensions"
            | "create"
            | "isFrozen"
            | "isSealed"
            | "isExtensible"
            | "getPrototypeOf"
            | "setPrototypeOf"
            | "defineProperty"
            | "defineProperties"
            | "getOwnPropertyDescriptor"
            | "getOwnPropertyDescriptors"
            | "getOwnPropertyNames"
            | "getOwnPropertySymbols"
            | "groupBy"
    )
}

/// Names of well-known `Array.<name>` static methods.
pub(crate) fn is_known_array_static_method(name: &str) -> bool {
    matches!(name, "isArray" | "from" | "of" | "fromAsync")
}

/// Names of `String.prototype.<name>` instance methods that Perry's
/// runtime implements (or short-circuits) — used by the `typeof
/// "".methodName` AST fold so feature-detection checks like
/// `if (typeof "".isWellFormed === "function")` see the methods that
/// the runtime would actually dispatch successfully.
pub(crate) fn is_known_string_prototype_method(name: &str) -> bool {
    matches!(
        name,
        // ES2015+ classics
        "charAt" | "charCodeAt" | "codePointAt" | "concat" | "endsWith"
        | "includes" | "indexOf" | "lastIndexOf" | "match" | "matchAll"
        | "normalize" | "padEnd" | "padStart" | "repeat" | "replace"
        | "replaceAll" | "search" | "slice" | "split" | "startsWith"
        | "substring" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase"
        | "toLocaleUpperCase" | "trim" | "trimEnd" | "trimStart" | "at"
        // ES2024
        | "isWellFormed" | "toWellFormed"
    )
}

/// `let/const x = new FinalizationRegistry(...)` bindings into the lowering
/// context. This is used by `obj.method()` lowering to recognise these instances
/// without requiring type inference (Perry's existing var-decl type inference
/// doesn't extend to WeakRef/FinalizationRegistry).
fn pre_scan_weakref_locals(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    fn classify_new(new_expr: &ast::NewExpr) -> Option<&'static str> {
        if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
            match ident.sym.as_ref() {
                "WeakRef" => Some("WeakRef"),
                "FinalizationRegistry" => Some("FinalizationRegistry"),
                "WeakMap" => Some("WeakMap"),
                "WeakSet" => Some("WeakSet"),
                "Proxy" => Some("Proxy"),
                _ => None,
            }
        } else {
            None
        }
    }
    fn unwrap_init(mut e: &ast::Expr) -> &ast::Expr {
        loop {
            match e {
                ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                ast::Expr::TsNonNull(nn) => e = &nn.expr,
                ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                ast::Expr::Paren(p) => e = &p.expr,
                _ => break,
            }
        }
        e
    }
    fn record_var(decl: &ast::VarDeclarator, ctx: &mut LoweringContext) {
        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, decl.init.as_ref()) {
            let init_unwrapped = unwrap_init(init);
            if let ast::Expr::New(new_expr) = init_unwrapped {
                let name = ident.id.sym.to_string();
                match classify_new(new_expr) {
                    Some("WeakRef") => {
                        ctx.weakref_locals.insert(name);
                    }
                    Some("FinalizationRegistry") => {
                        ctx.finreg_locals.insert(name);
                    }
                    Some("WeakMap") => {
                        ctx.weakmap_locals.insert(name);
                    }
                    Some("WeakSet") => {
                        ctx.weakset_locals.insert(name);
                    }
                    Some("Proxy") => {
                        ctx.proxy_locals.insert(name.clone());
                        // Track proxy target class for `new p(args)` fold.
                        if let Some(args) = new_expr.args.as_ref() {
                            if let Some(first) = args.first() {
                                if let ast::Expr::Ident(cls_ident) = first.expr.as_ref() {
                                    let cls_name = cls_ident.sym.to_string();
                                    ctx.proxy_target_classes.insert(name, cls_name);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    fn walk_stmt(stmt: &ast::Stmt, ctx: &mut LoweringContext) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    record_var(decl, ctx);
                }
            }
            ast::Stmt::Decl(ast::Decl::Using(using_decl)) => {
                for decl in &using_decl.decls {
                    record_var(decl, ctx);
                }
            }
            // Function declarations — descend into the body so `const
            // ref = new WeakRef(x)` inside a function is still tracked
            // and `ref.deref()` lowers to `Expr::WeakRefDeref` instead
            // of falling through to the generic method dispatch.
            ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
                if let Some(body) = &fn_decl.function.body {
                    for s in &body.stmts {
                        walk_stmt(s, ctx);
                    }
                }
            }
            ast::Stmt::Block(block) => {
                for s in &block.stmts {
                    walk_stmt(s, ctx);
                }
            }
            ast::Stmt::If(if_stmt) => {
                walk_stmt(&if_stmt.cons, ctx);
                if let Some(alt) = &if_stmt.alt {
                    walk_stmt(alt, ctx);
                }
            }
            ast::Stmt::While(w) => walk_stmt(&w.body, ctx),
            ast::Stmt::DoWhile(w) => walk_stmt(&w.body, ctx),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                    for decl in &vd.decls {
                        record_var(decl, ctx);
                    }
                }
                walk_stmt(&f.body, ctx);
            }
            ast::Stmt::ForIn(f) => walk_stmt(&f.body, ctx),
            ast::Stmt::ForOf(f) => walk_stmt(&f.body, ctx),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    walk_stmt(s, ctx);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body.stmts {
                        walk_stmt(s, ctx);
                    }
                }
                if let Some(finalizer) = &t.finalizer {
                    for s in &finalizer.stmts {
                        walk_stmt(s, ctx);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    for s in &case.cons {
                        walk_stmt(s, ctx);
                    }
                }
            }
            _ => {}
        }
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => walk_stmt(stmt, ctx),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(var_decl) = &export_decl.decl {
                    for decl in &var_decl.decls {
                        record_var(decl, ctx);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Pre-scan top-level function declarations for the standard TypeScript
/// mixin pattern:
///
///   function Foo<T extends Constructor>(Base: T) {
///     return class extends Base {
///       greet(): string { return "..."; }
///     };
///   }
///
/// Records the function name → (base_param_name, class_ast) so that calls
/// like `const Mixed = Foo(BaseClass)` can synthesize a real class.
fn pre_scan_mixin_functions(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    fn try_record_fn(fn_decl: &ast::FnDecl, ctx: &mut LoweringContext) {
        if fn_decl.function.params.len() != 1 {
            return;
        }
        let param_name = match &fn_decl.function.params[0].pat {
            ast::Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => return,
        };
        let body = match &fn_decl.function.body {
            Some(b) => b,
            None => return,
        };
        if body.stmts.len() != 1 {
            return;
        }
        let return_arg = match &body.stmts[0] {
            ast::Stmt::Return(r) => match &r.arg {
                Some(arg) => arg.as_ref(),
                None => return,
            },
            _ => return,
        };
        let mut e = return_arg;
        loop {
            match e {
                ast::Expr::Paren(p) => e = &p.expr,
                _ => break,
            }
        }
        let class_expr = match e {
            ast::Expr::Class(ce) => ce,
            _ => return,
        };
        let extends_param = match &class_expr.class.super_class {
            Some(sc) => {
                if let ast::Expr::Ident(ident) = sc.as_ref() {
                    ident.sym.as_ref() == param_name
                } else {
                    false
                }
            }
            None => false,
        };
        if !extends_param {
            return;
        }
        let fn_name = fn_decl.ident.sym.to_string();
        ctx.mixin_funcs
            .insert(fn_name, (param_name, Box::new((*class_expr.class).clone())));
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                try_record_fn(fn_decl, ctx);
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                if let ast::Decl::Fn(fn_decl) = &export.decl {
                    try_record_fn(fn_decl, ctx);
                }
            }
            _ => {}
        }
    }
}

pub fn lower_module_with_class_id(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_and_types(ast_module, name, source_file_path, start_class_id, None)
}

/// Append `class` to `module.classes` only if no class with the same name has
/// already been pushed. Same dedup policy that the function-scoped path at
/// `lower_decl.rs::3059` already enforces — extended here to every push site
/// so namespace-scoped, mixin, and class-expression paths can't smuggle a
/// duplicate-named class into codegen and trip the LLVM IR's "redefinition
/// of global '@perry_class_keys_<modprefix>__<class>'" rule. The lookup
/// pipeline (`Expr::New { class_name }`, `class_lookup` / `class_ids`) is
/// purely name-based today, so the second class wouldn't be reachable through
/// any binding anyway — emitting it just produces unreachable globals that
/// then collide with the first class's globals at link time. See #336.
fn push_class_dedup(module: &mut Module, class: Class) {
    if !module.classes.iter().any(|c| c.name == class.name) {
        module.classes.push(class);
    }
}

pub fn lower_module_with_class_id_and_types(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_types_and_seed(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        None,
    )
}

pub fn lower_module_with_class_id_types_and_seed(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_types_seed_and_entry(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        imported_class_fields,
        false,
    )
}

/// Issue #444: variant that takes `is_entry_module` so `import.meta.main`
/// resolves to `true` only inside the user-supplied entry TypeScript file
/// (matching Node 24+ / Bun semantics). All other lowering callers go
/// through the wrapper above with `is_entry_module=false`.
pub fn lower_module_with_class_id_types_seed_and_entry(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
    is_entry_module: bool,
) -> Result<(Module, ClassId)> {
    lower_module_full(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        imported_class_fields,
        is_entry_module,
        false,
    )
}

/// Issue #668: superset of the `_seed_and_entry` wrapper that also accepts
/// `is_external_module`. Callers in `crates/perry/src/commands/compile/`
/// pass `true` when the source file lives under any `node_modules/` segment
/// so the require-literal compile error in `lower_call.rs` skips library
/// code (which legitimately uses `require()` for deferred cycle breaks).
pub fn lower_module_full(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
    is_entry_module: bool,
    is_external_module: bool,
) -> Result<(Module, ClassId)> {
    let mut ctx = LoweringContext::with_class_id_start(source_file_path, start_class_id);
    ctx.resolved_types = resolved_types;
    ctx.is_entry_module = is_entry_module;
    ctx.is_external_module = is_external_module;
    if let Some(seed) = imported_class_fields {
        ctx.seed_imported_class_fields(seed);
    }
    let mut module = Module::new(name);

    // Pre-scan for WeakRef/FinalizationRegistry variable declarations so subsequent
    // method-call lowering (`x.deref()`, `x.register(...)`, `x.unregister(...)`) can
    // route via the dedicated HIR variants without relying on type inference.
    pre_scan_weakref_locals(ast_module, &mut ctx);

    // Pre-scan for mixin functions: a function whose body is exactly
    // `return class extends <param> { ... };`. Lets `const Mixed = MixinFn(SomeClass)`
    // synthesize a real concrete class extending `SomeClass`.
    pre_scan_mixin_functions(ast_module, &mut ctx);

    // For .tsx files, pre-register JSX runtime symbols so JSX expressions can be lowered.
    // This injects an automatic import of { jsx, jsxs } from "react/jsx-runtime"
    // (remapped to perry-react via the user's packageAliases).
    // Fragment is NOT imported — it's inlined as the string "__Fragment" directly in JSX lowering.
    if source_file_path.ends_with(".tsx") {
        ctx.register_imported_func("__jsx".to_string(), "jsx".to_string());
        ctx.register_imported_func("__jsxs".to_string(), "jsxs".to_string());
        module.imports.push(Import {
            source: "react/jsx-runtime".to_string(),
            specifiers: vec![
                ImportSpecifier::Named {
                    local: "__jsx".to_string(),
                    imported: "jsx".to_string(),
                },
                ImportSpecifier::Named {
                    local: "__jsxs".to_string(),
                    imported: "jsxs".to_string(),
                },
            ],
            is_native: false,
            module_kind: ModuleKind::NativeCompiled,
            resolved_path: None,
            type_only: false,
            is_dynamic: false,
        });
    }

    // Pre-scan: Find all function names that have implementations (bodies)
    // This is needed to properly handle TypeScript function overloads where
    // multiple signature-only declarations precede a single implementation
    let mut functions_with_bodies: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for item in &ast_module.body {
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(fn_decl) = fn_decl {
            if fn_decl.function.body.is_some() {
                functions_with_bodies.insert(fn_decl.ident.sym.to_string());
            }
        }
    }

    // First pass: collect all function declarations (both exported and non-exported)
    // Skip 'declare function' statements (functions with no body) - they are external FFI
    // BUT: also skip overload signatures if an implementation exists
    for item in &ast_module.body {
        // Extract function declaration from both regular statements and export declarations
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(fn_decl) = fn_decl {
            let func_name = fn_decl.ident.sym.to_string();

            // Skip signature-only declarations (no body)
            if fn_decl.function.body.is_none() {
                // If this function has an implementation elsewhere, skip the signature
                // (it's a TypeScript overload, not an external FFI declaration)
                if functions_with_bodies.contains(&func_name) {
                    continue;
                }

                // No implementation exists - treat as external FFI declaration
                // Extract parameter types for FFI signature
                let param_types: Vec<Type> = fn_decl
                    .function
                    .params
                    .iter()
                    .map(|param| extract_param_type_with_ctx(&param.pat, None))
                    .collect();

                // Extract return type
                let return_type = fn_decl
                    .function
                    .return_type
                    .as_ref()
                    .map(|rt| extract_ts_type(&rt.type_ann))
                    .unwrap_or(Type::Void);

                // Register as external function so calls resolve to ExternFuncRef
                ctx.register_imported_func(func_name.clone(), func_name.clone());
                // Also store type information for code generation
                ctx.register_extern_func_types(func_name, param_types, return_type);
                continue;
            }

            // Function has a body - each declaration gets a unique FuncId
            // (inner-scope functions shadow outer-scope same-name functions via reverse lookup)
            let func_id = ctx.fresh_func();
            ctx.register_func(func_name.clone(), func_id);

            // Pre-register return type annotation for call-site type inference
            // (so variables initialized from function calls can infer their type)
            if let Some(rt) = &fn_decl.function.return_type {
                let return_type = extract_ts_type(&rt.type_ann);
                if !matches!(return_type, Type::Any) {
                    ctx.register_func_return_type(func_name, return_type);
                }
            }
        }
    }

    // Pre-register module-level variable declarations so function bodies
    // declared before the variable can still reference them via lookup_local
    for item in &ast_module.body {
        let var_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(v))) => Some(v),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(v) = &export_decl.decl {
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(var_decl) = var_decl {
            for decl in &var_decl.decls {
                if let ast::Pat::Ident(ident) = &decl.name {
                    let name = ident.id.sym.to_string();
                    if ctx.lookup_local(&name).is_none() {
                        let ty = ident
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type(&ann.type_ann))
                            .unwrap_or(Type::Any);
                        ctx.define_local(name.clone(), ty);
                        ctx.pre_registered_module_vars.insert(name);
                    }
                }
            }
        }
    }

    // Pre-register all class declarations so that static method calls between
    // classes declared in the same file resolve correctly regardless of declaration order.
    // Without this, SqrtPriceMath.getAmount0Delta calling FullMath.mulDivRoundingUp
    // fails if FullMath is declared after SqrtPriceMath.
    for item in &ast_module.body {
        let class_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(cd))) => Some(cd),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Class(cd) = &export_decl.decl {
                    Some(cd)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(cd) = class_decl {
            let name = cd.ident.sym.to_string();
            if ctx.lookup_class(&name).is_none() {
                let id = ctx.fresh_class();
                ctx.register_class(name.clone(), id);
            }
            // Collect static field/method names
            let mut static_field_names = Vec::new();
            let mut static_method_names = Vec::new();
            for member in &cd.class.body {
                match member {
                    ast::ClassMember::Method(method) if method.is_static => {
                        if let ast::PropName::Ident(ident) = &method.key {
                            static_method_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateMethod(method) if method.is_static => {
                        static_method_names.push(format!("#{}", method.key.name));
                    }
                    ast::ClassMember::ClassProp(prop) if prop.is_static => {
                        if let ast::PropName::Ident(ident) = &prop.key {
                            static_field_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                        static_field_names.push(format!("#{}", prop.key.name));
                    }
                    _ => {}
                }
            }
            if !static_field_names.is_empty() || !static_method_names.is_empty() {
                // Only register if not already registered (lower_class_decl will re-register)
                if !ctx.class_statics.iter().any(|(cn, _, _)| cn == &name) {
                    ctx.register_class_statics(name, static_field_names, static_method_names);
                }
            }
        }
    }

    // Main pass: lower everything
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(&mut ctx, &mut module, stmt)?;
            }
            ast::ModuleItem::ModuleDecl(decl) => {
                lower_module_decl(&mut ctx, &mut module, decl)?;
            }
        }
        // Flush any pending functions created during expression lowering
        // (e.g., inline methods in object literals)
        for func in ctx.pending_functions.drain(..) {
            module.functions.push(func);
        }
        // Flush any pending classes created during expression lowering
        // (e.g., class expressions in `new (class extends Command { ... })()`)
        for class in ctx.pending_classes.drain(..) {
            push_class_dedup(&mut module, class);
        }
    }

    // Populate exported_native_instances by matching native_instances with exports
    for (local_name, module_name, class_name) in &ctx.native_instances {
        // Check if this native instance is exported
        for export in &module.exports {
            if let Export::Named { local, exported } = export {
                if local == local_name {
                    module.exported_native_instances.push((
                        exported.clone(),
                        module_name.clone(),
                        class_name.clone(),
                    ));
                }
            }
        }
    }

    // Populate exported_func_return_native_instances for functions that return native instances
    for (func_name, native_module, native_class) in &ctx.func_return_native_instances {
        // Check if this function is directly exported
        let is_exported = module
            .functions
            .iter()
            .any(|f| f.name == *func_name && f.is_exported);
        if is_exported {
            module.exported_func_return_native_instances.push((
                func_name.clone(),
                native_module.clone(),
                native_class.clone(),
            ));
        } else {
            // Also check named exports (e.g., `export { getRedis }`)
            for export in &module.exports {
                if let Export::Named { local, exported } = export {
                    if local == func_name {
                        module.exported_func_return_native_instances.push((
                            exported.clone(),
                            native_module.clone(),
                            native_class.clone(),
                        ));
                    }
                }
            }
        }
    }

    module.uses_fetch = ctx.uses_fetch;
    module.uses_webassembly = ctx.uses_webassembly;
    module.extern_funcs = ctx.extern_func_types.clone();

    // Post-pass: widen `mutable_captures` across sibling closures. When two
    // closures in the same scope share a capture and one of them assigns to
    // it, the variable must be boxed; every closure that captures it must
    // also go through the box so they observe each other's writes. Without
    // this pass, a `get: () => value` sibling of `inc: () => value++` captures
    // the raw initial value instead of the shared boxed binding.
    widen_mutable_captures_stmts(&mut module.init);
    for func in &mut module.functions {
        widen_mutable_captures_stmts(&mut func.body);
    }
    for class in &mut module.classes {
        for method in &mut class.methods {
            widen_mutable_captures_stmts(&mut method.body);
        }
        for (_, getter) in &mut class.getters {
            widen_mutable_captures_stmts(&mut getter.body);
        }
        for (_, setter) in &mut class.setters {
            widen_mutable_captures_stmts(&mut setter.body);
        }
        for static_method in &mut class.static_methods {
            widen_mutable_captures_stmts(&mut static_method.body);
        }
        if let Some(ref mut ctor) = class.constructor {
            widen_mutable_captures_stmts(&mut ctor.body);
        }
    }

    // Post-pass: infer `extends_name` from `extends_expr` for the bare-factory
    // shape `class Sub extends makeFactory() {}` where `makeFactory` is a
    // top-level function whose body trivially returns a static `ClassRef`.
    // Without this, the codegen chain walks
    // (`apply_field_initializers_recursive` + the keys-array generator) walk
    // by `extends_name` only, see `None`, and skip the factory class's
    // field initializers entirely — `new Sub().kind` reads `undefined`
    // instead of the parent's `kind = "bare"` literal. Surfaced by the
    // #806 mixin harness (bare-factory section).
    infer_dynamic_extends_names(&mut module);

    Ok((module, ctx.next_class_id))
}

/// Fill in `Class::extends_name` for classes whose parent is the result of
/// calling a statically-resolvable factory function — but ONLY when the
/// parent's field initializers are closure-free (no `LocalGet` reads). The
/// post-pass runs after every function and class is in `module`, so
/// forward-references work (e.g. `class Sub extends makeBare() {}` ahead of
/// `function makeBare() …` hoisting).
///
/// The closure-free guard exists because the field-init pass at codegen
/// (`apply_field_initializers_recursive`) inlines each chained class's
/// init expressions directly into the subclass's constructor. That's
/// correct for pure-literal initializers like `kind = "bare"` but wrong
/// for `_tag = tag` where `tag` is the factory's parameter — the inlined
/// `LocalGet(tag)` would re-resolve in the subclass's scope (where `tag`
/// doesn't exist) and produce garbage. Conservatively skip those: the
/// subclass's static parent stays None and field-init inheritance only
/// works for the literal-initialized parents that #806's bare-factory
/// section needs.
fn infer_dynamic_extends_names(module: &mut Module) {
    use std::collections::HashMap;
    // Build a map of `function_id → returned ClassRef name` for every
    // function whose body returns a static ClassRef. Only the LAST `Return`
    // is examined — bodies with multiple Returns to different classes
    // don't resolve uniquely, and the canonical factory shape has exactly
    // one Return as its last statement.
    let mut factory_returns: HashMap<u32, String> = HashMap::new();
    for func in &module.functions {
        if let Some(name) = trailing_return_classref(&func.body) {
            factory_returns.insert(func.id, name);
        }
    }
    // Index classes by name so we can re-resolve transitively (a chain like
    // `Sub extends A() {}` where `A` returns `__anon_N` and `__anon_N` is
    // a class we own — we only set `extends_name` for `Sub` here; chain
    // walks at codegen step through `__anon_N.extends_name` normally).
    let class_field_inits_pure: HashMap<String, bool> = module
        .classes
        .iter()
        .map(|c| (c.name.clone(), fields_are_pure(c)))
        .collect();
    for class in &mut module.classes {
        if class.extends_name.is_some() {
            continue;
        }
        let Some(expr) = class.extends_expr.as_deref() else {
            continue;
        };
        let Expr::Call { callee, .. } = expr else {
            continue;
        };
        let Expr::FuncRef(func_id) = callee.as_ref() else {
            continue;
        };
        let Some(parent_name) = factory_returns.get(func_id) else {
            continue;
        };
        // Only inherit field-init machinery when the parent's fields are
        // pure (no `LocalGet`). Methods on the parent are unaffected —
        // those dispatch through the runtime CLASS_REGISTRY which is
        // populated by the #826 RegisterClassParentDynamic side effect.
        if class_field_inits_pure
            .get(parent_name)
            .copied()
            .unwrap_or(false)
        {
            class.extends_name = Some(parent_name.clone());
        }
    }
}

/// True when none of the class's field initializers contain a `LocalGet`
/// (the canonical sign that an initializer closes over its surrounding
/// scope — function parameters, outer-block lets, etc.).
fn fields_are_pure(class: &Class) -> bool {
    for field in &class.fields {
        if let Some(init) = &field.init {
            if expr_reads_local(init) {
                return false;
            }
        }
        if let Some(key) = &field.key_expr {
            if expr_reads_local(key) {
                return false;
            }
        }
    }
    true
}

fn expr_reads_local(expr: &Expr) -> bool {
    if matches!(expr, Expr::LocalGet(_)) {
        return true;
    }
    let mut found = false;
    crate::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_reads_local(child) {
            found = true;
        }
    });
    found
}

/// Return `Some(name)` if `body`'s last `Return` statement yields a static
/// `Expr::ClassRef` (directly or as the last element of an `Expr::Sequence`).
fn trailing_return_classref(body: &[Stmt]) -> Option<String> {
    for stmt in body.iter().rev() {
        if let Stmt::Return(Some(expr)) = stmt {
            return classref_name(expr);
        }
    }
    None
}

fn classref_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::ClassRef(name) => Some(name.clone()),
        Expr::Sequence(parts) => parts.last().and_then(classref_name),
        _ => None,
    }
}

/// Post-lowering pass that widens every `Expr::Closure`'s `mutable_captures`
/// to include any capture that is assigned to inside a sibling closure in the
/// same lexical scope. Then recurses into each closure body so nested scopes
/// get the same treatment. This ensures that when multiple closures share a
/// captured binding and any one of them mutates it, all of them treat it as
/// boxed so reads and writes observe the same storage slot.
fn widen_mutable_captures_stmts(stmts: &mut [Stmt]) {
    // Tier 4.3 (v0.5.336): three independent read passes fused into a
    // single iteration over `stmts`. Pre-fix this was three separate
    // `for stmt in stmts.iter()` loops back-to-back, each populating
    // its own HashSet. The collectors don't depend on each other's
    // outputs (they read disjoint Expr/Stmt fields), so calling all
    // three per stmt is equivalent and saves 2 full slice traversals
    // per scope. The mutating pass below still runs separately because
    // it depends on the union of all three sets.
    //
    // Also detects variables that are captured by closures AND assigned
    // at the scope level (not inside a closure). This handles the pattern:
    //   let x = 0;
    //   fns.push(() => x);
    //   x = 10;               // assignment at scope level
    //   fns.push(() => x);
    // All closures should see the final value of x (capture-by-reference).
    let mut scope_mutable: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    let mut scope_captured: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    let mut scope_assigned_at_level: std::collections::HashSet<LocalId> =
        std::collections::HashSet::new();
    for stmt in stmts.iter() {
        collect_closure_assigned_stmt(stmt, &mut scope_mutable);
        collect_closure_captures_stmt(stmt, &mut scope_captured);
        collect_scope_level_assigns_stmt(stmt, &mut scope_assigned_at_level);
    }
    for id in &scope_captured {
        if scope_assigned_at_level.contains(id) {
            scope_mutable.insert(*id);
        }
    }
    for stmt in stmts.iter_mut() {
        widen_mutable_captures_stmt(stmt, &scope_mutable);
    }
}

fn widen_mutable_captures_stmt(
    stmt: &mut Stmt,
    scope_mutable: &std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Expr(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Return(Some(expr)) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Throw(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_stmts(then_branch);
            if let Some(else_stmts) = else_branch {
                widen_mutable_captures_stmts(else_stmts);
            }
        }
        Stmt::While { condition, body } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_stmts(body);
        }
        Stmt::DoWhile { body, condition } => {
            widen_mutable_captures_stmts(body);
            widen_mutable_captures_expr(condition, scope_mutable);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                widen_mutable_captures_stmt(init_stmt, scope_mutable);
            }
            if let Some(cond) = condition {
                widen_mutable_captures_expr(cond, scope_mutable);
            }
            if let Some(upd) = update {
                widen_mutable_captures_expr(upd, scope_mutable);
            }
            widen_mutable_captures_stmts(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            widen_mutable_captures_stmts(body);
            if let Some(catch_clause) = catch {
                widen_mutable_captures_stmts(&mut catch_clause.body);
            }
            if let Some(finally_stmts) = finally {
                widen_mutable_captures_stmts(finally_stmts);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            widen_mutable_captures_expr(discriminant, scope_mutable);
            for case in cases {
                if let Some(test) = &mut case.test {
                    widen_mutable_captures_expr(test, scope_mutable);
                }
                widen_mutable_captures_stmts(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            widen_mutable_captures_stmt(body, scope_mutable);
        }
        _ => {}
    }
}

fn widen_mutable_captures_expr(
    expr: &mut Expr,
    scope_mutable: &std::collections::HashSet<LocalId>,
) {
    match expr {
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            let mut mset: std::collections::HashSet<LocalId> =
                mutable_captures.iter().copied().collect();
            for id in captures.iter() {
                if scope_mutable.contains(id) {
                    mset.insert(*id);
                }
            }
            let mut new_mutable: Vec<LocalId> = mset.into_iter().collect();
            new_mutable.sort();
            *mutable_captures = new_mutable;

            // Recurse into the closure body so nested closures get a fresh
            // scope-relative widening.
            widen_mutable_captures_stmts(body);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            widen_mutable_captures_expr(left, scope_mutable);
            widen_mutable_captures_expr(right, scope_mutable);
        }
        Expr::Unary { operand, .. } => widen_mutable_captures_expr(operand, scope_mutable),
        Expr::Call { callee, args, .. } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        widen_mutable_captures_expr(e, scope_mutable)
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        widen_mutable_captures_expr(x, scope_mutable)
                    }
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                widen_mutable_captures_expr(v, scope_mutable);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                widen_mutable_captures_expr(v, scope_mutable);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_expr(then_expr, scope_mutable);
            widen_mutable_captures_expr(else_expr, scope_mutable);
        }
        Expr::PropertyGet { object, .. } => widen_mutable_captures_expr(object, scope_mutable),
        Expr::PropertySet { object, value, .. } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::PropertyUpdate { object, .. } => widen_mutable_captures_expr(object, scope_mutable),
        Expr::IndexGet { object, index } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::IndexUpdate { object, index, .. } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
        }
        Expr::New { args, .. } => {
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::NewDynamic { callee, args } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            widen_mutable_captures_expr(inner, scope_mutable);
        }
        Expr::InstanceOf { expr, .. } => widen_mutable_captures_expr(expr, scope_mutable),
        Expr::In { property, object } => {
            widen_mutable_captures_expr(property, scope_mutable);
            widen_mutable_captures_expr(object, scope_mutable);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(callback, scope_mutable);
        }
        Expr::ArraySort { array, comparator } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(comparator, scope_mutable);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(callback, scope_mutable);
            if let Some(init) = initial {
                widen_mutable_captures_expr(init, scope_mutable);
            }
        }
        Expr::ArrayToReversed { array } => {
            widen_mutable_captures_expr(array, scope_mutable);
        }
        Expr::ArrayToSorted { array, comparator } => {
            widen_mutable_captures_expr(array, scope_mutable);
            if let Some(cmp) = comparator {
                widen_mutable_captures_expr(cmp, scope_mutable);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            widen_mutable_captures_expr(delete_count, scope_mutable);
            for item in items {
                widen_mutable_captures_expr(item, scope_mutable);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            widen_mutable_captures_expr(target, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            if let Some(e) = end {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            widen_mutable_captures_expr(array, scope_mutable);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                widen_mutable_captures_expr(obj, scope_mutable);
            }
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            widen_mutable_captures_expr(closure, scope_mutable)
        }
        Expr::ArrayPush { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
            widen_mutable_captures_expr(value, scope_mutable);
        }
        _ => {}
    }
}

/// Walk a statement collecting the set of LocalIds that are assigned to
/// inside any `Expr::Closure` reachable from it (including nested closures).
/// This is the "mutably shared" set at the enclosing lexical scope.
fn collect_closure_assigned_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_assigned_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_assigned_expr(condition, out);
            for s in then_branch {
                collect_closure_assigned_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_assigned_expr(condition, out);
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_assigned_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_assigned_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_assigned_expr(upd, out);
            }
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_closure_assigned_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_assigned_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_assigned_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_assigned_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_assigned_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { body, .. } => {
            // Any LocalSet/Update inside this closure body (or nested closures
            // within it) counts as "assigned in a closure at our scope".
            for stmt in body {
                collect_closure_assigned_in_closure_body_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_assigned_expr(left, out);
            collect_closure_assigned_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_assigned_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_closure_assigned_expr(e, out),
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_assigned_expr(x, out)
                    }
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_assigned_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_assigned_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_assigned_expr(condition, out);
            collect_closure_assigned_expr(then_expr, out);
            collect_closure_assigned_expr(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_assigned_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => collect_closure_assigned_expr(object, out),
        Expr::IndexGet { object, index } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            collect_closure_assigned_expr(value, out);
        }
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            collect_closure_assigned_expr(inner, out);
        }
        Expr::InstanceOf { expr, .. } => collect_closure_assigned_expr(expr, out),
        Expr::In { property, object } => {
            collect_closure_assigned_expr(property, out);
            collect_closure_assigned_expr(object, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(callback, out);
        }
        Expr::ArraySort { array, comparator } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(comparator, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_assigned_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } => {
            collect_closure_assigned_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_assigned_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_assigned_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(start, out);
            collect_closure_assigned_expr(delete_count, out);
            for item in items {
                collect_closure_assigned_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(index, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_assigned_expr(target, out);
            collect_closure_assigned_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_assigned_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_assigned_expr(obj, out);
            }
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => collect_closure_assigned_expr(closure, out),
        _ => {}
    }
}

/// Collect all LocalIds that appear in the `captures` list of any closure in the scope.
fn collect_closure_captures_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_captures_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_captures_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_captures_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_captures_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_captures_expr(condition, out);
            for s in then_branch {
                collect_closure_captures_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_captures_expr(condition, out);
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_captures_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_captures_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_captures_expr(upd, out);
            }
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    collect_closure_captures_stmt(s, out);
                }
            }
            if let Some(fs) = finally {
                for s in fs {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_captures_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_captures_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_captures_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_captures_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { captures, body, .. } => {
            for id in captures {
                out.insert(*id);
            }
            // Also recurse into nested closures
            for stmt in body {
                collect_closure_captures_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_captures_expr(left, out);
            collect_closure_captures_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_captures_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_captures_expr(callee, out);
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_captures_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_closure_captures_expr(e, out),
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_captures_expr(x, out)
                    }
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_captures_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_captures_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_captures_expr(condition, out);
            collect_closure_captures_expr(then_expr, out);
            collect_closure_captures_expr(else_expr, out);
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            collect_closure_captures_expr(value, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_captures_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => {
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::ArrayPush { value, .. }
        | Expr::Await(value)
        | Expr::TypeOf(value)
        | Expr::Void(value)
        | Expr::Delete(value) => {
            collect_closure_captures_expr(value, out);
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(callback, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_captures_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } => {
            collect_closure_captures_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_captures_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_captures_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(start, out);
            collect_closure_captures_expr(delete_count, out);
            for item in items {
                collect_closure_captures_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_captures_expr(target, out);
            collect_closure_captures_expr(start, out);
            if let Some(e) = end {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_captures_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_captures_expr(obj, out);
            }
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => collect_closure_captures_expr(closure, out),
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_captures_expr(e, out);
            }
        }
        _ => {}
    }
}

/// Collect LocalIds that are assigned to at the current scope level
/// (via LocalSet or Update), but NOT inside closure bodies.
fn collect_scope_level_assigns_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_scope_level_assigns_expr(expr, out),
        Stmt::Expr(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Throw(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in then_branch {
                collect_scope_level_assigns_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_scope_level_assigns_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_scope_level_assigns_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_scope_level_assigns_expr(upd, out);
            }
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
            if let Some(fs) = finally {
                for s in fs {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_scope_level_assigns_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_scope_level_assigns_expr(test, out);
                }
                for s in &case.body {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_scope_level_assigns_stmt(body, out),
        _ => {}
    }
}

fn collect_scope_level_assigns_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_scope_level_assigns_expr(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        // Do NOT recurse into closures — we only want scope-level assignments
        Expr::Closure { .. } => {}
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_scope_level_assigns_expr(left, out);
            collect_scope_level_assigns_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_scope_level_assigns_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_scope_level_assigns_expr(callee, out);
            for arg in args {
                collect_scope_level_assigns_expr(arg, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_scope_level_assigns_expr(condition, out);
            collect_scope_level_assigns_expr(then_expr, out);
            collect_scope_level_assigns_expr(else_expr, out);
        }
        _ => {}
    }
}

/// Walk a closure body collecting every LocalSet/Update target AND any
/// assigns inside nested closures within this body.
fn collect_closure_assigned_in_closure_body_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_assigned_in_body_expr(condition, out);
            for s in then_branch {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_assigned_in_body_expr(condition, out);
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_assigned_in_closure_body_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_assigned_in_body_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_assigned_in_body_expr(upd, out);
            }
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_assigned_in_body_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_assigned_in_body_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_assigned_in_closure_body_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_assigned_in_body_expr(
    expr: &Expr,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match expr {
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Closure { body, .. } => {
            for stmt in body {
                collect_closure_assigned_in_closure_body_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_assigned_in_body_expr(left, out);
            collect_closure_assigned_in_body_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_assigned_in_body_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        collect_closure_assigned_in_body_expr(e, out)
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_assigned_in_body_expr(x, out)
                    }
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_assigned_in_body_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_assigned_in_body_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_assigned_in_body_expr(condition, out);
            collect_closure_assigned_in_body_expr(then_expr, out);
            collect_closure_assigned_in_body_expr(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_assigned_in_body_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => collect_closure_assigned_in_body_expr(object, out),
        Expr::IndexGet { object, index } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::GlobalSet(_, value) => collect_closure_assigned_in_body_expr(value, out),
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            collect_closure_assigned_in_body_expr(inner, out);
        }
        Expr::InstanceOf { expr, .. } => collect_closure_assigned_in_body_expr(expr, out),
        Expr::In { property, object } => {
            collect_closure_assigned_in_body_expr(property, out);
            collect_closure_assigned_in_body_expr(object, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(callback, out);
        }
        Expr::ArraySort { array, comparator } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(comparator, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_assigned_in_body_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } => {
            collect_closure_assigned_in_body_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_assigned_in_body_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_assigned_in_body_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(start, out);
            collect_closure_assigned_in_body_expr(delete_count, out);
            for item in items {
                collect_closure_assigned_in_body_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(index, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_assigned_in_body_expr(target, out);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_assigned_in_body_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_assigned_in_body_expr(obj, out);
            }
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            collect_closure_assigned_in_body_expr(closure, out)
        }
        // Array mutation methods may reallocate the array pointer, so they
        // count as assignments to the array_id for mutable-capture widening.
        Expr::ArrayPush { array_id, value }
        | Expr::ArrayUnshift { array_id, value }
        | Expr::ArrayPushSpread {
            array_id,
            source: value,
        } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            out.insert(*array_id);
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(dc) = delete_count {
                collect_closure_assigned_in_body_expr(dc, out);
            }
            for item in items {
                collect_closure_assigned_in_body_expr(item, out);
            }
        }
        _ => {}
    }
}

fn append_legacy_decorator_init_for_class(
    ctx: &mut LoweringContext,
    init: &mut Vec<Stmt>,
    class: &Class,
) {
    let has_param_decorators = class
        .constructor
        .as_ref()
        .map(|ctor| ctor.params.iter().any(|p| !p.decorators.is_empty()))
        .unwrap_or(false);
    let has_method_decorators = class.methods.iter().any(method_has_legacy_decorators)
        || class
            .static_methods
            .iter()
            .any(method_has_legacy_decorators);
    let has_property_decorators = class.fields.iter().any(field_has_legacy_decorators)
        || class.static_fields.iter().any(field_has_legacy_decorators);
    if class.decorators.is_empty()
        && !has_param_decorators
        && !has_method_decorators
        && !has_property_decorators
    {
        return;
    }

    if let Some(ctor) = &class.constructor {
        let param_types = ctor
            .params
            .iter()
            .map(|p| type_metadata_expr(&p.ty))
            .collect();
        init.push(Stmt::Expr(Expr::ReflectDefineMetadata {
            key: Box::new(Expr::String("design:paramtypes".to_string())),
            value: Box::new(Expr::Array(param_types)),
            target: Box::new(Expr::ClassRef(class.name.clone())),
            property_key: None,
        }));
    }

    if let Some(ctor) = &class.constructor {
        for (index, param) in ctor.params.iter().enumerate().rev() {
            append_decorator_invocations(
                ctx,
                init,
                &param.decorators,
                vec![
                    Expr::ClassRef(class.name.clone()),
                    Expr::Undefined,
                    Expr::Number(index as f64),
                ],
            );
        }
    }

    for method in &class.methods {
        append_method_decorator_init(ctx, init, class, method);
    }
    for method in &class.static_methods {
        append_method_decorator_init(ctx, init, class, method);
    }
    for field in &class.fields {
        append_property_decorator_init(ctx, init, class, field);
    }
    for field in &class.static_fields {
        append_property_decorator_init(ctx, init, class, field);
    }

    append_class_decorator_invocations(
        ctx,
        init,
        &class.decorators,
        &class.name,
        vec![Expr::ClassRef(class.name.clone())],
    );
}

fn method_has_legacy_decorators(method: &Function) -> bool {
    !method.decorators.is_empty() || method.params.iter().any(|p| !p.decorators.is_empty())
}

fn field_has_legacy_decorators(field: &ClassField) -> bool {
    !field.decorators.is_empty()
}

fn append_property_decorator_init(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    class: &Class,
    field: &ClassField,
) {
    if field.decorators.is_empty() {
        return;
    }

    out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
        key: Box::new(Expr::String("design:type".to_string())),
        value: Box::new(type_metadata_expr(&field.ty)),
        target: Box::new(Expr::ClassRef(class.name.clone())),
        property_key: Some(Box::new(Expr::String(field.name.clone()))),
    }));

    append_decorator_invocations(
        ctx,
        out,
        &field.decorators,
        vec![
            Expr::ClassRef(class.name.clone()),
            Expr::String(field.name.clone()),
        ],
    );
}

fn append_method_decorator_init(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    class: &Class,
    method: &Function,
) {
    if !method_has_legacy_decorators(method) {
        return;
    }

    if method.params.iter().any(|p| !p.decorators.is_empty()) || !method.decorators.is_empty() {
        let param_types = method
            .params
            .iter()
            .map(|p| type_metadata_expr(&p.ty))
            .collect();
        out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
            key: Box::new(Expr::String("design:paramtypes".to_string())),
            value: Box::new(Expr::Array(param_types)),
            target: Box::new(Expr::ClassRef(class.name.clone())),
            property_key: Some(Box::new(Expr::String(method.name.clone()))),
        }));
    }

    for (index, param) in method.params.iter().enumerate().rev() {
        append_decorator_invocations(
            ctx,
            out,
            &param.decorators,
            vec![
                Expr::ClassRef(class.name.clone()),
                Expr::String(method.name.clone()),
                Expr::Number(index as f64),
            ],
        );
    }

    let descriptor = Expr::Object(vec![(
        "value".to_string(),
        Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_class_prototype_method_value".to_string(),
                param_types: Vec::new(),
                return_type: Type::Any,
            }),
            args: vec![
                Expr::ClassRef(class.name.clone()),
                Expr::String(method.name.clone()),
            ],
            type_args: Vec::new(),
        },
    )]);
    append_decorator_invocations(
        ctx,
        out,
        &method.decorators,
        vec![
            Expr::ClassRef(class.name.clone()),
            Expr::String(method.name.clone()),
            descriptor,
        ],
    );
}

fn append_decorator_invocations(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    invocation_args: Vec<Expr>,
) {
    append_decorator_invocations_inner(ctx, out, decorators, invocation_args, None);
}

/// Same as `append_decorator_invocations`, but each non-`Reflect.metadata`
/// invocation captures its return value and throws a `TypeError` if it is
/// anything other than `undefined`. Used for class decorators, where TS
/// allows the decorator to return a replacement class but Perry does not
/// install the replacement (the lowered class is fixed in the IR). Throwing
/// on a non-`undefined` return surfaces the silent-failure case the
/// maintainer flagged on PR #754 (`@Memoize`, `@Throttle`, GraphQL resolver
/// wrappers, etc.).
fn append_class_decorator_invocations(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    class_name: &str,
    invocation_args: Vec<Expr>,
) {
    append_decorator_invocations_inner(ctx, out, decorators, invocation_args, Some(class_name));
}

fn append_decorator_invocations_inner(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    invocation_args: Vec<Expr>,
    class_name_for_replacement_check: Option<&str>,
) {
    let mut callees: Vec<(Expr, String)> = Vec::with_capacity(decorators.len());
    for dec in decorators {
        if dec.is_reflect_metadata {
            append_reflect_metadata_decorator(out, dec, &invocation_args);
            continue;
        }
        let base = decorator_callee_expr(ctx, &dec.name);
        if dec.is_factory {
            let temp_id = ctx.fresh_local();
            out.push(Stmt::Let {
                id: temp_id,
                name: format!("__perry_dec_{}", temp_id),
                ty: Type::Function(FunctionType {
                    params: Vec::new(),
                    return_type: Box::new(Type::Any),
                    is_async: false,
                    is_generator: false,
                }),
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(base),
                    args: dec.args.clone(),
                    type_args: Vec::new(),
                }),
            });
            callees.push((Expr::LocalGet(temp_id), dec.name.clone()));
        } else {
            callees.push((base, dec.name.clone()));
        }
    }

    for (callee, dec_name) in callees.into_iter().rev() {
        let call = Expr::Call {
            callee: Box::new(callee),
            args: invocation_args.clone(),
            type_args: Vec::new(),
        };
        match class_name_for_replacement_check {
            Some(class_name) => {
                let ret_id = ctx.fresh_local();
                out.push(Stmt::Let {
                    id: ret_id,
                    name: format!("__perry_dec_ret_{}", ret_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(call),
                });
                let msg = format!(
                    "Class decorator `@{dec_name}` on `{class_name}` returned a value. \
Perry does not install decorator return values as class replacements (see \
docs/src/language/decorators.md). Return `undefined` (or nothing) to keep \
the decorator running for side effects only."
                );
                // Issue #832: also accept the identity return (`return target`)
                // as a no-op — node under `--experimental-strip-types` ignores
                // decorator return values entirely, and many real-world
                // decorators (factory output that hasn't been simplified, the
                // trivial `return target` no-op) hit this. Only the *actual*
                // class-replacement case (`return SomeOtherFn`) still throws.
                // The target is the first invocation arg (`Expr::ClassRef` for
                // class decorators); if it's not available, fall back to the
                // original strict undefined-only check.
                let ret_get = Expr::LocalGet(ret_id);
                let ne_undef = Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(ret_get.clone()),
                    right: Box::new(Expr::Undefined),
                };
                let condition = match invocation_args.first() {
                    Some(target_expr) => Expr::Logical {
                        op: LogicalOp::And,
                        left: Box::new(ne_undef),
                        right: Box::new(Expr::Compare {
                            op: CompareOp::Ne,
                            left: Box::new(ret_get),
                            right: Box::new(target_expr.clone()),
                        }),
                    },
                    None => ne_undef,
                };
                out.push(Stmt::If {
                    condition,
                    // Perry has dedicated HIR variants for built-in errors
                    // (`Expr::TypeErrorNew`, etc.); the generic
                    // `Expr::New { class_name: "TypeError" }` path falls
                    // through to an empty-object placeholder and prints
                    // `Uncaught exception: [object] (bits=…)`.
                    then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(msg))))],
                    else_branch: None,
                });
            }
            None => {
                out.push(Stmt::Expr(call));
            }
        }
    }
}

fn append_reflect_metadata_decorator(
    out: &mut Vec<Stmt>,
    dec: &Decorator,
    invocation_args: &[Expr],
) {
    let key = dec.args.first().cloned().unwrap_or(Expr::Undefined);
    let value = dec.args.get(1).cloned().unwrap_or(Expr::Undefined);
    let target = invocation_args.first().cloned().unwrap_or(Expr::Undefined);
    let property_key = invocation_args.get(1).cloned().and_then(|arg| {
        if matches!(arg, Expr::Undefined) {
            None
        } else {
            Some(Box::new(arg))
        }
    });
    out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
        key: Box::new(key),
        value: Box::new(value),
        target: Box::new(target),
        property_key,
    }));
}

fn type_metadata_expr(ty: &Type) -> Expr {
    match ty {
        Type::Named(name) => Expr::ClassRef(name.clone()),
        Type::Generic { base, .. } => Expr::ClassRef(base.clone()),
        Type::Array(_) | Type::Tuple(_) => Expr::ClassRef("Array".to_string()),
        Type::String => Expr::ClassRef("String".to_string()),
        Type::Number | Type::Int32 | Type::BigInt => Expr::ClassRef("Number".to_string()),
        Type::Boolean => Expr::ClassRef("Boolean".to_string()),
        Type::Object(_) => Expr::ClassRef("Object".to_string()),
        Type::Function(_) => Expr::ClassRef("Function".to_string()),
        _ => Expr::Undefined,
    }
}

fn decorator_callee_expr(ctx: &LoweringContext, name: &str) -> Expr {
    if let Some(id) = ctx.lookup_func(name) {
        Expr::FuncRef(id)
    } else if let Some(orig_name) = ctx.lookup_imported_func(name) {
        let (param_types, return_type) = ctx
            .lookup_extern_func_types(orig_name)
            .map(|(p, r)| (p.clone(), r.clone()))
            .unwrap_or_else(|| (Vec::new(), Type::Any));
        Expr::ExternFuncRef {
            name: orig_name.to_string(),
            param_types,
            return_type,
        }
    } else {
        Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }
    }
}

/// Emit a one-shot note when the user imports `reflect-metadata`. Perry
/// ships a built-in subset that's enough for the Nest-style decorator
/// metadata canaries (see docs/src/language/decorators.md), but is NOT
/// the full npm package's surface — `class-validator` and `TypeORM` reach
/// into corners that aren't shimmed. Telling the user up-front avoids the
/// silent-failure surprise where a polyfill they `import`-ed turns out
/// not to be the polyfill they expected.
fn emit_reflect_metadata_shim_note() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static EMITTED: AtomicBool = AtomicBool::new(false);
    if EMITTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[perry] note: `import \"reflect-metadata\"` is satisfied by Perry's built-in \
metadata subset. Implemented surface: Reflect.defineMetadata, getMetadata, \
getOwnMetadata, hasMetadata, hasOwnMetadata, getMetadataKeys, \
getOwnMetadataKeys, deleteMetadata, and @Reflect.metadata(...). \
Anything outside this surface (Symbol-keyed metadata, the full reflect-metadata \
runtime used by class-validator/TypeORM) is not provided. \
See docs/src/language/decorators.md."
    );
}

fn lower_module_decl(
    ctx: &mut LoweringContext,
    module: &mut Module,
    decl: &ast::ModuleDecl,
) -> Result<()> {
    match decl {
        ast::ModuleDecl::Import(import_decl) => {
            // Get the source module path
            let raw_source = import_decl.src.value.as_str().unwrap_or("").to_string();
            // Normalize "node:" prefix (e.g., "node:async_hooks" -> "async_hooks")
            let source = raw_source
                .strip_prefix("node:")
                .unwrap_or(&raw_source)
                .to_string();

            if source == "reflect-metadata" {
                emit_reflect_metadata_shim_note();
                return Ok(());
            }

            // Check if this is a native module import
            let is_native = is_native_module(&source);

            // Native modules have no class metadata to extract — `node:fs`,
            // `node:path`, etc. produce no `ImportedClass` entries and the
            // runtime can't resolve type-only names anyway. Skip the whole
            // declaration for native modules.
            //
            // For TypeScript modules, fall through and process the
            // declaration even when `type_only` is set: the named-specifier
            // loop below treats a whole-`import type { ... }` like a
            // per-specifier `import { type ... }` (which already flowed
            // class info into `imported_classes` since the v0.5.405 fix).
            // Without this, `import type { Foo }` dropped Foo's class
            // metadata before it reached compile.rs, so codegen lost the
            // method registry — `obj.method()` worked only via the
            // CLASS_VTABLE_REGISTRY runtime fallback (#392 followup) and
            // `typeof obj.method` returned `"undefined"`. Issue #446.
            if import_decl.type_only && is_native {
                return Ok(());
            }
            let whole_decl_type_only = import_decl.type_only;

            // Parse import specifiers
            let mut specifiers = Vec::new();
            for spec in &import_decl.specifiers {
                match spec {
                    ast::ImportSpecifier::Named(named) => {
                        // Skip individual type-only specifiers (`import { type Foo,
                        // Bar }`) only for native module imports — there's no
                        // class info to extract from `node:fs`, etc., and the
                        // runtime can't resolve them anyway. For TypeScript
                        // module imports, fall through and treat them as regular
                        // named imports so the source module's class info
                        // flows into `imported_classes` (for method dispatch on
                        // a `q: Query` typed local) and `cross_module_class_field_types`
                        // (for type inference on `someLocal.field` for-of
                        // iteration). Pre-fix, `import { type Query } from
                        // "../src"` made `q.forEach(...)` a silent no-op
                        // because Query was never registered with codegen —
                        // the spread call on the closure invoked a stub that
                        // returned undefined. ECS demo-simple repro.
                        let spec_type_only = named.is_type_only || whole_decl_type_only;
                        if spec_type_only && is_native {
                            continue;
                        }
                        let local = named.local.sym.to_string();
                        let imported = named
                            .imported
                            .as_ref()
                            .map(|i| match i {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            })
                            .unwrap_or_else(|| local.clone());
                        if is_native {
                            // Register as native module function with the original method name
                            // e.g., import { v4 as uuid } from 'uuid' -> uuid maps to uuid.v4
                            ctx.register_native_module(
                                local.clone(),
                                source.clone(),
                                Some(imported.clone()),
                            );
                            // Auto-register parentPort from worker_threads as a native instance
                            // (it's a singleton, not created via `new`)
                            if source == "worker_threads" && imported == "parentPort" {
                                ctx.register_native_instance(
                                    local.clone(),
                                    "worker_threads".to_string(),
                                    "MessagePort".to_string(),
                                );
                            }
                        } else {
                            // Register as imported function (we assume all imports are functions for now)
                            ctx.register_imported_func(local.clone(), imported.clone());
                        }
                        specifiers.push(ImportSpecifier::Named { imported, local });
                    }
                    ast::ImportSpecifier::Default(default) => {
                        let local = default.local.sym.to_string();
                        if is_native {
                            // Default import of native module (e.g., import mysql from 'mysql2/promise')
                            // Default exports don't have a method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                        } else {
                            // Default import from JS module — register so calls resolve to
                            // ExternFuncRef. Use the LOCAL name as the original-name marker
                            // (identity registration) so the resulting ExternFuncRef carries a
                            // per-import-site UNIQUE name. Before this, every default import
                            // registered `(local, "default")`, so two `import X from "./a";
                            // import Y from "./b"` in the same file both lowered to
                            // `ExternFuncRef { name: "default" }` and the codegen's flat
                            // `import_function_prefixes` lookup keyed on "default" collided —
                            // whichever module's prefix landed last in the HashMap won, and
                            // both `X` and `Y` resolved to the SAME source module. Pino's
                            // CJS wrap exposed this: `const { SORTING_ORDER } =
                            // require('./lib/constants')` and `const { ... } =
                            // require('./lib/tools')` get hoisted as
                            // `import _req_9 from './lib/constants'; import _req_10 from
                            // './lib/tools'`, and pino.js's IIFE call `require('./lib/constants')`
                            // returned tools' exports, throwing
                            // `TypeError: Cannot read properties of undefined (reading 'ASC')`.
                            //
                            // The local name is unique per import site, so the resulting
                            // `ExternFuncRef { name: <local> }` looks up correctly against the
                            // CLI's `import_function_prefixes` (which already inserts both
                            // `exported_name="default"` AND `local_name=<unique>`). The CLI's
                            // companion insert at `crates/perry/src/commands/compile.rs`
                            // (right after the `import_function_prefixes` insert pair for
                            // Default specifiers) places `local_name → "default"` into
                            // `import_function_origin_names`, so the codegen's symbol
                            // construction at `lower_call.rs` / `expr.rs` builds
                            // `perry_fn_<src>__default` — matching what the origin module
                            // actually emits. Closes #901.
                            ctx.register_imported_func(local.clone(), local.clone());
                        }
                        specifiers.push(ImportSpecifier::Default { local });
                    }
                    ast::ImportSpecifier::Namespace(ns) => {
                        let local = ns.local.sym.to_string();
                        if is_native {
                            // Namespace import of native module (e.g., import * as mysql from 'mysql2')
                            // Methods are called via the namespace, so no specific method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                            // Also register as builtin module alias so method-level
                            // recognition works (child_process, fs, os, etc.)
                            ctx.register_builtin_module_alias(local.clone(), source.clone());
                        } else {
                            // Namespace import from JS module - register so calls resolve to ExternFuncRef
                            ctx.register_imported_func(local.clone(), local.clone());
                        }
                        specifiers.push(ImportSpecifier::Namespace { local });
                    }
                }
            }

            // Determine module kind based on the source and whether it's native
            let module_kind = if is_native {
                ModuleKind::NativeRust
            } else {
                // Default to NativeCompiled - the compiler driver will update this
                // based on file resolution
                ModuleKind::NativeCompiled
            };

            module.imports.push(Import {
                source,
                specifiers,
                is_native,
                module_kind,
                resolved_path: None, // Will be set by compiler driver during module resolution
                type_only: whole_decl_type_only,
                is_dynamic: false,
            });
        }
        ast::ModuleDecl::ExportDecl(export) => {
            match &export.decl {
                ast::Decl::Fn(fn_decl) => {
                    // Skip overload signatures (no body) — they share the same func_id
                    // as the implementation. Pushing them to module.functions would cause
                    // codegen to compile the empty-body overload and skip the real implementation.
                    if fn_decl.function.body.is_none() {
                        return Ok(());
                    }
                    let mut func = lower_fn_decl(ctx, fn_decl)?;
                    func.is_exported = true;
                    let func_name = func.name.clone();
                    let func_id = func.id;
                    // Register return type for call-site inference
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func_name.clone(), func.return_type.clone());
                    }
                    // If the declared return type maps to a native instance
                    // (e.g. `function openSocket(): Socket { ... }`), register
                    // the function as a factory so call sites can pick up
                    // the instance class — see lookup_func_return_native_instance.
                    if let Some((module, class)) =
                        native_instance_from_return_type(&func.return_type)
                    {
                        ctx.func_return_native_instances.push((
                            func_name.clone(),
                            module.to_string(),
                            class.to_string(),
                        ));
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> =
                        func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    let rest_idx = func.params.iter().position(|p| p.is_rest);
                    ctx.func_defaults
                        .push((func.id, defaults, param_ids, rest_idx));
                    module.functions.push(func);
                    // Track in exports
                    module.exports.push(Export::Named {
                        local: func_name.clone(),
                        exported: func_name.clone(),
                    });
                    // Track exported function for cross-module value passing
                    module.exported_functions.push((func_name, func_id));
                }
                ast::Decl::Var(var_decl) => {
                    // Handle exported variables
                    for decl in &var_decl.decls {
                        let name = get_binding_name(&decl.name)?;
                        let ty = extract_binding_type(&decl.name);
                        if let Some(init) = &decl.init {
                            // Check if this is a native class instantiation and register it.
                            // Mirrors the destructuring.rs path: first try the
                            // general `lookup_native_module` (covers any class
                            // imported from a known native module), then fall
                            // back to a small hardcoded map for global-style
                            // names. Pool/Client/MongoClient are intentionally
                            // NOT in the fallback — those names collide with
                            // user classes and TS-source npm packages (e.g.
                            // `@perryts/mysql` exports its own `Pool`); the
                            // legitimate `import { Pool } from "pg"` flow is
                            // caught by the general lookup above. (Issue #536.)
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name = class_ident.sym.as_ref();
                                    // If the user has declared a class with this
                                    // name in the current module, it shadows the
                                    // hardcoded library-name fallback below — the
                                    // user-defined class wins. Without this gate
                                    // a user's `class Big { f0=0; ... }` was
                                    // routed through `big.js`'s handle-based
                                    // method dispatch (returning 0 for every
                                    // unknown property), so reads of any field
                                    // returned 0. Same shadowing logic applies
                                    // to `Decimal`, `BigNumber`, etc.
                                    let user_class_defined = module
                                        .classes
                                        .iter()
                                        .any(|c| c.name == class_name)
                                        || ctx.pending_classes.iter().any(|c| c.name == class_name);
                                    let module_name: Option<String> = if let Some((m, _)) =
                                        ctx.lookup_native_module(class_name)
                                    {
                                        Some(m.to_string())
                                    } else if user_class_defined {
                                        None
                                    } else {
                                        match class_name {
                                            "EventEmitter" => Some("events".to_string()),
                                            "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                            "AsyncResource" => Some("async_hooks".to_string()),
                                            "WebSocket" | "WebSocketServer" => {
                                                Some("ws".to_string())
                                            }
                                            "Redis" => Some("ioredis".to_string()),
                                            "LRUCache" => Some("lru-cache".to_string()),
                                            "Command" => Some("commander".to_string()),
                                            "Big" => Some("big.js".to_string()),
                                            "Decimal" => Some("decimal.js".to_string()),
                                            "BigNumber" => Some("bignumber.js".to_string()),
                                            _ => None,
                                        }
                                    };
                                    // Issue #848: StringDecoder runs entirely through
                                    // HANDLE_METHOD_DISPATCH / HANDLE_PROPERTY_DISPATCH
                                    // — registering it as a typed native instance would
                                    // re-route `d.write` (property read) through the
                                    // NativeMethodCall-with-empty-args path that
                                    // pre-invokes the FFI as a getter, so
                                    // `typeof d.write === "function"` would silently
                                    // become `"number"` (the empty-string write return,
                                    // misclassified). Skipping the registration lets
                                    // the regular PropertyGet path fall into
                                    // HANDLE_PROPERTY_DISPATCH which returns the bound-
                                    // method closure built by `js_class_method_bind` —
                                    // `typeof` reads `"function"`, and the eventual
                                    // call routes through HANDLE_METHOD_DISPATCH back
                                    // to the same `dispatch_string_decoder` impl.
                                    let module_name = match (class_name, module_name.as_deref()) {
                                        ("StringDecoder", Some("string_decoder")) => None,
                                        _ => module_name,
                                    };
                                    if let Some(native_module) = module_name {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            native_module,
                                            class_name.to_string(),
                                        );
                                    }
                                }
                            }

                            // Check if this is an awaited native class instantiation (e.g., await new Redis())
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref()
                                    {
                                        let class_name = class_ident.sym.as_ref();
                                        // Same user-class shadowing rule as the
                                        // non-await new-expr path above.
                                        let user_class_defined =
                                            module.classes.iter().any(|c| c.name == class_name)
                                                || ctx
                                                    .pending_classes
                                                    .iter()
                                                    .any(|c| c.name == class_name);
                                        let module_name: Option<String> = if let Some((m, _)) =
                                            ctx.lookup_native_module(class_name)
                                        {
                                            Some(m.to_string())
                                        } else if user_class_defined {
                                            None
                                        } else {
                                            match class_name {
                                                "EventEmitter" => Some("events".to_string()),
                                                "AsyncLocalStorage" => {
                                                    Some("async_hooks".to_string())
                                                }
                                                "AsyncResource" => Some("async_hooks".to_string()),
                                                "WebSocket" | "WebSocketServer" => {
                                                    Some("ws".to_string())
                                                }
                                                "Redis" => Some("ioredis".to_string()),
                                                "LRUCache" => Some("lru-cache".to_string()),
                                                "Command" => Some("commander".to_string()),
                                                "Big" => Some("big.js".to_string()),
                                                "Decimal" => Some("decimal.js".to_string()),
                                                "BigNumber" => Some("bignumber.js".to_string()),
                                                _ => None,
                                            }
                                        };
                                        // Issue #848: StringDecoder runs entirely through
                                        // HANDLE_*_DISPATCH (see the gate on the sync path
                                        // above for the full rationale). Defensive mirror
                                        // on this awaited-new branch so the same skip
                                        // applies to `await new StringDecoder(...)` —
                                        // which is unusual but legal TS.
                                        let module_name = match (class_name, module_name.as_deref())
                                        {
                                            ("StringDecoder", Some("string_decoder")) => None,
                                            _ => module_name,
                                        };
                                        if let Some(native_module) = module_name {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                native_module,
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }

                            // Check if this is a native module factory function call (e.g., mysql.createPool())
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let ast::Callee::Expr(callee) = &call_expr.callee {
                                    if let ast::Expr::Member(member) = callee.as_ref() {
                                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                            let obj_name = obj_ident.sym.as_ref();
                                            // Check if it's a known native module
                                            // Clone module_name to avoid borrow conflict with ctx mutation below
                                            let native_mod = ctx
                                                .lookup_native_module(obj_name)
                                                .map(|(m, _)| m.to_string());
                                            if let Some(module_name_owned) = native_mod {
                                                if let ast::MemberProp::Ident(method_ident) =
                                                    &member.prop
                                                {
                                                    let method_name = method_ident.sym.as_ref();
                                                    // Map factory functions to their class names
                                                    let class_name = match (
                                                        module_name_owned.as_str(),
                                                        method_name,
                                                    ) {
                                                        (
                                                            "mysql2" | "mysql2/promise",
                                                            "createPool",
                                                        ) => Some("Pool"),
                                                        (
                                                            "mysql2" | "mysql2/promise",
                                                            "createConnection",
                                                        ) => Some("Connection"),
                                                        ("pg", "connect") => Some("Client"),
                                                        ("http" | "https", "request" | "get") => {
                                                            Some("ClientRequest")
                                                        }
                                                        // net.createConnection(host, port) returns a Socket handle.
                                                        // Without registering this, subsequent `sock.write/on/end/destroy`
                                                        // calls fall through to dynamic dispatch and never reach
                                                        // the `js_net_socket_*` FFI functions.
                                                        ("net", "createConnection") => {
                                                            Some("Socket")
                                                        }
                                                        // node-cron's `cron.schedule(expr, cb)` returns a job
                                                        // handle whose `start()`/`stop()`/`isRunning()` etc.
                                                        // dispatch via the ("node-cron", true, METHOD) entries
                                                        // in expr.rs's native_module dispatch table. Without
                                                        // registering the handle as a "CronJob" native instance,
                                                        // `job.stop()` falls through to dynamic dispatch and the
                                                        // stop never reaches js_cron_job_stop.
                                                        ("node-cron", "schedule") => {
                                                            Some("CronJob")
                                                        }
                                                        // node:http server (issue #577) — `http.createServer(...)`
                                                        // returns an HttpServer handle whose `.listen(...)` /
                                                        // `.close()` / `.address()` / `.on(...)` dispatches via
                                                        // the ("http", true, *) class_filter=Some("HttpServer")
                                                        // entries in NATIVE_MODULE_TABLE.
                                                        ("http", "createServer") => {
                                                            Some("HttpServer")
                                                        }
                                                        ("https", "createServer") => {
                                                            Some("HttpsServer")
                                                        }
                                                        ("http2", "createSecureServer") => {
                                                            Some("Http2SecureServer")
                                                        }
                                                        ("async_hooks", "createHook") => {
                                                            Some("AsyncHook")
                                                        }
                                                        _ => None,
                                                    };
                                                    if let Some(class_name) = class_name {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            module_name_owned.clone(),
                                                            class_name.to_string(),
                                                        );
                                                        // Also register as module-level native instance so it survives scope exits.
                                                        // Without this, pool = mysql.createPool() at module top level loses
                                                        // its native tracking when function scopes are entered/exited,
                                                        // causing pool.query() inside functions to miss the Pool dispatch.
                                                        ctx.module_native_instances.push((
                                                            name.clone(),
                                                            module_name_owned,
                                                            class_name.to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Check if this is a direct call to a default import from a native module
                                    // e.g., Fastify() where Fastify is imported from 'fastify'
                                    if let ast::Expr::Ident(func_ident) = callee.as_ref() {
                                        let func_name = func_ident.sym.as_ref();
                                        // Check if this is a default import from a native module
                                        if let Some((module_name, None)) =
                                            ctx.lookup_native_module(func_name)
                                        {
                                            // Register as native instance - the "class" is the module name for default exports
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name.to_string(),
                                                "App".to_string(),
                                            );
                                        }
                                        // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                                        if let Some((module_name, Some(method_name))) =
                                            ctx.lookup_native_module(func_name)
                                        {
                                            if module_name == "perry/ui" {
                                                match method_name {
                                                    "Canvas" | "State" | "Sheet" | "Toolbar"
                                                    | "Window" | "LazyVStack"
                                                    | "NavigationStack" | "Picker" | "Table"
                                                    | "TabBar" => {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            module_name.to_string(),
                                                            method_name.to_string(),
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        // node:http server (issue #577) — named-import factory:
                                        // `import { createServer } from "node:http"; const s = createServer(...)`.
                                        // Resolve module + method up front to drop the immutable borrow
                                        // on `ctx` before we mutate via register_native_instance + push.
                                        let factory_class: Option<&'static str> = ctx
                                            .lookup_native_module(func_name)
                                            .and_then(|(m, method)| match (m, method) {
                                                ("http", Some("createServer")) => {
                                                    Some("HttpServer")
                                                }
                                                ("https", Some("createServer")) => {
                                                    Some("HttpsServer")
                                                }
                                                ("http2", Some("createSecureServer")) => {
                                                    Some("Http2SecureServer")
                                                }
                                                ("async_hooks", Some("createHook")) => {
                                                    Some("AsyncHook")
                                                }
                                                _ => None,
                                            });
                                        if let Some(class_name) = factory_class {
                                            // Module name comes from `lookup_native_module` —
                                            // re-look it up after the borrow ends.
                                            let module = ctx
                                                .lookup_native_module(func_name)
                                                .map(|(m, _)| m.to_string())
                                                .unwrap_or_default();
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.clone(),
                                                class_name.to_string(),
                                            );
                                            ctx.module_native_instances.push((
                                                name.clone(),
                                                module,
                                                class_name.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }

                            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref()
                                            {
                                                let obj_name = obj_ident.sym.as_ref();
                                                if let Some((module_name, _)) =
                                                    ctx.lookup_native_module(obj_name)
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) =
                                                        &member.prop
                                                    {
                                                        let class_name = match (
                                                            module_name,
                                                            method_ident.sym.as_ref(),
                                                        ) {
                                                            ("mongodb", "connect") => {
                                                                Some("MongoClient")
                                                            }
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "createPool",
                                                            ) => Some("Pool"),
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "createConnection",
                                                            ) => Some("Connection"),
                                                            ("pg", "connect") => Some("Client"),
                                                            (
                                                                "http" | "https",
                                                                "request" | "get",
                                                            ) => Some("ClientRequest"),
                                                            (
                                                                "axios",
                                                                "get" | "post" | "put" | "delete"
                                                                | "patch" | "request",
                                                            ) => Some("Response"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = class_name {
                                                            ctx.register_native_instance(
                                                                name.clone(),
                                                                module_name.to_string(),
                                                                class_name.to_string(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is a `new NativeClass(...)` expression
                            // e.g., const db = new Database('mango.db') where Database is from better-sqlite3
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name_str = class_ident.sym.as_ref();
                                    // Check if this class comes from a native module import
                                    let native_info = ctx
                                        .lookup_native_module(class_name_str)
                                        .map(|(m, _)| m.to_string());
                                    if let Some(module_name) = native_info {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.clone(),
                                            class_name_str.to_string(),
                                        );
                                        ctx.module_native_instances.push((
                                            name.clone(),
                                            module_name,
                                            class_name_str.to_string(),
                                        ));
                                    }
                                } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                                    if let (
                                        ast::Expr::Ident(module_ident),
                                        ast::MemberProp::Ident(class_ident),
                                    ) = (member.obj.as_ref(), &member.prop)
                                    {
                                        let module_alias = module_ident.sym.as_ref();
                                        if let Some(module_name) = ctx
                                            .lookup_native_module(module_alias)
                                            .map(|(m, _)| m.to_string())
                                        {
                                            let class_name_str = class_ident.sym.as_ref();
                                            let is_known_native_class = matches!(
                                                (module_name.as_str(), class_name_str),
                                                (
                                                    "async_hooks",
                                                    "AsyncLocalStorage" | "AsyncResource"
                                                )
                                            );
                                            if is_known_native_class {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    module_name.clone(),
                                                    class_name_str.to_string(),
                                                );
                                                ctx.module_native_instances.push((
                                                    name.clone(),
                                                    module_name,
                                                    class_name_str.to_string(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is a method call on a registered native instance (chaining).
                            // e.g., const db = client.db(name) where client is a mongodb native instance.
                            {
                                // Unwrap await if present
                                let actual_init =
                                    if let ast::Expr::Await(await_expr) = init.as_ref() {
                                        await_expr.arg.as_ref()
                                    } else {
                                        init.as_ref()
                                    };
                                if let ast::Expr::Call(call_expr) = actual_init {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref()
                                            {
                                                let obj_name = obj_ident.sym.to_string();
                                                if let Some((module_name, _class)) = ctx
                                                    .lookup_native_instance(&obj_name)
                                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) =
                                                        &member.prop
                                                    {
                                                        let method_name = method_ident.sym.as_ref();
                                                        // Determine if the method returns a handle (another native instance)
                                                        let returns_handle = match (
                                                            module_name.as_str(),
                                                            method_name,
                                                        ) {
                                                            ("mongodb", "db") => Some("Database"),
                                                            ("mongodb", "collection") => {
                                                                Some("Collection")
                                                            }
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "getConnection",
                                                            ) => Some("PoolConnection"),
                                                            ("better-sqlite3", "prepare") => {
                                                                Some("Statement")
                                                            }
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = returns_handle {
                                                            ctx.register_native_instance(
                                                                name.clone(),
                                                                module_name,
                                                                class_name.to_string(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is an arrow function with a native return type
                            // e.g., export const getRedis = async (): Promise<Redis> => { ... }
                            if let ast::Expr::Arrow(arrow) = init.as_ref() {
                                if let Some(ref rt) = arrow.return_type {
                                    let return_type =
                                        extract_ts_type_with_ctx(&rt.type_ann, Some(ctx));
                                    // Unwrap Promise<T> for async functions
                                    let check_type = match &return_type {
                                        Type::Generic { base, type_args } if base == "Promise" => {
                                            type_args.first().unwrap_or(&return_type)
                                        }
                                        Type::Promise(inner) => inner.as_ref(),
                                        other => other,
                                    };
                                    if let Type::Named(type_name) = check_type {
                                        let module_info = match type_name.as_str() {
                                            "Redis" => Some(("ioredis", "Redis")),
                                            "EventEmitter" => Some(("events", "EventEmitter")),
                                            "Pool" => Some(("mysql2/promise", "Pool")),
                                            "PoolConnection" => {
                                                Some(("mysql2/promise", "PoolConnection"))
                                            }
                                            "WebSocket" | "WebSocketServer" => {
                                                Some(("ws", type_name.as_str()))
                                            }
                                            // perry-stdlib net.Socket: lets library wrappers like
                                            //   export function openSocket(host, port): Socket { ... }
                                            // propagate native-instance tagging to callers, so
                                            //   const sock = openSocket(...);
                                            //   sock.on(...);   // dispatches to js_net_socket_on
                                            // works without ceremony.
                                            "Socket" => Some(("net", "Socket")),
                                            _ => {
                                                // Also check dotted names (e.g., mysql.Pool)
                                                if let Some(dot_pos) = type_name.find('.') {
                                                    let module_alias = &type_name[..dot_pos];
                                                    let class_name = &type_name[dot_pos + 1..];
                                                    if let Some((module_name, _)) =
                                                        ctx.lookup_native_module(module_alias)
                                                    {
                                                        Some((module_name, class_name))
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            }
                                        };
                                        if let Some((module, class)) = module_info {
                                            ctx.func_return_native_instances.push((
                                                name.clone(),
                                                module.to_string(),
                                                class.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }

                            // Track exported values that need cross-module access.
                            // Any exported const/let with an initializer needs a global data slot
                            // so that importing modules can read its value at runtime.
                            // Previously this only matched Object/Call/Array/New/Arrow expressions,
                            // which caused exported string, number, bigint, and boolean constants
                            // to be undefined when imported by other modules.
                            let needs_export_global = true;

                            // Check if this is a Widget({...}) call from perry/widget
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }

                            let expr = lower_expr(ctx, init)?;
                            let id = if ctx.pre_registered_module_vars.remove(&name) {
                                let id = ctx.lookup_local(&name).unwrap();
                                if let Some((_, _, existing_ty)) =
                                    ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                                {
                                    *existing_ty = ty.clone();
                                }
                                id
                            } else {
                                ctx.define_local(name.clone(), ty.clone())
                            };
                            module.init.push(Stmt::Let {
                                id,
                                name: name.clone(),
                                ty,
                                mutable: matches!(
                                    var_decl.kind,
                                    ast::VarDeclKind::Let | ast::VarDeclKind::Var
                                ),
                                init: Some(expr),
                            });
                            module.exports.push(Export::Named {
                                local: name.clone(),
                                exported: name.clone(),
                            });

                            // Register exported values that need cross-module globals
                            if needs_export_global {
                                module.exported_objects.push(name.clone());
                            }

                            // Handle identifier aliases: export const foo = existingVar;
                            if let ast::Expr::Ident(ident) = init.as_ref() {
                                let ref_name = ident.sym.to_string();
                                if let Some(func_id) = ctx.lookup_func(&ref_name) {
                                    // Function alias - add to exported_functions
                                    module.exported_functions.push((name, func_id));
                                } else {
                                    // Non-function alias (e.g., export const alias = someObject)
                                    // Needs its own export global for cross-module access
                                    module.exported_objects.push(name.clone());
                                }
                            }
                        }
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let class = lower_class_decl(ctx, class_decl, true)?;
                    let class_name = class.name.clone();
                    // Issue #711: dynamic parent-class registration at the
                    // source position (see non-export class arm below for
                    // rationale).
                    if let Some(extends_expr) = &class.extends_expr {
                        module
                            .init
                            .push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                                class_name: class_name.clone(),
                                parent_expr: extends_expr.clone(),
                            }));
                    }
                    // Inject static-field-init statements in source order
                    // (see non-export class arm below for rationale).
                    for sf in &class.static_fields {
                        if let Some(init) = &sf.init {
                            // Computed-key static fields (`static [sym] = v`)
                            // emit a runtime-register call instead of a
                            // string-keyed StaticFieldSet. Refs #420.
                            if let Some(key) = sf.key_expr.as_ref() {
                                module.init.push(Stmt::Expr(Expr::ClassStaticSymbolSet {
                                    class_name: class_name.clone(),
                                    key: Box::new(key.clone()),
                                    value: Box::new(init.clone()),
                                }));
                            } else {
                                module.init.push(Stmt::Expr(Expr::StaticFieldSet {
                                    class_name: class_name.clone(),
                                    field_name: sf.name.clone(),
                                    value: Box::new(init.clone()),
                                }));
                            }
                        }
                    }
                    append_legacy_decorator_init_for_class(ctx, &mut module.init, &class);
                    push_class_dedup(module, class);
                    module.exports.push(Export::Named {
                        local: class_name.clone(),
                        exported: class_name,
                    });
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, true)?;
                    let enum_name = en.name.clone();
                    module.enums.push(en);
                    module.exported_objects.push(enum_name.clone());
                    module.exports.push(Export::Named {
                        local: enum_name.clone(),
                        exported: enum_name,
                    });
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, true)?;
                    let iface_name = iface.name.clone();
                    module.interfaces.push(iface);
                    module.exports.push(Export::Named {
                        local: iface_name.clone(),
                        exported: iface_name,
                    });
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, true)?;
                    let alias_name = alias.name.clone();
                    module.type_aliases.push(alias);
                    module.exports.push(Export::Named {
                        local: alias_name.clone(),
                        exported: alias_name,
                    });
                }
                ast::Decl::TsModule(ts_module) => {
                    // export namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let class =
                                lower_namespace_as_class(ctx, module, &ns_name, body, true)?;
                            let class_name = class.name.clone();
                            push_class_dedup(module, class);
                            module.exports.push(Export::Named {
                                local: class_name.clone(),
                                exported: class_name,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportNamed(export_named) => {
            // Skip type-only exports (export type { ... }) - they have no runtime value
            if export_named.type_only {
                return Ok(());
            }
            // export { foo, bar as baz }
            // export { foo } from "source"
            if let Some(ref src) = export_named.src {
                // Re-export from another module
                let source = src.value.as_str().unwrap_or("").to_string();
                for spec in &export_named.specifiers {
                    match spec {
                        ast::ExportSpecifier::Named(named) => {
                            // Skip individual type-only specifiers (export { type Foo, Bar })
                            if named.is_type_only {
                                continue;
                            }
                            let local = match &named.orig {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let exported = named
                                .exported
                                .as_ref()
                                .map(|e| match e {
                                    ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                    ast::ModuleExportName::Str(s) => {
                                        s.value.as_str().unwrap_or("").to_string()
                                    }
                                })
                                .unwrap_or_else(|| local.clone());
                            module.exports.push(Export::ReExport {
                                source: source.clone(),
                                imported: local,
                                exported,
                            });
                        }
                        ast::ExportSpecifier::Namespace(ns) => {
                            // `export * as Foo from "./Foo"` — closes #310. Pre-fix
                            // SWC's `ExportSpecifier::Namespace` was silently dropped
                            // here because the arm only matched `Named`. The
                            // re-exported file then never entered the module graph,
                            // and every `<name>.<member>` access in consumer code
                            // lowered to 0 (the unknown-identifier sentinel).
                            let name = match &ns.name {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            module.exports.push(Export::NamespaceReExport {
                                source: source.clone(),
                                name,
                            });
                        }
                        // `export v from 'mod'` — TC39 stage-1, never standardised.
                        // Not emitted by tsc/swc TS output, so silently ignore.
                        ast::ExportSpecifier::Default(_) => {}
                    }
                }
            } else {
                // Local export: export { foo, bar as baz }
                for spec in &export_named.specifiers {
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only {
                            continue;
                        }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => {
                                s.value.as_str().unwrap_or("").to_string()
                            }
                        };
                        let exported = named
                            .exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::Named {
                            local: local.clone(),
                            exported: exported.clone(),
                        });

                        // Fix #482 (v0.5.577): when `class Foo {}` is followed by
                        // `export { Foo }` in a separate clause, the class was
                        // lowered with `is_exported = false` (because the decl
                        // wasn't a syntactic `export class Foo {}`). The
                        // CLI-driver's `exported_classes` lookup
                        // (`crates/perry/src/commands/compile.rs:1684`) filters
                        // on `class.is_exported`, so the class never reached
                        // the importer's `imported_classes` registry — `new
                        // Foo()` from another module fell through to the
                        // empty-object placeholder. Flip the class's
                        // is_exported bit when an export clause names it.
                        for class in module.classes.iter_mut() {
                            if class.name == local {
                                class.is_exported = true;
                                break;
                            }
                        }

                        // Fix #588: parallel of the class arm above for function
                        // decls. `function drizzle() {}` followed by `export {
                        // drizzle }` was lowered with `is_exported = false` and
                        // never landed in `module.exported_functions`. The CLI
                        // driver's `exported_func_param_counts` lookup at
                        // compile.rs:1907 filters on `func.is_exported`, and
                        // its fallback at compile.rs:1921 iterates only
                        // `exported_functions` — both miss this function.
                        // Importers then fall back to `args.len()` in
                        // `lower_call.rs:767` and declare the cross-module
                        // function with the wrong arity. The ABI mismatch
                        // causes trailing default-defaulted params to read
                        // stale register state (typically a small
                        // f64::from_bits(N) value, e.g. 1.26e-321), so the
                        // body's `if (p === undefined) p = default` desugar
                        // never fires.
                        if let Some(func_id) = ctx.lookup_func(&local) {
                            for func in module.functions.iter_mut() {
                                if func.id == func_id {
                                    func.is_exported = true;
                                    break;
                                }
                            }
                            if !module
                                .exported_functions
                                .iter()
                                .any(|(n, _)| n == &exported)
                            {
                                module.exported_functions.push((exported.clone(), func_id));
                            }
                        }

                        // Check if the variable is a closure or other exportable object
                        // by looking through init statements. For #460: also catch let
                        // bindings whose init is a function-reference value
                        // (`const _await = core.deferredAwait`) — without this branch
                        // the renamed export `_await as await` produces no backing
                        // global / getter at all and `_perry_fn_<mod>__await` link-fails.
                        for stmt in &module.init {
                            if let Stmt::Let {
                                name,
                                init: Some(init_expr),
                                ..
                            } = stmt
                            {
                                if name == &local {
                                    let is_exportable = matches!(
                                        init_expr,
                                        Expr::Closure { .. }
                                            | Expr::Object(_)
                                            | Expr::Array(_)
                                            | Expr::Call { .. }
                                            | Expr::New { .. }
                                            | Expr::JsNew { .. }
                                            | Expr::LocalGet(_)
                                            | Expr::FuncRef(_)
                                            | Expr::ExternFuncRef { .. }
                                            | Expr::PropertyGet { .. }
                                            // #421 fix (v0.5.574): primitive literals must
                                            // also flow through `exported_objects` so the
                                            // importing module's `imported_vars` set picks
                                            // them up — without this, `var X = "literal";
                                            // export { X };` (the shape hono / drizzle /
                                            // any prebundled JS uses for string / number
                                            // constants) gets imported as a closure-pointer
                                            // wrapper instead of the actual value, and
                                            // `typeof X` returns "function" + `X.toString`
                                            // prints `[object Object]`.
                                            | Expr::String(_)
                                            | Expr::Number(_)
                                            | Expr::Bool(_)
                                            | Expr::BigInt(_)
                                            | Expr::Null
                                            | Expr::Undefined
                                            // Refs #420 (drizzle): `const entityKind = Symbol.for(...)`
                                            // followed by `export { entityKind }` must register the
                                            // local as an exported variable so importing modules
                                            // pick it up via `imported_vars` (and route through the
                                            // getter, not as a closure pointer).
                                            | Expr::SymbolFor(_)
                                            // Issue #923: `const pool = mysql.createPool(...)`
                                            // followed by `export { pool }` lowers the init to
                                            // `Expr::NativeMethodCall` (not `Expr::Call`) because
                                            // `mysql` is a registered native-module alias and the
                                            // factory call resolves to the stdlib FFI dispatch.
                                            // Without this branch, `pool` never lands in
                                            // `exported_objects`, the producer-side getter
                                            // `perry_fn_<src>__pool` is never emitted, and the
                                            // consumer-side `ExternFuncRef { name: "pool" }` falls
                                            // through to the closure-wrapper path
                                            // (`__perry_wrap_perry_fn_<src>__pool`) which #836's
                                            // Sub-bug B emits as a no-op returning undefined. End
                                            // result: link succeeds but `typeof pool === "function"`
                                            // and `pool.execute(...)` segfaults. Mirroring the
                                            // inline-export shape (`export const pool = ...` at
                                            // line ~5087) makes both export forms equivalent.
                                            //
                                            // Covers every stdlib factory pattern: `mysql2/promise`
                                            // `createPool` / `createConnection`, `net.createConnection`,
                                            // `http.createServer`, `pg.connect`, `ioredis`
                                            // constructors, `tls.connect`, and any other
                                            // factory the codegen already lowers to a
                                            // `NativeMethodCall` via lookup_native_module.
                                            | Expr::NativeMethodCall { .. }
                                    );
                                    if is_exportable {
                                        module.exported_objects.push(exported.clone());
                                        // Ensure the LOCAL name also surfaces as
                                        // exported — that's what gates the global
                                        // emission in codegen (`exported_var_names`
                                        // is built from `exported_objects`).
                                        // Without the local entry, `_await`'s id
                                        // is never registered in `module_globals`,
                                        // so even the local-name getter is missing
                                        // and call sites that resolve through the
                                        // local name fall through to undefined.
                                        if !module.exported_objects.contains(&local) {
                                            module.exported_objects.push(local.clone());
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        ast::ModuleDecl::ExportDefaultDecl(export_default) => {
            // export default function foo() {} or export default class Foo {}
            match &export_default.decl {
                ast::DefaultDecl::Fn(fn_expr) => {
                    if let Some(ref ident) = fn_expr.ident {
                        // Named function: `export default function foo() {}`.
                        //
                        // Pre-fix this branch only pushed an `Export::Named`
                        // entry and dropped the body — so the consumer's
                        // `import foo from "./mod"; foo()` resolved to
                        // `ExternFuncRef { name: "default" }` and link/runtime
                        // produced `undefined` because no
                        // `perry_fn_<src>__default` (and no
                        // `perry_fn_<src>__foo`) symbol was ever emitted.
                        //
                        // This was the root cause of the uuid `v4()` smoke-
                        // test failure: `rng.js` is exactly
                        // `export default function rng() { ... }`, so the
                        // imported `rng()` call in `v4.js` returned undefined
                        // and the downstream `.length` access threw.
                        //
                        // Mirror the `ExportDecl::Fn` flow so the function
                        // body is actually emitted under
                        // `perry_fn_<src>__<ident>`, then register the
                        // function under the exported name `"default"`.
                        // codegen's alias-emission pass (codegen.rs ~L2259)
                        // sees `exported_name != f.name` and synthesizes the
                        // `perry_fn_<src>__default` forwarder pointing at
                        // `perry_fn_<src>__<ident>`.
                        let func_name = ident.sym.to_string();
                        if fn_expr.function.body.is_some() {
                            let synth_fn_decl = ast::FnDecl {
                                ident: ident.clone(),
                                declare: false,
                                function: fn_expr.function.clone(),
                            };
                            let mut func = lower_fn_decl(ctx, &synth_fn_decl)?;
                            func.is_exported = true;
                            let func_id = func.id;
                            if !matches!(func.return_type, Type::Any) {
                                ctx.register_func_return_type(
                                    func_name.clone(),
                                    func.return_type.clone(),
                                );
                            }
                            if let Some((mod_name, class)) =
                                native_instance_from_return_type(&func.return_type)
                            {
                                ctx.func_return_native_instances.push((
                                    func_name.clone(),
                                    mod_name.to_string(),
                                    class.to_string(),
                                ));
                            }
                            let defaults: Vec<Option<Expr>> =
                                func.params.iter().map(|p| p.default.clone()).collect();
                            let param_ids: Vec<LocalId> =
                                func.params.iter().map(|p| p.id).collect();
                            let rest_idx = func.params.iter().position(|p| p.is_rest);
                            ctx.func_defaults
                                .push((func.id, defaults, param_ids, rest_idx));
                            module.functions.push(func);
                            // Register under both names: callable locally as
                            // `<ident>` (some modules also `export { foo }`
                            // or call themselves by name) AND as the
                            // module's `default` export so importers resolve
                            // through the wrapper.
                            module.exports.push(Export::Named {
                                local: func_name.clone(),
                                exported: "default".to_string(),
                            });
                            module
                                .exported_functions
                                .push(("default".to_string(), func_id));
                        } else {
                            // Body-less form (declaration-only) — keep
                            // historical behavior of only recording the
                            // export entry.
                            module.exports.push(Export::Named {
                                local: func_name,
                                exported: "default".to_string(),
                            });
                        }
                    } else if fn_expr.function.body.is_some() {
                        // Anonymous-default-function (zod / vitest blocker):
                        // `export default function () { ... }` arrives as a
                        // `DefaultDecl::Fn(FnExpr)` with `fn_expr.ident == None`.
                        // Pre-fix this branch dropped the body entirely —
                        // codegen never saw the function, so
                        // `perry_fn_<src>__default` was never emitted and
                        // consumers link-failed with `Undefined symbols:
                        // _perry_fn_<src>__default`. The
                        // `__perry_wrap_perry_fn_<src>__default` rename
                        // wrapper added in #837 had nothing to point at
                        // either.
                        //
                        // Synthesize an `FnDecl` with ident `default` so
                        // the HIR function name (and therefore the LLVM
                        // symbol) is `perry_fn_<src>__default`, matching
                        // what consumers ask for. Run the resulting decl
                        // through the same flow as `ExportDecl::Fn`: lower
                        // the body, mark `is_exported`, register defaults
                        // and `exported_functions` so codegen's wrapper-
                        // emission machinery picks it up.
                        //
                        // Scope-narrow: this only changes the
                        // `fn_expr.ident == None` branch. The named-default
                        // case above keeps its existing behavior pending a
                        // separate fix.
                        let synth_ident = ast::Ident::new(
                            "default".to_string().into(),
                            swc_common::DUMMY_SP,
                            Default::default(),
                        );
                        let synth_fn_decl = ast::FnDecl {
                            ident: synth_ident,
                            declare: false,
                            function: fn_expr.function.clone(),
                        };
                        let mut func = lower_fn_decl(ctx, &synth_fn_decl)?;
                        func.is_exported = true;
                        let func_id = func.id;
                        // Defaults registration mirrors the `ExportDecl::Fn`
                        // path so call sites that pad missing args still
                        // resolve user-written defaults.
                        let defaults: Vec<Option<Expr>> =
                            func.params.iter().map(|p| p.default.clone()).collect();
                        let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                        let rest_idx = func.params.iter().position(|p| p.is_rest);
                        ctx.func_defaults
                            .push((func.id, defaults, param_ids, rest_idx));
                        module.functions.push(func);
                        // Both the named export entry (so the importer's
                        // namespace populator sees `default`) and the
                        // `exported_functions` registry (so codegen's
                        // alias / wrapper emission treats it as a real
                        // exported function) are required.
                        module.exports.push(Export::Named {
                            local: "default".to_string(),
                            exported: "default".to_string(),
                        });
                        module
                            .exported_functions
                            .push(("default".to_string(), func_id));
                    }
                }
                ast::DefaultDecl::Class(class_expr) => {
                    if let Some(ref ident) = class_expr.ident {
                        let class_name = ident.sym.to_string();
                        module.exports.push(Export::Named {
                            local: class_name,
                            exported: "default".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportAll(export_all) => {
            // export * from "source"
            let source = export_all.src.value.as_str().unwrap_or("").to_string();
            module.exports.push(Export::ExportAll { source });
        }
        ast::ModuleDecl::ExportDefaultExpr(export_default_expr) => {
            // export default <expr>
            let lowered = lower_expr(ctx, &export_default_expr.expr)?;

            // If the expression is a FuncRef, add to exported_functions for proper wrapper generation
            if let Expr::FuncRef(func_id) = &lowered {
                // Find the function and add as exported with name "default"
                let func_id = *func_id;
                module
                    .exported_functions
                    .push(("default".to_string(), func_id));
                // Also mark the function as exported
                for func in &mut module.functions {
                    if func.id == func_id {
                        func.is_exported = true;
                        break;
                    }
                }
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            } else if let Expr::ClassRef(class_name) = &lowered {
                // Issue #665: `export default <ClassName>` where the
                // identifier names a class declared in this module — the
                // shape cjs_wrap emits for `module.exports = ClassName`
                // (e.g. rate-limiter-flexible's `RateLimiterMemory`).
                // Mirror both the `ExportDefaultDecl::Class` path AND the
                // `export { X }` Fix-#482 path: register the class
                // binding as the default export AND flip the class's
                // `is_exported` bit so the CLI driver's
                // `exported_classes` lookup (which filters on
                // `class.is_exported`) carries the class into the
                // importer's `imported_classes` registry. Without this,
                // the importer hits the fallback branch below: a
                // synthetic `default` Any-typed local whose value
                // happens to be the class but whose class identity HIR
                // lost — `new (default-import)(...)` then falls through
                // to the empty-object placeholder with every method
                // undefined.
                module.exports.push(Export::Named {
                    local: class_name.clone(),
                    exported: "default".to_string(),
                });
                for class in module.classes.iter_mut() {
                    if &class.name == class_name {
                        class.is_exported = true;
                        break;
                    }
                }
            } else {
                // For other expressions (closures, calls, etc.), create a synthetic "default" variable
                let id = ctx.define_local("default".to_string(), Type::Any);
                module.init.push(Stmt::Let {
                    id,
                    name: "default".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(lowered),
                });
                module.exported_objects.push("default".to_string());
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            }
        }
        _ => {
            // TsImportEquals, TsExportAssignment, TsNamespaceExport - TypeScript specific
        }
    }
    Ok(())
}

/// Lower a TypeScript namespace declaration into a synthetic class with static methods.
/// `export namespace Slug { export function create() { ... } }` becomes a class `Slug`
/// with a static method `create`. Exported namespace variables are lowered as module-level
/// locals (not static fields) and accessed via compile-time namespace resolution.
/// Private namespace members (non-exported) are lowered as module-level variables.
fn lower_namespace_as_class(
    ctx: &mut LoweringContext,
    module: &mut Module,
    ns_name: &str,
    body: &ast::TsNamespaceBody,
    is_exported: bool,
) -> Result<Class> {
    let class_id = match ctx.lookup_class(ns_name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(ns_name.to_string(), id);
            id
        }
    };

    let items = match body {
        ast::TsNamespaceBody::TsModuleBlock(block) => &block.body,
        ast::TsNamespaceBody::TsNamespaceDecl(_) => {
            // Nested namespace (namespace A.B { }) — not supported yet
            return Ok(Class {
                id: class_id,
                name: ns_name.to_string(),
                type_params: Vec::new(),
                extends: None,
                extends_name: None,
                native_extends: None,
                extends_expr: None,
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                getters: Vec::new(),
                setters: Vec::new(),
                static_fields: Vec::new(),
                static_methods: Vec::new(),
                decorators: Vec::new(),
                is_exported,
                aliases: Vec::new(),
            });
        }
    };

    let mut static_methods = Vec::new();
    let mut static_method_names = Vec::new();

    // First pass: collect exported function names, pre-register all functions and variables
    // (so namespace members can reference each other regardless of declaration order)
    for item in items {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_some() {
                            let name = fn_decl.ident.sym.to_string();
                            static_method_names.push(name.clone());
                            // Pre-register exported functions so other namespace members can call them
                            if ctx.lookup_func(&name).is_none() {
                                let id = ctx.fresh_func();
                                ctx.register_func(name, id);
                            }
                        }
                    }
                    ast::Decl::Var(var_decl) => {
                        // Pre-register exported namespace variables as module-level locals
                        for decl in &var_decl.decls {
                            if let Ok(name) = get_binding_name(&decl.name) {
                                if ctx.lookup_local(&name).is_none() {
                                    let ty = extract_binding_type(&decl.name);
                                    ctx.define_local(name.clone(), ty);
                                    ctx.pre_registered_module_vars.insert(name);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Pre-register non-exported functions (hoisted like JS)
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                if fn_decl.function.body.is_some() {
                    let name = fn_decl.ident.sym.to_string();
                    if ctx.lookup_func(&name).is_none() {
                        let id = ctx.fresh_func();
                        ctx.register_func(name, id);
                    }
                }
            }
            // Pre-register non-exported variables
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var_decl))) => {
                for decl in &var_decl.decls {
                    if let ast::Pat::Ident(ident) = &decl.name {
                        let name = ident.id.sym.to_string();
                        if ctx.lookup_local(&name).is_none() {
                            let ty = ident
                                .type_ann
                                .as_ref()
                                .map(|ann| extract_ts_type(&ann.type_ann))
                                .unwrap_or(Type::Any);
                            ctx.define_local(name.clone(), ty);
                            ctx.pre_registered_module_vars.insert(name);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Register class and statics early so method bodies can reference them
    ctx.register_class_statics(ns_name.to_string(), Vec::new(), static_method_names.clone());

    // Set current namespace so internal function calls resolve as StaticMethodCall
    let prev_namespace = ctx.current_namespace.take();
    ctx.current_namespace = Some(ns_name.to_string());

    // Second pass: lower all items
    for item in items {
        match item {
            // Non-exported items → module-level variables/functions
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(ctx, module, stmt)?;
            }
            // Exported items
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_none() {
                            continue; // Skip declare functions
                        }
                        let func = lower_fn_decl(ctx, fn_decl)?;
                        // Register return type for call-site inference
                        if !matches!(func.return_type, Type::Any) {
                            ctx.register_func_return_type(
                                func.name.clone(),
                                func.return_type.clone(),
                            );
                        }
                        if let Some((module, class)) =
                            native_instance_from_return_type(&func.return_type)
                        {
                            ctx.func_return_native_instances.push((
                                func.name.clone(),
                                module.to_string(),
                                class.to_string(),
                            ));
                        }
                        static_methods.push(func);
                    }
                    ast::Decl::Var(var_decl) => {
                        // Lower exported namespace variables as module-level locals
                        let mutable = var_decl.kind != ast::VarDeclKind::Const;
                        for decl in &var_decl.decls {
                            let name = get_binding_name(&decl.name)?;
                            let ty = extract_binding_type(&decl.name);
                            if let Some(init) = &decl.init {
                                let expr = lower_expr(ctx, init)?;
                                let id = if ctx.pre_registered_module_vars.remove(&name) {
                                    let id = ctx.lookup_local(&name).unwrap();
                                    if let Some((_, _, existing_ty)) =
                                        ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                                    {
                                        *existing_ty = ty.clone();
                                    }
                                    id
                                } else {
                                    ctx.define_local(name.clone(), ty.clone())
                                };
                                module.init.push(Stmt::Let {
                                    id,
                                    name: name.clone(),
                                    ty,
                                    mutable,
                                    init: Some(expr),
                                });
                                // Track as namespace variable for Ns.member access resolution
                                ctx.namespace_vars
                                    .push((ns_name.to_string(), name.clone(), id));
                                // Export the variable for cross-module access
                                if is_exported {
                                    module.exported_objects.push(name.clone());
                                    module.exports.push(Export::Named {
                                        local: name.clone(),
                                        exported: name.clone(),
                                    });
                                }
                            }
                        }
                    }
                    ast::Decl::Class(class_decl) => {
                        let class = lower_class_decl(ctx, class_decl, is_exported)?;
                        push_class_dedup(module, class);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Restore previous namespace context
    ctx.current_namespace = prev_namespace;

    Ok(Class {
        id: class_id,
        name: ns_name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods,
        decorators: Vec::new(),
        is_exported,
        aliases: Vec::new(),
    })
}

/// Recursively walk a destructuring pattern collecting every leaf identifier
/// (and pre-defining each as a local). Used by the for-of binding pre-pass so
/// the loop body can reference variables introduced by *nested* patterns like
/// `for (const { foo, bar: [a, b] } of arr)` — the outer per-prop loop only
/// handled `Ident` leaves, so leaves buried in nested array/object patterns
/// were silently skipped and read as zero in the body. Issue #554.
pub(crate) fn collect_for_of_pattern_leaves(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    out: &mut Vec<(String, LocalId)>,
) {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let id = ctx.define_local(name.clone(), Type::Any);
            out.push((name, id));
        }
        ast::Pat::Array(arr_pat) => {
            for elem in &arr_pat.elems {
                if let Some(ep) = elem {
                    if let ast::Pat::Rest(rest) = ep {
                        collect_for_of_pattern_leaves(ctx, &rest.arg, out);
                    } else {
                        collect_for_of_pattern_leaves(ctx, ep, out);
                    }
                }
            }
        }
        ast::Pat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let id = ctx.define_local(name.clone(), Type::Any);
                        out.push((name, id));
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_for_of_pattern_leaves(ctx, &kv.value, out);
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_for_of_pattern_leaves(ctx, &rest.arg, out);
                    }
                }
            }
        }
        ast::Pat::Assign(assign_pat) => {
            collect_for_of_pattern_leaves(ctx, &assign_pat.left, out);
        }
        ast::Pat::Rest(rest) => {
            collect_for_of_pattern_leaves(ctx, &rest.arg, out);
        }
        _ => {}
    }
}

/// Emit `Stmt::Let` bindings for a destructuring pattern given a `source`
/// expression that produces the value being destructured. Reuses the
/// pre-allocated leaf ids in `var_ids` (in source order) so the loop body — which
/// was already lowered against those ids — sees the correct bindings. Mirrors
/// `destructuring::lower_pattern_binding_into` but takes pre-allocated ids
/// instead of calling `define_local` itself. Issue #554.
pub(crate) fn emit_for_of_pattern_binding(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    var_ids: &[(String, LocalId)],
    var_idx: &mut usize,
    out: &mut Vec<Stmt>,
) -> Result<()> {
    match pat {
        ast::Pat::Ident(_) => {
            let (name, id) = var_ids[*var_idx].clone();
            *var_idx += 1;
            out.push(Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            Ok(())
        }
        ast::Pat::Array(arr_pat) => {
            // Default to Array(Any) so OOB reads return undefined/NaN as the
            // existing `destructuring::lower_pattern_binding_into` helper does.
            // (Typing the temp as Any pulls reads through the typed-element
            // fast path which returns 0 for OOB and breaks default-value
            // handling.)
            let arr_ty = arr_pat
                .type_ann
                .as_ref()
                .map(|ann| crate::lower_types::extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Array(Box::new(Type::Any)));
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, arr_ty.clone()));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: arr_ty,
                mutable: false,
                init: Some(source),
            });
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                let Some(elem_pat) = elem else { continue };
                if let ast::Pat::Rest(rest) = elem_pat {
                    let slice = Expr::ArraySlice {
                        array: Box::new(Expr::LocalGet(tmp_id)),
                        start: Box::new(Expr::Number(idx as f64)),
                        end: None,
                    };
                    emit_for_of_pattern_binding(ctx, &rest.arg, slice, var_ids, var_idx, out)?;
                    break;
                }
                let elem_src = Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(tmp_id)),
                    index: Box::new(Expr::Number(idx as f64)),
                };
                emit_for_of_pattern_binding(ctx, elem_pat, elem_src, var_ids, var_idx, out)?;
            }
            Ok(())
        }
        ast::Pat::Object(obj_pat) => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let key = assign.key.sym.to_string();
                        let prop_access = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };
                        let init = if let Some(default_expr) = &assign.value {
                            let default_val = lower_expr(ctx, default_expr)?;
                            Expr::Conditional {
                                condition: Box::new(Expr::Compare {
                                    op: CompareOp::Ne,
                                    left: Box::new(prop_access.clone()),
                                    right: Box::new(Expr::Undefined),
                                }),
                                then_expr: Box::new(prop_access),
                                else_expr: Box::new(default_val),
                            }
                        } else {
                            prop_access
                        };
                        let (name, id) = var_ids[*var_idx].clone();
                        *var_idx += 1;
                        out.push(Stmt::Let {
                            id,
                            name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(init),
                        });
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            _ => continue,
                        };
                        let elem_src = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };
                        emit_for_of_pattern_binding(
                            ctx, &kv.value, elem_src, var_ids, var_idx, out,
                        )?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        ast::Pat::Assign(assign_pat) => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            let default_val = lower_expr(ctx, &assign_pat.right)?;
            let with_default = Expr::Conditional {
                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(Expr::LocalGet(tmp_id)))),
                then_expr: Box::new(default_val),
                else_expr: Box::new(Expr::LocalGet(tmp_id)),
            };
            emit_for_of_pattern_binding(ctx, &assign_pat.left, with_default, var_ids, var_idx, out)
        }
        _ => Ok(()),
    }
}

fn lower_stmt(ctx: &mut LoweringContext, module: &mut Module, stmt: &ast::Stmt) -> Result<()> {
    match stmt {
        ast::Stmt::Decl(decl) => {
            match decl {
                ast::Decl::Fn(fn_decl) => {
                    // Skip declare functions (no body) - they are external FFI declarations
                    if fn_decl.function.body.is_none() {
                        return Ok(());
                    }
                    let func = lower_fn_decl(ctx, fn_decl)?;
                    // Register return type for call-site inference
                    if let Some((module, class)) =
                        native_instance_from_return_type(&func.return_type)
                    {
                        ctx.func_return_native_instances.push((
                            func.name.clone(),
                            module.to_string(),
                            class.to_string(),
                        ));
                    }
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> =
                        func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    let rest_idx = func.params.iter().position(|p| p.is_rest);
                    ctx.func_defaults
                        .push((func.id, defaults, param_ids, rest_idx));
                    module.functions.push(func);
                }
                ast::Decl::Var(var_decl) => {
                    let mutable = var_decl.kind != ast::VarDeclKind::Const;
                    let is_var = var_decl.kind == ast::VarDeclKind::Var;
                    for decl in &var_decl.decls {
                        // Check if this is a Widget({...}) call from perry/widget
                        if let Some(init) = &decl.init {
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }
                        }
                        // For array destructuring from generator calls, wrap init in
                        // IteratorToArray so the destructuring gets a real array.
                        // This converts: const [a, b, ...rest] = gen()
                        // to: const [a, b, ...rest] = IteratorToArray(gen())
                        // by inserting a temp variable.
                        if matches!(&decl.name, ast::Pat::Array(_)) {
                            if let Some(init) = &decl.init {
                                if let ast::Expr::Call(call) = init.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call.callee {
                                        if let ast::Expr::Ident(ident) = callee.as_ref() {
                                            if ctx.generator_func_names.contains(ident.sym.as_ref())
                                            {
                                                // Lower the generator call, wrap in IteratorToArray, assign to temp
                                                let gen_expr = lower_expr(ctx, init)?;
                                                let arr_expr =
                                                    Expr::IteratorToArray(Box::new(gen_expr));
                                                let temp_id = ctx.fresh_local();
                                                ctx.locals.push((
                                                    format!("__gen_arr_{}", temp_id),
                                                    temp_id,
                                                    Type::Array(Box::new(Type::Any)),
                                                ));
                                                module.init.push(Stmt::Let {
                                                    id: temp_id,
                                                    name: format!("__gen_arr_{}", temp_id),
                                                    ty: Type::Array(Box::new(Type::Any)),
                                                    mutable: false,
                                                    init: Some(arr_expr),
                                                });
                                                // Now destructure from the temp array
                                                // Create a synthetic VarDeclarator with init = LocalGet(temp_id)
                                                // For simplicity, manually extract each element
                                                if let ast::Pat::Array(arr_pat) = &decl.name {
                                                    let mut idx = 0;
                                                    for elem in &arr_pat.elems {
                                                        if let Some(elem_pat) = elem {
                                                            match elem_pat {
                                                                ast::Pat::Ident(ident) => {
                                                                    let name =
                                                                        ident.id.sym.to_string();
                                                                    let id = ctx.define_local(
                                                                        name.clone(),
                                                                        Type::Any,
                                                                    );
                                                                    module.init.push(Stmt::Let {
                                                                        id,
                                                                        name,
                                                                        ty: Type::Any,
                                                                        mutable,
                                                                        init: Some(
                                                                            Expr::IndexGet {
                                                                                object: Box::new(
                                                                                    Expr::LocalGet(
                                                                                        temp_id,
                                                                                    ),
                                                                                ),
                                                                                index: Box::new(
                                                                                    Expr::Number(
                                                                                        idx as f64,
                                                                                    ),
                                                                                ),
                                                                            },
                                                                        ),
                                                                    });
                                                                    idx += 1;
                                                                }
                                                                ast::Pat::Rest(rest) => {
                                                                    if let ast::Pat::Ident(
                                                                        rest_ident,
                                                                    ) = &*rest.arg
                                                                    {
                                                                        let name = rest_ident
                                                                            .id
                                                                            .sym
                                                                            .to_string();
                                                                        let id = ctx.define_local(
                                                                            name.clone(),
                                                                            Type::Array(Box::new(
                                                                                Type::Any,
                                                                            )),
                                                                        );
                                                                        module.init.push(Stmt::Let {
                                                                            id,
                                                                            name,
                                                                            ty: Type::Array(Box::new(Type::Any)),
                                                                            mutable,
                                                                            init: Some(Expr::ArraySlice {
                                                                                array: Box::new(Expr::LocalGet(temp_id)),
                                                                                start: Box::new(Expr::Number(idx as f64)),
                                                                                end: None,
                                                                            }),
                                                                        });
                                                                    }
                                                                }
                                                                _ => {
                                                                    idx += 1;
                                                                }
                                                            }
                                                        } else {
                                                            idx += 1; // skip holes
                                                        }
                                                    }
                                                }
                                                continue; // skip the regular destructuring path
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Track locals assigned from `regex.exec(...)` so .index/.groups
                        // accesses route to the bare RegExpExecIndex/Groups variants.
                        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) {
                            if is_regex_exec_init(ctx, init) {
                                ctx.regex_exec_locals.insert(ident.id.sym.to_string());
                            }
                        }
                        // `const { proxy: revProxy, revoke } = Proxy.revocable(t, h)`
                        // is rewritten to a ProxyNew binding + a dummy revoke binding.
                        if let (ast::Pat::Object(obj_pat), Some(init)) = (&decl.name, &decl.init) {
                            let inner = {
                                let mut e = init.as_ref();
                                loop {
                                    match e {
                                        ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                                        ast::Expr::TsNonNull(nn) => e = &nn.expr,
                                        ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                                        ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                                        ast::Expr::Paren(p) => e = &p.expr,
                                        _ => break,
                                    }
                                }
                                e
                            };
                            let mut is_proxy_revocable = false;
                            if let ast::Expr::Call(call) = inner {
                                if let ast::Callee::Expr(callee) = &call.callee {
                                    if let ast::Expr::Member(m) = callee.as_ref() {
                                        if let ast::Expr::Ident(o) = m.obj.as_ref() {
                                            if o.sym.as_ref() == "Proxy" {
                                                if let ast::MemberProp::Ident(p) = &m.prop {
                                                    if p.sym.as_ref() == "revocable" {
                                                        is_proxy_revocable = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if is_proxy_revocable {
                                if let ast::Expr::Call(call) = inner {
                                    let target_ast = call.args.first().map(|a| a.expr.clone());
                                    let handler_ast = call.args.get(1).map(|a| a.expr.clone());
                                    let target = if let Some(t) = target_ast {
                                        lower_expr(ctx, &t)?
                                    } else {
                                        Expr::Undefined
                                    };
                                    let handler = if let Some(h) = handler_ast {
                                        lower_expr(ctx, &h)?
                                    } else {
                                        Expr::Object(vec![])
                                    };
                                    let mut proxy_alias: Option<String> = None;
                                    let mut revoke_alias: Option<String> = None;
                                    for prop in &obj_pat.props {
                                        match prop {
                                            ast::ObjectPatProp::KeyValue(kv) => {
                                                let key_name = match &kv.key {
                                                    ast::PropName::Ident(i) => i.sym.to_string(),
                                                    ast::PropName::Str(s) => {
                                                        s.value.as_str().unwrap_or("").to_string()
                                                    }
                                                    _ => continue,
                                                };
                                                if let ast::Pat::Ident(alias) = &*kv.value {
                                                    let alias_name = alias.id.sym.to_string();
                                                    if key_name == "proxy" {
                                                        proxy_alias = Some(alias_name);
                                                    } else if key_name == "revoke" {
                                                        revoke_alias = Some(alias_name);
                                                    }
                                                }
                                            }
                                            ast::ObjectPatProp::Assign(a) => {
                                                let name = a.key.sym.to_string();
                                                if name == "proxy" {
                                                    proxy_alias = Some(name);
                                                } else if name == "revoke" {
                                                    revoke_alias = Some(name);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some(p_name) = proxy_alias {
                                        let proxy_id = ctx.define_local(p_name.clone(), Type::Any);
                                        module.init.push(Stmt::Let {
                                            id: proxy_id,
                                            name: p_name.clone(),
                                            ty: Type::Any,
                                            mutable,
                                            init: Some(Expr::ProxyNew {
                                                target: Box::new(target),
                                                handler: Box::new(handler),
                                            }),
                                        });
                                        ctx.proxy_locals.insert(p_name.clone());
                                        if let Some(r_name) = revoke_alias {
                                            ctx.proxy_revoke_locals.insert(r_name.clone(), p_name);
                                            let rev_id =
                                                ctx.define_local(r_name.clone(), Type::Any);
                                            module.init.push(Stmt::Let {
                                                id: rev_id,
                                                name: r_name,
                                                ty: Type::Any,
                                                mutable,
                                                init: Some(Expr::Undefined),
                                            });
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                        // `const X = class { ... }` — lower the class expression
                        // inline using the binding name as the class name (so
                        // `new X(...)` later resolves without a dynamic dispatch
                        // shim). The let binding still stores a sentinel value
                        // (the new'd object) but the class is fully lowered.
                        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) {
                            let inner_expr = {
                                let mut e = init.as_ref();
                                loop {
                                    match e {
                                        ast::Expr::Paren(p) => e = &p.expr,
                                        ast::Expr::TsAs(a) => e = &a.expr,
                                        ast::Expr::TsNonNull(n) => e = &n.expr,
                                        ast::Expr::TsTypeAssertion(a) => e = &a.expr,
                                        _ => break,
                                    }
                                }
                                e
                            };
                            if let ast::Expr::Class(class_expr) = inner_expr {
                                let bind_name = ident.id.sym.to_string();
                                // Only handle if there's no explicit type annotation
                                // that would conflict, and the binding name isn't
                                // already a class (no shadow).
                                if ctx.lookup_class(&bind_name).is_none() {
                                    // Refs #486: `var X = class _X { ... new _X() ... }` —
                                    // the inner self-binding name `_X` references the same
                                    // class as the outer binding `X`. Pre-register the inner
                                    // name as a class alias BEFORE lowering the class body
                                    // so any `Ident("_X")` inside method bodies (e.g.
                                    // `new _X()`) lowers to `Expr::ClassRef("_X")` instead
                                    // of falling through to ExternFuncRef. The HIR `new`
                                    // ident path keys off `lookup_class`. Hono's
                                    // `var Node = class _Node { ... }` and similar npm dist
                                    // shapes hit this.
                                    let inner_name_for_register = class_expr
                                        .ident
                                        .as_ref()
                                        .map(|i| i.sym.to_string())
                                        .filter(|n| n != &bind_name);
                                    if let Some(ref inner_name) = inner_name_for_register {
                                        // Allocate the class id eagerly so we can register
                                        // it under both names; lower_class_from_ast picks up
                                        // the same id via lookup_class(bind_name).
                                        let class_id = ctx.fresh_class();
                                        ctx.register_class(bind_name.clone(), class_id);
                                        ctx.register_class(inner_name.clone(), class_id);
                                        ctx.class_expr_aliases
                                            .insert(inner_name.clone(), bind_name.clone());
                                    }
                                    // Lower the class with the binding name so
                                    // `new BindName(...)` works unchanged.
                                    let mut lowered_class =
                                        crate::lower_decl::lower_class_from_ast(
                                            ctx,
                                            &class_expr.class,
                                            &bind_name,
                                            false,
                                        )?;
                                    if let Some(inner_name) = inner_name_for_register {
                                        lowered_class.aliases.push(inner_name);
                                    }
                                    push_class_dedup(module, lowered_class);
                                    // Register the alias so `new X()` → `new X()`
                                    // (no-op lookup, but marks the binding as a class).
                                    ctx.class_expr_aliases
                                        .insert(bind_name.clone(), bind_name.clone());
                                    // We intentionally DO NOT push a Stmt::Let for
                                    // this binding — the class itself takes the
                                    // role of a "static value" referenced by name.
                                    continue;
                                }
                            }
                            // `const Mixed = MixinFn(BaseClass)` — detect a call
                            // to a known mixin function and synthesize a real
                            // class extending the supplied base. The mixin's
                            // class AST is taken from the pre-scan map and
                            // copied verbatim with the `extends` clause rewritten
                            // to point at the concrete base class.
                            if let ast::Expr::Call(call) = inner_expr {
                                if let ast::Callee::Expr(callee_expr) = &call.callee {
                                    if let ast::Expr::Ident(fn_ident) = callee_expr.as_ref() {
                                        let fn_name = fn_ident.sym.to_string();
                                        if let Some((_param_name, mixin_class_box)) =
                                            ctx.mixin_funcs.get(&fn_name).cloned()
                                        {
                                            if call.args.len() == 1 {
                                                if let ast::Expr::Ident(base_ident) =
                                                    call.args[0].expr.as_ref()
                                                {
                                                    let base_class_name =
                                                        base_ident.sym.to_string();
                                                    if ctx.lookup_class(&base_class_name).is_some()
                                                    {
                                                        let bind_name = ident.id.sym.to_string();
                                                        if ctx.lookup_class(&bind_name).is_none() {
                                                            let mut new_class =
                                                                (*mixin_class_box).clone();
                                                            let base_id = ast::Ident::new(
                                                                base_class_name.clone().into(),
                                                                base_ident.span,
                                                                base_ident.ctxt,
                                                            );
                                                            new_class.super_class = Some(Box::new(
                                                                ast::Expr::Ident(base_id),
                                                            ));
                                                            let lowered_class = crate::lower_decl::lower_class_from_ast(
                                                                ctx,
                                                                &new_class,
                                                                &bind_name,
                                                                false,
                                                            )?;
                                                            push_class_dedup(module, lowered_class);
                                                            ctx.class_expr_aliases.insert(
                                                                bind_name.clone(),
                                                                bind_name.clone(),
                                                            );
                                                            continue;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable)?;
                        // `var` is function-scoped: mark defined locals so
                        // `pop_block_scope` preserves them when leaving an inner block.
                        if is_var {
                            for s in &stmts {
                                if let Stmt::Let { id, .. } = s {
                                    ctx.var_hoisted_ids.insert(*id);
                                }
                            }
                        }
                        // Track awaited native module calls as native instances
                        // so property accesses (response.status, response.data) route
                        // through NativeMethodCall dispatch instead of generic PropertyGet.
                        for s in &stmts {
                            if let Stmt::Let {
                                name,
                                init: Some(Expr::Await(inner)),
                                ..
                            } = s
                            {
                                if let Expr::NativeMethodCall {
                                    module: mod_name,
                                    method,
                                    ..
                                } = inner.as_ref()
                                {
                                    let class_name = match (mod_name.as_str(), method.as_str()) {
                                        (
                                            "axios",
                                            "get" | "post" | "put" | "delete" | "patch" | "request",
                                        ) => Some("Response"),
                                        ("mongodb", "connect") => Some("MongoClient"),
                                        ("pg", "connect") => Some("Client"),
                                        _ => None,
                                    };
                                    if let Some(cn) = class_name {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            mod_name.clone(),
                                            cn.to_string(),
                                        );
                                    }
                                }
                            }
                            // Track synchronous native module factories as native instances.
                            // Added for workstream A1.5 so `const sock = net.createConnection(...)`
                            // registers `sock` as a Socket instance; without this, subsequent
                            // `sock.write/on/end/destroy` miss the NATIVE_MODULE_TABLE dispatch
                            // and never reach the `js_net_socket_*` FFI in perry-stdlib.
                            if let Stmt::Let {
                                name,
                                init:
                                    Some(Expr::NativeMethodCall {
                                        module: mod_name,
                                        method,
                                        object: None,
                                        ..
                                    }),
                                ..
                            } = s
                            {
                                let class_name = match (mod_name.as_str(), method.as_str()) {
                                    ("net", "createConnection" | "connect") => Some("Socket"),
                                    // tls.connect returns the same Socket class — reuses
                                    // all the write/end/destroy/on/upgradeToTLS dispatch.
                                    ("tls", "connect") => Some("Socket"),
                                    // Issue #422: `new net.Socket()` lowers to a
                                    // receiver-less `Expr::NativeMethodCall` whose
                                    // method is the constructor name "Socket"; this
                                    // arm registers the binding so `sock.connect/...`
                                    // dispatches via the class-filtered entries.
                                    ("net", "Socket") => Some("Socket"),
                                    _ => None,
                                };
                                if let Some(cn) = class_name {
                                    // Register under `"net"` (the module the Socket class belongs to)
                                    // regardless of which module the factory lived in, so method
                                    // dispatch resolves correctly.
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "net".to_string(),
                                        cn.to_string(),
                                    );
                                    let _ = mod_name; // suppress unused on tls branch
                                }
                                // Issue #577 — node:http / node:https / node:http2 server
                                // factories. `const s = createServer(...)` (named import)
                                // and `const s = http.createServer(...)` (namespace import)
                                // both lower to a receiver-less NativeMethodCall here, so
                                // this arm covers both shapes.
                                let http_class = match (mod_name.as_str(), method.as_str()) {
                                    ("http", "createServer") => Some("HttpServer"),
                                    ("https", "createServer") => Some("HttpsServer"),
                                    ("http2", "createSecureServer") => Some("Http2SecureServer"),
                                    ("async_hooks", "createHook") => Some("AsyncHook"),
                                    _ => None,
                                };
                                if let Some(cn) = http_class {
                                    let module_owned = mod_name.clone();
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_owned.clone(),
                                        cn.to_string(),
                                    );
                                    ctx.module_native_instances.push((
                                        name.clone(),
                                        module_owned,
                                        cn.to_string(),
                                    ));
                                }
                                // Issue #769 — node:http / node:https CLIENT factories.
                                // `const req = http.request(url, cb)` and `http.get` / `https.*`
                                // variants return a ClientRequest handle; register under
                                // module `"http"` so subsequent `req.on/.end/.write/...`
                                // resolve via the class-filtered entries in NATIVE_MODULE_TABLE.
                                let client_class = match (mod_name.as_str(), method.as_str()) {
                                    ("http", "request" | "get") => Some("ClientRequest"),
                                    ("https", "request" | "get") => Some("ClientRequest"),
                                    _ => None,
                                };
                                if let Some(cn) = client_class {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "http".to_string(),
                                        cn.to_string(),
                                    );
                                }
                            }
                            // User-defined factory wrappers: when the init is a
                            // bare call to `userFunc(...)` and `userFunc` was
                            // registered as a native-instance factory (via
                            // its declared return type), inherit the class so
                            // downstream `local.method(...)` dispatches statically.
                            // Example: `function openSocket(): Socket { ... }`
                            // followed by `const sock = openSocket(...)` registers
                            // sock as ("net", "Socket").
                            if let Stmt::Let {
                                name,
                                init: Some(Expr::Call { callee, .. }),
                                ..
                            } = s
                            {
                                if let Expr::FuncRef(func_id) = callee.as_ref() {
                                    let func_name_owned =
                                        ctx.lookup_func_name(*func_id).map(|s| s.to_string());
                                    if let Some(func_name) = func_name_owned {
                                        let lookup = ctx
                                            .lookup_func_return_native_instance(&func_name)
                                            .map(|(m, c)| (m.to_string(), c.to_string()));
                                        if let Some((m, c)) = lookup {
                                            ctx.register_native_instance(name.clone(), m, c);
                                        }
                                    }
                                }
                            }
                        }
                        module.init.extend(stmts);
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let class = lower_class_decl(ctx, class_decl, false)?;
                    // Issue #711: emit dynamic parent-class registration
                    // at the source-order position of the class declaration
                    // BEFORE the static-field-init stmts. Static field
                    // initializers may reference inherited static methods,
                    // and method dispatch reads the parent chain — wiring
                    // the parent edge first keeps the inherited lookup
                    // path consistent for those inits.
                    if let Some(extends_expr) = &class.extends_expr {
                        module
                            .init
                            .push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                                class_name: class.name.clone(),
                                parent_expr: extends_expr.clone(),
                            }));
                    }
                    // Inject static-field-init statements at the source
                    // position of the class declaration. Per ES spec, a
                    // class declaration's static initializers run when the
                    // declaration evaluates — i.e., here in source order,
                    // not at the top of module init. This matters when a
                    // static field's initializer references a top-level
                    // const declared earlier in the module: the upfront
                    // `init_static_fields` pass at codegen.rs:3449 runs
                    // before any user `Let` bindings, so it captures
                    // unbound (undefined) values. The inline statements
                    // re-run with the correct values once we reach this
                    // point in source order.
                    for sf in &class.static_fields {
                        if let Some(init) = &sf.init {
                            if let Some(key) = sf.key_expr.as_ref() {
                                module.init.push(Stmt::Expr(Expr::ClassStaticSymbolSet {
                                    class_name: class.name.clone(),
                                    key: Box::new(key.clone()),
                                    value: Box::new(init.clone()),
                                }));
                            } else {
                                module.init.push(Stmt::Expr(Expr::StaticFieldSet {
                                    class_name: class.name.clone(),
                                    field_name: sf.name.clone(),
                                    value: Box::new(init.clone()),
                                }));
                            }
                        }
                    }
                    append_legacy_decorator_init_for_class(ctx, &mut module.init, &class);
                    push_class_dedup(module, class);
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, false)?;
                    module.enums.push(en);
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, false)?;
                    module.interfaces.push(iface);
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, false)?;
                    module.type_aliases.push(alias);
                }
                ast::Decl::Using(using_decl) => {
                    // `using x = expr` / `await using x = expr` — TC39 Explicit
                    // Resource Management. Lower as const bindings. Disposal at
                    // block-scope exit is not yet automated — the variables are
                    // accessible but [Symbol.dispose/asyncDispose] isn't called.
                    // Treat as a const var declaration.
                    let fake_var = ast::VarDecl {
                        span: using_decl.span,
                        kind: ast::VarDeclKind::Const,
                        declare: false,
                        decls: using_decl.decls.clone(),
                        ctxt: Default::default(),
                    };
                    let mutable = false;
                    let _is_var = false;
                    for decl in &fake_var.decls {
                        if let Some(init) = &decl.init {
                            if let ast::Pat::Ident(bind_ident) = &decl.name {
                                let name = bind_ident.sym.to_string();
                                let init_expr = lower_expr(ctx, init)?;
                                let ty = Type::Any;
                                let id = ctx.fresh_local();
                                ctx.locals.push((name.clone(), id, ty.clone()));
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable,
                                    init: Some(init_expr),
                                });
                            }
                        }
                    }
                }
                ast::Decl::TsModule(ts_module) => {
                    // namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let class =
                                lower_namespace_as_class(ctx, module, &ns_name, body, false)?;
                            push_class_dedup(module, class);
                        }
                    }
                }
                // #853: `ast::Decl` is `#[non_exhaustive]` upstream — keep
                // this catch-all so a future SWC variant is dropped silently
                // (the supported variants above each have explicit handling).
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            // Check if this is a destructuring assignment that needs special handling
            if let ast::Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    module.init.extend(stmts);
                    return Ok(());
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            module.init.push(Stmt::Expr(expr));
        }
        ast::Stmt::If(if_stmt) => {
            let condition = lower_expr(ctx, &if_stmt.test)?;
            // Each branch introduces its own lexical scope. Skip extra push if
            // branch is a BlockStmt (handled there) or an If (else-if chain).
            let then_branch = if matches!(*if_stmt.cons, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &if_stmt.cons)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &if_stmt.cons)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            let else_branch = if_stmt
                .alt
                .as_ref()
                .map(|s| {
                    if matches!(**s, ast::Stmt::Block(_)) || matches!(**s, ast::Stmt::If(_)) {
                        lower_body_stmt(ctx, s)
                    } else {
                        let mark = ctx.push_block_scope();
                        let stmts = lower_body_stmt(ctx, s);
                        ctx.pop_block_scope(mark);
                        stmts
                    }
                })
                .transpose()?;
            module.init.push(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        ast::Stmt::While(while_stmt) => {
            let condition = lower_expr(ctx, &while_stmt.test)?;
            let body = if matches!(*while_stmt.body, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &while_stmt.body)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &while_stmt.body)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            module.init.push(Stmt::While { condition, body });
        }
        ast::Stmt::DoWhile(do_while_stmt) => {
            let body = lower_body_stmt(ctx, &do_while_stmt.body)?;
            let condition = lower_expr(ctx, &do_while_stmt.test)?;
            module.init.push(Stmt::DoWhile { body, condition });
        }
        ast::Stmt::Labeled(labeled_stmt) => {
            let label = labeled_stmt.label.sym.to_string();
            let inner = lower_body_stmt(ctx, &labeled_stmt.body)?;
            if inner.len() == 1 {
                let body = inner.into_iter().next().unwrap();
                module.init.push(Stmt::Labeled {
                    label,
                    body: Box::new(body),
                });
            } else {
                let mut inner = inner;
                let last = inner.pop().unwrap();
                for s in inner {
                    module.init.push(s);
                }
                module.init.push(Stmt::Labeled {
                    label,
                    body: Box::new(last),
                });
            }
        }
        ast::Stmt::For(for_stmt) => {
            // Push a lexical scope covering init/test/update/body, so
            // `for (let i = 0; ...)` bindings don't leak to the outer scope.
            let for_scope_mark = ctx.push_block_scope();
            let init = if let Some(init) = &for_stmt.init {
                match init {
                    ast::VarDeclOrExpr::VarDecl(var_decl) => {
                        let is_var = var_decl.kind == ast::VarDeclKind::Var;
                        if is_var {
                            for decl in var_decl.decls.iter() {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                ctx.var_hoisted_ids.insert(id);
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                });
                            }
                            None
                        } else {
                            for decl in var_decl.decls.iter().skip(1) {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                });
                            }
                            if let Some(decl) = var_decl.decls.first() {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                Some(Box::new(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                }))
                            } else {
                                None
                            }
                        }
                    }
                    ast::VarDeclOrExpr::Expr(expr) => {
                        Some(Box::new(Stmt::Expr(lower_expr(ctx, expr)?)))
                    }
                }
            } else {
                None
            };
            let condition = for_stmt
                .test
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .transpose()?;
            let update = for_stmt
                .update
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .transpose()?;
            let body = lower_body_stmt(ctx, &for_stmt.body)?;
            ctx.pop_block_scope(for_scope_mark);
            module.init.push(Stmt::For {
                init,
                condition,
                update,
                body,
            });
        }
        ast::Stmt::Block(block) => {
            // Bare block: introduce a lexical scope so inner let/const shadow
            // without leaking into the enclosing module scope.
            let stmts = lower_block_stmt_scoped(ctx, block)?;
            for stmt in stmts {
                module.init.push(stmt);
            }
        }
        ast::Stmt::Try(try_stmt) => {
            // try body is its own lexical scope
            let body = lower_block_stmt_scoped(ctx, &try_stmt.block)?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    Some((param_id, param_name))
                } else {
                    None
                };

                let catch_body = lower_block_stmt(ctx, &catch_clause.body)?;
                ctx.exit_scope(scope_mark);

                Some(CatchClause {
                    param,
                    body: catch_body,
                })
            } else {
                None
            };

            // finally block is its own lexical scope
            let finally = if let Some(ref finally_block) = try_stmt.finalizer {
                Some(lower_block_stmt_scoped(ctx, finally_block)?)
            } else {
                None
            };

            module.init.push(Stmt::Try {
                body,
                catch,
                finally,
            });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            module.init.push(Stmt::Throw(expr));
        }
        ast::Stmt::Switch(switch_stmt) => {
            let discriminant = lower_expr(ctx, &switch_stmt.discriminant)?;
            let mut cases = Vec::new();

            for case in &switch_stmt.cases {
                let test = case.test.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;

                let mut body = Vec::new();
                for stmt in &case.cons {
                    body.extend(lower_body_stmt(ctx, stmt)?);
                }

                cases.push(SwitchCase { test, body });
            }

            module.init.push(Stmt::Switch {
                discriminant,
                cases,
            });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // --- Iterator protocol path for generators ---
            // Detect: for (const x of genFunc(...)) where genFunc is function*
            let is_generator_call = if let ast::Expr::Call(call) = &*for_of_stmt.right {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = &**callee_expr {
                        ctx.generator_func_names.contains(ident.sym.as_ref())
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            // Detect whether the called generator was an `async function*`.
            // Async generators always return `Promise<{value, done}>` from
            // `.next()`, so the iterator-protocol loop must `await` each
            // call before reading `.value` / `.done`. Either the user
            // wrote `for await (...)` (SWC `is_await`) or the callee was
            // declared async — both must trigger awaiting.
            let callee_is_async_gen = if let ast::Expr::Call(call) = &*for_of_stmt.right {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = &**callee_expr {
                        ctx.async_generator_func_names.contains(ident.sym.as_ref())
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            let needs_await = for_of_stmt.is_await || callee_is_async_gen;

            // Also detect: for (const x of new Range(...)) where Range
            // defines `*[Symbol.iterator]()`. We lowered that method as
            // a synthesized top-level generator function taking `this`
            // as its first parameter; the for-of here dispatches by
            // calling that function with the lowered receiver.
            let iter_from_class: Option<perry_types::FuncId> =
                if let ast::Expr::New(new_expr) = &*for_of_stmt.right {
                    if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                        let class_name = ident.sym.to_string();
                        ctx.iterator_func_for_class.get(&class_name).copied()
                    } else {
                        None
                    }
                } else {
                    None
                };

            if is_generator_call || iter_from_class.is_some() {
                // Lower to iterator protocol:
                //   let __iter = genFunc(...);                     // generator-fn path
                //   let __iter = __perry_iter_Range(new Range(...));  // class path
                //   let __result = __iter.next();
                //   while (!__result.done) { const x = __result.value; body; __result = __iter.next(); }
                let for_scope_mark = ctx.push_block_scope();
                let iter_expr = lower_expr(ctx, &for_of_stmt.right)?;
                // For the class path we wrap the lowered `new Range(..)`
                // in a direct FuncRef call to the synthesized iterator
                // function (which has `this` as its first parameter).
                let iter_expr = if let Some(iter_fn_id) = iter_from_class {
                    Expr::Call {
                        callee: Box::new(Expr::FuncRef(iter_fn_id)),
                        args: vec![iter_expr],
                        type_args: vec![],
                    }
                } else {
                    iter_expr
                };
                let iter_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
                module.init.push(Stmt::Let {
                    id: iter_id,
                    name: format!("__iter_{}", iter_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(iter_expr),
                });

                let result_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__result_{}", result_id), result_id, Type::Any));
                // __result = __iter.next()
                // For async generators / `for await ... of`, wrap the
                // call in `Expr::Await` so the resolved iter-result
                // (`{value, done}`) is what's stored, not the Promise.
                let raw_next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };
                let next_call = if needs_await {
                    Expr::Await(Box::new(raw_next_call))
                } else {
                    raw_next_call
                };
                module.init.push(Stmt::Let {
                    id: result_id,
                    name: format!("__result_{}", result_id),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(next_call.clone()),
                });

                // Extract the loop variable binding
                let item_name = if let ast::ForHead::VarDecl(var_decl) = &for_of_stmt.left {
                    if let Some(decl) = var_decl.decls.first() {
                        if let ast::Pat::Ident(ident) = &decl.name {
                            ident.id.sym.to_string()
                        } else {
                            "__gen_item".to_string()
                        }
                    } else {
                        "__gen_item".to_string()
                    }
                } else {
                    "__gen_item".to_string()
                };
                let item_id = ctx.define_local(item_name.clone(), Type::Any);

                // Lower loop body
                let mut body_stmts = Vec::new();
                // const x = __result.value
                body_stmts.push(Stmt::Let {
                    id: item_id,
                    name: item_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(result_id)),
                        property: "value".to_string(),
                    }),
                });
                // Lower user body statements. lower_stmt appends to module.init,
                // so we snapshot and drain to capture the body stmts.
                // Handle both Block bodies (`for (...) { ... }`) AND single-statement
                // bodies (`for (...) console.log(v);`). Pre-fix the brace-less
                // form was silently dropped — `for (const v of gen()) doThing(v);`
                // produced no output at all.
                let init_before = module.init.len();
                if let ast::Stmt::Block(block) = &*for_of_stmt.body {
                    for s in &block.stmts {
                        lower_stmt(ctx, module, s)?;
                    }
                } else {
                    lower_stmt(ctx, module, &for_of_stmt.body)?;
                }
                let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
                body_stmts.append(&mut user_body);
                // __result = __iter.next()
                body_stmts.push(Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))));

                // while (!__result.done) { body }
                module.init.push(Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: body_stmts,
                });

                ctx.pop_block_scope(for_scope_mark);
                return Ok(());
            }

            // --- Standard array-based for-of path ---
            // Desugar for-of to a regular for loop:
            // for (const x of arr) { body }
            // becomes:
            // { let __arr = arr; for (let __i = 0; __i < __arr.length; __i++) { const x = __arr[__i]; body } }
            // Push a block scope so loop variables and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();

            // Detect string iteration BEFORE lowering (so we can use the AST-level type info).
            // for (const ch of "hello") — each iteration yields a 1-char string via str[i].
            let is_string_iter = is_ast_string_expr(ctx, &for_of_stmt.right);

            // `for (const [k, v] of h)` where h is a Headers handle: WHATWG
            // Fetch spec says iteration of a Headers object yields `[key,
            // value]` pairs sorted by key. Without this rewrite, for-of falls
            // through to the generic array path and reads `.length` on the
            // raw handle (returns 0 → silent empty loop). Refs #576.
            let is_headers_iter = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => matches!(
                    ctx.lookup_native_instance(ident.sym.as_ref()),
                    Some((_, "Headers"))
                ),
                _ => false,
            };

            // `for (const [k, v] of params)` where `params` is a
            // URLSearchParams local. Same shape as the Headers case but
            // tracked via `lookup_local_type` (Type::Named) instead of the
            // native-instance registry. Refs #575.
            let is_urlsp_iter = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => matches!(
                    ctx.lookup_local_type(ident.sym.as_ref()),
                    Some(Type::Named(n)) if n == "URLSearchParams"
                ),
                ast::Expr::New(new_expr) => matches!(
                    new_expr.callee.as_ref(),
                    ast::Expr::Ident(c) if c.sym.as_ref() == "URLSearchParams"
                ),
                _ => false,
            };

            // Lower the iterable expression (the array)
            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;
            let arr_expr = if is_headers_iter {
                Expr::NativeMethodCall {
                    module: "Headers".to_string(),
                    class_name: Some("Headers".to_string()),
                    object: Some(Box::new(arr_expr)),
                    method: "entries".to_string(),
                    args: vec![],
                }
            } else if is_urlsp_iter {
                Expr::UrlSearchParamsEntries(Box::new(arr_expr))
            } else {
                arr_expr
            };

            // Issue #302: resolve iterable type from either local var or
            // class instance field (`this.someMap`). Was limited to
            // `Ident` only. Issue #311 extends to plain object property
            // access (`obj.m` where `obj` is a local with an inferred
            // `Type::Object` shape) — without this arm `for (const x of
            // obj.m)` fell through to `None`, the loop read `.length` on
            // a raw Map handle (returns 0), and silently iterated zero
            // times.
            let iterable_type: Option<Type> = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => ctx.lookup_local_type(ident.sym.as_ref()).cloned(),
                ast::Expr::Member(m) => {
                    if matches!(m.obj.as_ref(), ast::Expr::This(_)) {
                        if let (Some(cls), ast::MemberProp::Ident(p)) =
                            (ctx.current_class.clone(), &m.prop)
                        {
                            ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned()
                        } else {
                            None
                        }
                    } else if let ast::MemberProp::Ident(p) = &m.prop {
                        let obj_ty = crate::lower_types::infer_type_from_expr(&m.obj, ctx);
                        match obj_ty {
                            Type::Object(ot) => {
                                ot.properties.get(p.sym.as_ref()).map(|pi| pi.ty.clone())
                            }
                            // Class instance: receiver is `new Example()` or
                            // a local typed `Example`. Consult the same
                            // class_field_types registry the `this.<field>`
                            // arm uses (populated for #302).
                            Type::Named(cls) => {
                                ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned()
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };

            // If the iterable is a Map, wrap in MapEntries to convert to array
            // This handles: for (const [k, v] of myMap) { ... } AND
            // for (const [k, v] of this.classMap) { ... } per #302.
            let mut map_key_type: Option<Type> = None;
            let mut map_val_type: Option<Type> = None;
            // Issue #542/#543: also accept Type::Union containing Map (the
            // shape produced by `Map<K, V> | undefined` parameters/returns).
            let type_contains_map =
                |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Map") };
            let is_iterable_map = match &iterable_type {
                Some(Type::Generic { base, .. }) if base == "Map" => true,
                Some(Type::Union(variants)) => variants.iter().any(type_contains_map),
                _ => false,
            };
            // Fast path: `for (const [k, v] of mapExpr)` with an exact two-element
            // identifier destructure can iterate the Map's flat entries buffer
            // directly via `MapEntryKeyAt` / `MapEntryValueAt`, skipping the N+1
            // small Array allocations that `MapEntries` would do per iteration.
            // Detected here so we can keep the iterable expression unwrapped
            // and emit a different binding/bound shape below.
            // Map fast path also fires for the single-binding shapes
            //   for (const [k] of map)        — only key
            //   for (const [, v] of map)      — only value
            // Each non-empty slot must be a plain Ident (no nested patterns).
            // Anything else falls through to the MapEntries materialization
            // path so destructuring semantics for objects / nested arrays
            // / defaults stay correct.
            let map_kv_fastpath = is_iterable_map
                && match &for_of_stmt.left {
                    ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                        Some(decl) => match &decl.name {
                            ast::Pat::Array(arr_pat) => {
                                let len = arr_pat.elems.len();
                                (len == 1 || len == 2)
                                    && arr_pat.elems.iter().all(|e| {
                                        e.is_none() || matches!(e, Some(ast::Pat::Ident(_)))
                                    })
                            }
                            _ => false,
                        },
                        None => false,
                    },
                    _ => false,
                };
            // Fast path: `for (const x of setExpr)` with a single-Ident
            // binding. Reads elements directly via `SetValueAt` (→
            // `js_set_value_at`) instead of materializing the buffer with
            // `js_set_to_array`. ECS hot paths (changeset.removes, etc.)
            // iterate Sets repeatedly; this saves an Array alloc per loop.
            // Issue #542/#543: also accept Type::Union containing Set.
            let type_contains_set =
                |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Set") };
            let is_iterable_set = match &iterable_type {
                Some(Type::Generic { base, .. }) if base == "Set" => true,
                Some(Type::Union(variants)) => variants.iter().any(type_contains_set),
                _ => false,
            };
            let set_fastpath = is_iterable_set
                && match &for_of_stmt.left {
                    ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                        Some(decl) => matches!(&decl.name, ast::Pat::Ident(_)),
                        None => false,
                    },
                    _ => false,
                };
            // Issue #542/#543: dispatch on `is_iterable_map` / `is_iterable_set`
            // so the Union-with-Map / Union-with-Set shapes also wrap correctly
            // (matches the same fix applied to `lower_decl.rs`'s for-of arm).
            // Extract the Map's K/V type args from whichever variant carries
            // them (direct Generic or the Union's Map arm).
            let map_type_args: Option<Vec<Type>> = if is_iterable_map {
                match &iterable_type {
                    Some(Type::Generic { base, type_args }) if base == "Map" => {
                        Some(type_args.clone())
                    }
                    Some(Type::Union(variants)) => variants.iter().find_map(|v| match v {
                        Type::Generic { base, type_args } if base == "Map" => {
                            Some(type_args.clone())
                        }
                        _ => None,
                    }),
                    _ => None,
                }
            } else {
                None
            };
            // Issue #578: typed-array iterables. Wrap in `Expr::ArrayFrom`
            // so the holder is a regular Array of materialized element values.
            // Without this, the generated `for (let i=0; i<__arr.length; ++i)
            // __item = __arr[i]` loop reads f64s straight off the typed
            // array's byte-packed storage and yields raw bit reinterpretations.
            // `js_array_clone` (the runtime backing of `ArrayFrom`) detects the
            // typed-array tag and materializes through the per-kind accessor.
            let is_iterable_typed_array = matches!(
                &iterable_type,
                Some(Type::Named(name)) if matches!(name.as_str(),
                    "Uint8Array" | "Int8Array" | "Uint8ClampedArray"
                    | "Uint16Array" | "Int16Array"
                    | "Uint32Array" | "Int32Array"
                    | "Float32Array" | "Float64Array"
                )
            );
            let arr_expr = if is_iterable_map {
                if let Some(args) = map_type_args.as_ref() {
                    if args.len() >= 2 {
                        map_key_type = Some(args[0].clone());
                        map_val_type = Some(args[1].clone());
                    }
                }
                if map_kv_fastpath {
                    arr_expr
                } else {
                    Expr::MapEntries(Box::new(arr_expr))
                }
            } else if is_iterable_set {
                if set_fastpath {
                    arr_expr
                } else {
                    Expr::SetValues(Box::new(arr_expr))
                }
            } else if is_iterable_typed_array {
                Expr::ArrayFrom(Box::new(arr_expr))
            } else {
                arr_expr
            };

            // Determine the array element type: String for strings, Tuple(K, V) for Maps, Any otherwise.
            // For an identifier iterable like `for (const word of words)` where
            // `words: string[]`, extract the element type from the local's
            // declared Array<T> so the synthesized iteration variable gets
            // the right type (was always Any, breaking `word.length` etc.).
            // #302: also draws Set + class-field Array element types
            // from the resolved `iterable_type` above instead of
            // re-doing the Ident lookup here.
            let elem_type = if is_string_iter {
                Type::String
            } else if let (Some(ref k), Some(ref v)) = (&map_key_type, &map_val_type) {
                Type::Tuple(vec![k.clone(), v.clone()])
            } else if is_iterable_typed_array {
                // Issue #578: typed-array element values are always Number.
                Type::Number
            } else {
                match &iterable_type {
                    Some(Type::Array(elem)) => (**elem).clone(),
                    Some(Type::Generic { base, type_args })
                        if base == "Array" && type_args.len() == 1 =>
                    {
                        type_args[0].clone()
                    }
                    Some(Type::Generic { base, type_args })
                        if base == "Set" && !type_args.is_empty() =>
                    {
                        type_args[0].clone()
                    }
                    _ => Type::Any,
                }
            };
            // The __arr holder's type: String for string iteration, Map for
            // the Map-fast-path so `__m.size` resolves through `is_map_expr`,
            // Array otherwise.
            let arr_type = if is_string_iter {
                Type::String
            } else if map_kv_fastpath {
                Type::Generic {
                    base: "Map".to_string(),
                    type_args: vec![
                        map_key_type.clone().unwrap_or(Type::Any),
                        map_val_type.clone().unwrap_or(Type::Any),
                    ],
                }
            } else if set_fastpath {
                Type::Generic {
                    base: "Set".to_string(),
                    type_args: vec![elem_type.clone()],
                }
            } else {
                Type::Array(Box::new(elem_type.clone()))
            };

            // Create internal variables for the array and index
            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            // Register these in the context so they can be looked up
            ctx.locals
                .push((format!("__arr_{}", arr_id), arr_id, arr_type.clone()));
            ctx.locals
                .push((format!("__idx_{}", idx_id), idx_id, Type::Number));

            // Store array reference: let __arr = arr
            module.init.push(Stmt::Let {
                id: arr_id,
                name: format!("__arr_{}", arr_id),
                ty: arr_type,
                mutable: false,
                init: Some(arr_expr),
            });

            // IMPORTANT: Define iteration variables BEFORE lowering the body
            // so the body can reference them
            let item_id = ctx.fresh_local();
            ctx.locals
                .push((format!("__item_{}", item_id), item_id, elem_type.clone()));

            // Pre-define all variables from the pattern so body can reference them
            let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        match &decl.name {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                let id = ctx.define_local(name.clone(), elem_type.clone());
                                vec![(name, id)]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut ids = Vec::new();
                                for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(ident) = elem_pat {
                                            let name = ident.id.sym.to_string();
                                            // For Map destructuring [k, v], use key type for idx 0, value type for idx 1
                                            let var_type = if let Type::Tuple(ref types) = elem_type
                                            {
                                                types.get(idx).cloned().unwrap_or(Type::Any)
                                            } else {
                                                Type::Any
                                            };
                                            let id = ctx.define_local(name.clone(), var_type);
                                            ids.push((name, id));
                                        }
                                    }
                                }
                                ids
                            }
                            ast::Pat::Object(obj_pat) => {
                                let mut ids = Vec::new();
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let name = assign.key.sym.to_string();
                                            let id = ctx.define_local(name.clone(), Type::Any);
                                            ids.push((name, id));
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            if let ast::Pat::Ident(ident) = &*kv.value {
                                                let name = ident.id.sym.to_string();
                                                let id = ctx.define_local(name.clone(), Type::Any);
                                                ids.push((name, id));
                                            } else {
                                                // Nested pattern (e.g. `key: [a, b]`).
                                                // Recurse so leaves get pre-defined and
                                                // the body can reference them. Issue #554.
                                                collect_for_of_pattern_leaves(
                                                    ctx, &kv.value, &mut ids,
                                                );
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                ids
                            }
                            _ => {
                                let name = get_binding_name(&decl.name)?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                vec![(name, id)]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => {
                    let name = get_pat_name(pat)?;
                    let id = ctx.define_local(name.clone(), Type::Any);
                    vec![(name, id)]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // NOW lower the body - variables are defined so body can reference them
            let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

            // Build binding statements using the pre-defined variable IDs
            let binding_stmts = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        // `for await (const x of arr)`: spec ECMA-262 §14.7.5.10
                        // says each iteration must Await the value yielded by
                        // the iterator. For a plain-array iterable that means
                        // `await arr[i]` — unwraps a Promise element into its
                        // resolved value before binding. Without this, `for
                        // await (const x of [Promise.resolve(1), …])` would
                        // bind `x = <Promise object>` and any numeric op would
                        // see NaN. The iterator-protocol path above already
                        // wraps the `__iter.next()` call in `Expr::Await` for
                        // async generators; this brings the array-iteration
                        // path to parity.
                        let raw_item_expr = Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        };
                        let item_expr = if for_of_stmt.is_await {
                            Expr::Await(Box::new(raw_item_expr))
                        } else {
                            raw_item_expr
                        };

                        match &decl.name {
                            ast::Pat::Ident(_) => {
                                // Simple binding: for (const x of arr)
                                let (name, id) = var_ids[0].clone();
                                let init = if set_fastpath {
                                    Expr::SetValueAt {
                                        set: Box::new(Expr::LocalGet(arr_id)),
                                        idx: Box::new(Expr::LocalGet(idx_id)),
                                    }
                                } else {
                                    item_expr
                                };
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: elem_type.clone(),
                                    mutable: false,
                                    init: Some(init),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
                                if map_kv_fastpath {
                                    // Map [k, v] / [k] / [, v] fast path: read
                                    // each requested entry slot directly from
                                    // the Map's flat buffer at the loop index.
                                    // No `__item` Array materialization. Skipped
                                    // slots ([,v] etc.) emit no binding.
                                    let key_ty = map_key_type.clone().unwrap_or(Type::Any);
                                    let val_ty = map_val_type.clone().unwrap_or(Type::Any);
                                    let mut stmts: Vec<Stmt> = Vec::new();
                                    let mut var_idx = 0;
                                    for (slot, elem) in arr_pat.elems.iter().enumerate() {
                                        let Some(ast::Pat::Ident(_)) = elem else {
                                            continue;
                                        };
                                        let (name, id) = var_ids[var_idx].clone();
                                        var_idx += 1;
                                        let (ty, init) = if slot == 0 {
                                            (
                                                key_ty.clone(),
                                                Expr::MapEntryKeyAt {
                                                    map: Box::new(Expr::LocalGet(arr_id)),
                                                    idx: Box::new(Expr::LocalGet(idx_id)),
                                                },
                                            )
                                        } else {
                                            (
                                                val_ty.clone(),
                                                Expr::MapEntryValueAt {
                                                    map: Box::new(Expr::LocalGet(arr_id)),
                                                    idx: Box::new(Expr::LocalGet(idx_id)),
                                                },
                                            )
                                        };
                                        stmts.push(Stmt::Let {
                                            id,
                                            name,
                                            ty,
                                            mutable: false,
                                            init: Some(init),
                                        });
                                    }
                                    stmts
                                } else {
                                    // Array destructuring: for (const [a, b] of arr)
                                    let mut stmts = vec![Stmt::Let {
                                        id: item_id,
                                        name: format!("__item_{}", item_id),
                                        ty: elem_type.clone(),
                                        mutable: false,
                                        init: Some(item_expr),
                                    }];

                                    // Extract each element using pre-defined IDs
                                    let mut var_idx = 0;
                                    for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                        if let Some(elem_pat) = elem {
                                            if let ast::Pat::Ident(_) = elem_pat {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                // For Map destructuring, use the Tuple element type
                                                let var_type =
                                                    if let Type::Tuple(ref types) = elem_type {
                                                        types.get(idx).cloned().unwrap_or(Type::Any)
                                                    } else {
                                                        Type::Any
                                                    };
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: var_type,
                                                    mutable: false,
                                                    init: Some(Expr::IndexGet {
                                                        object: Box::new(Expr::LocalGet(item_id)),
                                                        index: Box::new(Expr::Number(idx as f64)),
                                                    }),
                                                });
                                            }
                                        }
                                    }
                                    stmts
                                }
                            }
                            ast::Pat::Object(obj_pat) => {
                                // Object destructuring: for (const { a, b } of arr)
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];

                                // Extract each property using pre-defined IDs
                                let mut var_idx = 0;
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let prop_name = assign.key.sym.to_string();
                                            let (name, id) = var_ids[var_idx].clone();
                                            var_idx += 1;
                                            let init_value = if let Some(default_expr) =
                                                &assign.value
                                            {
                                                let prop_access = Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                };
                                                let default_val = lower_expr(ctx, default_expr)?;
                                                let condition = Expr::Compare {
                                                    op: CompareOp::Ne,
                                                    left: Box::new(prop_access.clone()),
                                                    right: Box::new(Expr::Undefined),
                                                };
                                                Expr::Conditional {
                                                    condition: Box::new(condition),
                                                    then_expr: Box::new(prop_access),
                                                    else_expr: Box::new(default_val),
                                                }
                                            } else {
                                                Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                }
                                            };
                                            stmts.push(Stmt::Let {
                                                id,
                                                name,
                                                ty: Type::Any,
                                                mutable: false,
                                                init: Some(init_value),
                                            });
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            let key = match &kv.key {
                                                ast::PropName::Ident(ident) => {
                                                    ident.sym.to_string()
                                                }
                                                ast::PropName::Str(s) => {
                                                    s.value.as_str().unwrap_or("").to_string()
                                                }
                                                _ => continue,
                                            };
                                            let key_source = Expr::PropertyGet {
                                                object: Box::new(Expr::LocalGet(item_id)),
                                                property: key,
                                            };
                                            if let ast::Pat::Ident(_) = &*kv.value {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: Type::Any,
                                                    mutable: false,
                                                    init: Some(key_source),
                                                });
                                            } else {
                                                // Nested pattern (e.g. `key: [a, b]`).
                                                // Issue #554.
                                                emit_for_of_pattern_binding(
                                                    ctx,
                                                    &kv.value,
                                                    key_source,
                                                    &var_ids,
                                                    &mut var_idx,
                                                    &mut stmts,
                                                )?;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                stmts
                            }
                            _ => {
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(Expr::IndexGet {
                                        object: Box::new(Expr::LocalGet(arr_id)),
                                        index: Box::new(Expr::LocalGet(idx_id)),
                                    }),
                                }]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(_) => {
                    let (name, id) = var_ids[0].clone();
                    vec![Stmt::Let {
                        id,
                        name,
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        }),
                    }]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // Prepend the binding statements to the loop body
            for (i, stmt) in binding_stmts.into_iter().enumerate() {
                loop_body.insert(i, stmt);
            }

            // Loop bound. Map/Set fast paths read `.size` (lowered by
            // codegen to `js_map_size` / `js_set_size`); regular path uses
            // `__arr.length` against the materialized iterable.
            let bound_expr = if map_kv_fastpath || set_fastpath {
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(arr_id)),
                    property: "size".to_string(),
                }
            } else {
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(arr_id)),
                    property: "length".to_string(),
                }
            };
            // Create the for loop:
            // for (let __i = 0; __i < __arr.length; __i++) { ... }
            module.init.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(bound_expr),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        ast::Stmt::ForIn(for_in_stmt) => {
            // Desugar for-in to a for-of over Object.keys(obj):
            // for (const key in obj) { body }
            // becomes:
            // { let __keys = Object.keys(obj); for (let __i = 0; __i < __keys.length; __i++) { const key = __keys[__i]; body } }
            // Push a block scope so the loop key and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();

            // Get the iteration variable name
            let key_name = match &for_in_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        get_binding_name(&decl.name)?
                    } else {
                        return Err(anyhow!("for-in requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => get_pat_name(pat)?,
                _ => return Err(anyhow!("Unsupported for-in left-hand side")),
            };

            // Lower the object expression
            let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;

            // Create Object.keys(obj) expression to get the array of keys
            let keys_expr = Expr::ObjectKeys(Box::new(obj_expr));

            // Create internal variables for the keys array and index
            let keys_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            let key_id = ctx.define_local(key_name.clone(), Type::String);

            // Store keys array reference: let __keys = Object.keys(obj)
            module.init.push(Stmt::Let {
                id: keys_id,
                name: format!("__keys_{}", keys_id),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                init: Some(keys_expr),
            });

            // Lower the body
            let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;

            // Prepend: const key = __keys[__i]
            loop_body.insert(
                0,
                Stmt::Let {
                    id: key_id,
                    name: key_name,
                    ty: Type::String,
                    mutable: false,
                    init: Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(keys_id)),
                        index: Box::new(Expr::LocalGet(idx_id)),
                    }),
                },
            );

            // Create the for loop:
            // for (let __i = 0; __i < __keys.length; __i++) { ... }
            module.init.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(keys_id)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        _ => {}
    }
    Ok(())
}

/// Assign a value to an expression target (used for unwrapped paren/type-assertion targets).
/// Converts an Expr (which should be an ident or member access) into an assignment.
pub(super) fn lower_expr_assignment(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    value: Box<Expr>,
) -> Result<Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else if ctx.lookup_class(&name).is_some() || ctx.lookup_func(&name).is_some() {
                // v0.5.757: don't shadow a class/function binding with an
                // implicit local for `<Name> = X` patterns. Drizzle's
                // sql.js uses `((sql2) => { ... })(sql || (sql = {}))` —
                // the binding exists (truthy), the OR short-circuits, and
                // the assignment is dead. Pre-fix the implicit local hid
                // the original binding from later reads. Just evaluate
                // the RHS for side effects. Refs #420.
                Ok(*value)
            } else {
                eprintln!(
                    "  Warning: Assignment to undeclared variable '{}', creating implicit local",
                    name
                );
                let id = ctx.define_local(name, Type::Any);
                Ok(Expr::LocalSet(id, value))
            }
        }
        ast::Expr::Member(member) => {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: obj_name,
                                field_name,
                                value,
                            });
                        }
                    }
                }
            }
            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    // Issue #711 part 2: `<expr>.prototype = <value>`
                    // pattern (Effect's effectable.ts uses this to
                    // declare prototype-based classes — `function
                    // Base() {}; Base.prototype = CommitPrototype`).
                    // Route through the SetFunctionPrototype HIR node
                    // so codegen calls
                    // `js_set_function_prototype(func, proto)`, which
                    // allocates a synthetic class id keyed by the
                    // function value. The runtime helper is a no-op
                    // when `object` doesn't evaluate to a function
                    // (preserves baseline for legitimate
                    // `someClass.prototype = X` writes on non-function
                    // values).
                    if property == "prototype" {
                        return Ok(Expr::SetFunctionPrototype {
                            func: object,
                            proto: value,
                        });
                    }
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexSet {
                        object,
                        index,
                        value,
                    })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
            }
        }
        // Recursively unwrap parens and type annotations
        ast::Expr::Paren(paren) => lower_expr_assignment(ctx, &paren.expr, value),
        ast::Expr::TsAs(ts_as) => lower_expr_assignment(ctx, &ts_as.expr, value),
        ast::Expr::TsNonNull(ts_nn) => lower_expr_assignment(ctx, &ts_nn.expr, value),
        ast::Expr::TsTypeAssertion(ts_ta) => lower_expr_assignment(ctx, &ts_ta.expr, value),
        ast::Expr::TsSatisfies(ts_sat) => lower_expr_assignment(ctx, &ts_sat.expr, value),
        _ => Err(anyhow!(
            "Unsupported expression as assignment target: {:?}",
            expr
        )),
    }
}

pub(crate) fn lower_expr(ctx: &mut LoweringContext, expr: &ast::Expr) -> Result<Expr> {
    match expr {
        ast::Expr::Lit(lit) => lower_lit(lit),
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalGet(id))
            } else if let Some(id) = ctx.lookup_func(&name) {
                Ok(Expr::FuncRef(id))
            } else if let Some((module_name, method_name)) = ctx.lookup_native_module(&name) {
                // Special handling for worker_threads named imports
                if module_name == "worker_threads" {
                    if let Some(method) = method_name {
                        if method == "workerData" {
                            // workerData is a property-like import that calls a getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "workerData".to_string(),
                                args: Vec::new(),
                            });
                        }
                        if method == "parentPort" {
                            // parentPort is a singleton handle - call getter function
                            return Ok(Expr::NativeMethodCall {
                                module: "worker_threads".to_string(),
                                class_name: None,
                                object: None,
                                method: "parentPort".to_string(),
                                args: Vec::new(),
                            });
                        }
                    }
                }
                // Native module reference (e.g., mysql from 'mysql2/promise')
                Ok(Expr::NativeModuleRef(module_name.to_string()))
            } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
                // Imported function - reference by its original exported name
                // Look up type information if available
                let (param_types, return_type) = ctx
                    .lookup_extern_func_types(orig_name)
                    .map(|(p, r)| (p.clone(), r.clone()))
                    .unwrap_or_else(|| (Vec::new(), Type::Any));
                Ok(Expr::ExternFuncRef {
                    name: orig_name.to_string(),
                    param_types,
                    return_type,
                })
            } else if is_builtin_function(&name) {
                // Built-in global function (setTimeout, etc.)
                Ok(Expr::ExternFuncRef {
                    name,
                    param_types: Vec::new(),
                    return_type: Type::Any,
                })
            } else if ctx.lookup_class(&name).is_some() {
                // Class used as a first-class value (e.g., { Point: Point })
                Ok(Expr::ClassRef(name))
            } else if name == "undefined" {
                // Global undefined identifier
                Ok(Expr::Undefined)
            } else if name == "null" {
                // Global null identifier (though typically written as literal)
                Ok(Expr::Null)
            } else if name == "NaN" {
                // Global NaN identifier
                Ok(Expr::Number(f64::NAN))
            } else if name == "Infinity" {
                // Global Infinity identifier
                Ok(Expr::Number(f64::INFINITY))
            } else if name == "__dirname" || name == "__filename" {
                // Issue #667: CJS-style module locals. Without this fold,
                // the bare reference falls through to GlobalGet(0) -> 0,
                // which silently corrupts any path computation built on
                // path.join(__dirname, ...). Mirrors the import.meta arm
                // (expr_misc::import_meta_paths) so both surfaces agree.
                let path = &ctx.source_file_path;
                let value = if name == "__filename" {
                    path.clone()
                } else {
                    match path.rfind('/') {
                        Some(i) if i > 0 => path[..i].to_string(),
                        Some(_) => "/".to_string(),
                        None => String::new(),
                    }
                };
                Ok(Expr::String(value))
            } else {
                // GlobalGet(0) is a sentinel: codegen routes by name from the
                // parent PropertyGet/Call/Member context. Bare uses lower to
                // 0.0 (perry-codegen/src/expr.rs Expr::GlobalGet arm).
                if name != "console"
                    && name != "process"
                    && name != "globalThis"
                    && name != "Buffer"
                    && name != "Date"
                    && name != "JSON"
                    && name != "Math"
                    && name != "Object"
                    && name != "Array"
                    && name != "String"
                    && name != "Number"
                    && name != "Boolean"
                    && name != "Function"
                    && name != "Error"
                    && name != "TypeError"
                    && name != "RangeError"
                    && name != "Promise"
                    && name != "Map"
                    && name != "Set"
                    && name != "RegExp"
                    && name != "Symbol"
                    && name != "WeakMap"
                    && name != "WeakSet"
                    && name != "WeakRef"
                    && name != "FinalizationRegistry"
                    && name != "Proxy"
                    && name != "Reflect"
                    && name != "Uint8Array"
                    && name != "Int8Array"
                    && name != "Int16Array"
                    && name != "Uint16Array"
                    && name != "Int32Array"
                    && name != "Uint32Array"
                    && name != "Float32Array"
                    && name != "Float64Array"
                    && name != "TextEncoder"
                    && name != "TextDecoder"
                    && name != "URL"
                    && name != "URLSearchParams"
                    && name != "AbortController"
                    && name != "FormData"
                    && name != "Headers"
                    && name != "fetch"
                    && name != "crypto"
                    && name != "performance"
                    && name != "queueMicrotask"
                    && name != "structuredClone"
                    && name != "atob"
                    && name != "btoa"
                    && name != "BigInt"
                    && name != "WebAssembly"
                {
                    eprintln!(
                        "  Warning: unknown identifier '{}' — assuming global; member access will dispatch by name at runtime, bare reads lower to 0",
                        name
                    );
                }
                // Bare built-in constructor identifiers (`Date`, `Array`,
                // `Object`, ...) used as VALUES (not method receivers /
                // `new` callees) need a real closure pointer so identity
                // comparisons like `inst.constructor === Date` hold —
                // both sides must resolve to the same `populate_global_this_builtins`-
                // installed closure. Reuse the existing
                // `PropertyGet { GlobalGet, <name> }` codegen path that
                // dispatches through `js_get_global_this` for builtin
                // names. Bare-callee shapes (e.g. `Date.now()`, `new
                // Date()`) are picked off earlier by their dedicated HIR
                // variants — `Expr::DateNow`, `Expr::DateNew(...)`,
                // `Expr::Date*Get(...)` — so they don't reach this arm.
                // date-fns / drizzle / lodash duck-typing path.
                if is_builtin_global_value_name(&name) {
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::GlobalGet(0)),
                        property: name,
                    });
                }
                Ok(Expr::GlobalGet(0))
            }
        }
        ast::Expr::Bin(bin) => {
            // Handle 'in' operator: property in object
            if matches!(bin.op, ast::BinaryOp::In) {
                // Proxy fast path: `key in proxy` routes through js_proxy_has.
                if let ast::Expr::Ident(obj_ident) = bin.right.as_ref() {
                    let obj_name = obj_ident.sym.to_string();
                    if ctx.proxy_locals.contains(&obj_name) {
                        let key = Box::new(lower_expr(ctx, &bin.left)?);
                        let proxy = Box::new(lower_expr(ctx, &bin.right)?);
                        return Ok(Expr::ProxyHas { proxy, key });
                    }
                }
                let property = Box::new(lower_expr(ctx, &bin.left)?);
                let object = Box::new(lower_expr(ctx, &bin.right)?);
                return Ok(Expr::In { property, object });
            }

            // Handle instanceof specially - needs to extract class name
            if matches!(bin.op, ast::BinaryOp::InstanceOf) {
                // WeakRef / FinalizationRegistry: Perry doesn't register a runtime class id,
                // so generic InstanceOf would always return false. Pre-scan tracks bindings
                // explicitly, so `local instanceof WeakRef|FinalizationRegistry` can be folded
                // at lowering time when we recognise the receiver.
                if let ast::Expr::Ident(class_ident) = bin.right.as_ref() {
                    let class_name = class_ident.sym.as_ref();
                    if class_name == "WeakRef" || class_name == "FinalizationRegistry" {
                        if let ast::Expr::Ident(left_ident) = bin.left.as_ref() {
                            let local_name = left_ident.sym.to_string();
                            let is_match = (class_name == "WeakRef"
                                && ctx.weakref_locals.contains(&local_name))
                                || (class_name == "FinalizationRegistry"
                                    && ctx.finreg_locals.contains(&local_name));
                            return Ok(Expr::Bool(is_match));
                        }
                    }
                }
                let expr = Box::new(lower_expr(ctx, &bin.left)?);
                // Right side can be an identifier (ClassName) or member expression (Module.ClassName)
                let ty = match bin.right.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    ast::Expr::Member(member) => {
                        // Handle Module.ClassName - extract the full qualified name
                        let obj_name = if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            obj_ident.sym.to_string()
                        } else {
                            "Unknown".to_string()
                        };
                        let prop_name = match &member.prop {
                            ast::MemberProp::Ident(prop_ident) => prop_ident.sym.to_string(),
                            _ => "Unknown".to_string(),
                        };
                        format!("{}.{}", obj_name, prop_name)
                    }
                    _ => {
                        // For complex expressions, use a generic type name
                        "Object".to_string()
                    }
                };
                // v0.5.749: when the right side resolves to a local
                // variable holding a class ref (e.g. `function is(value,
                // type) { return value instanceof type; }`), emit a
                // dynamic-dispatch path that evaluates the class ref at
                // runtime. Without this, the codegen sees `ty = "type"`
                // (the param name), can't resolve it as a class, and
                // falls through to `class_id = 0` — every dynamic
                // instanceof returns false. Drizzle's `is(value, type)`
                // chain depends on this. Refs #420 / #618 followup.
                let ty_expr = if let ast::Expr::Ident(ident) = bin.right.as_ref() {
                    let name = ident.sym.as_ref();
                    if ctx.lookup_local(name).is_some() {
                        match lower_expr(ctx, &bin.right) {
                            Ok(e) => Some(Box::new(e)),
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                return Ok(Expr::InstanceOf { expr, ty, ty_expr });
            }

            let left = Box::new(lower_expr(ctx, &bin.left)?);
            let right = Box::new(lower_expr(ctx, &bin.right)?);

            match bin.op {
                // Arithmetic
                ast::BinaryOp::Add => Ok(Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                }),
                ast::BinaryOp::Sub => Ok(Expr::Binary {
                    op: BinaryOp::Sub,
                    left,
                    right,
                }),
                ast::BinaryOp::Mul => Ok(Expr::Binary {
                    op: BinaryOp::Mul,
                    left,
                    right,
                }),
                ast::BinaryOp::Div => Ok(Expr::Binary {
                    op: BinaryOp::Div,
                    left,
                    right,
                }),
                ast::BinaryOp::Mod => Ok(Expr::Binary {
                    op: BinaryOp::Mod,
                    left,
                    right,
                }),
                ast::BinaryOp::Exp => Ok(Expr::Binary {
                    op: BinaryOp::Pow,
                    left,
                    right,
                }),

                // Comparison (treat == same as === for typed code)
                ast::BinaryOp::EqEq => {
                    // Proxy/Reflect fold: `Reflect.getPrototypeOf(x) === <Class>.prototype`
                    // always true in our model (we don't maintain real prototypes).
                    // Same fold for `Object.getPrototypeOf(x) === <Class>.prototype`.
                    if matches!(
                        &*left,
                        Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
                    ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
                    {
                        return Ok(Expr::Bool(true));
                    }
                    Ok(Expr::Compare {
                        op: CompareOp::LooseEq,
                        left,
                        right,
                    })
                }
                ast::BinaryOp::EqEqEq => {
                    if matches!(
                        &*left,
                        Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
                    ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
                    {
                        return Ok(Expr::Bool(true));
                    }
                    Ok(Expr::Compare {
                        op: CompareOp::Eq,
                        left,
                        right,
                    })
                }
                ast::BinaryOp::NotEq => Ok(Expr::Compare {
                    op: CompareOp::LooseNe,
                    left,
                    right,
                }),
                ast::BinaryOp::NotEqEq => Ok(Expr::Compare {
                    op: CompareOp::Ne,
                    left,
                    right,
                }),
                ast::BinaryOp::Lt => Ok(Expr::Compare {
                    op: CompareOp::Lt,
                    left,
                    right,
                }),
                ast::BinaryOp::LtEq => Ok(Expr::Compare {
                    op: CompareOp::Le,
                    left,
                    right,
                }),
                ast::BinaryOp::Gt => Ok(Expr::Compare {
                    op: CompareOp::Gt,
                    left,
                    right,
                }),
                ast::BinaryOp::GtEq => Ok(Expr::Compare {
                    op: CompareOp::Ge,
                    left,
                    right,
                }),

                // Logical
                ast::BinaryOp::LogicalAnd => Ok(Expr::Logical {
                    op: LogicalOp::And,
                    left,
                    right,
                }),
                ast::BinaryOp::LogicalOr => Ok(Expr::Logical {
                    op: LogicalOp::Or,
                    left,
                    right,
                }),
                ast::BinaryOp::NullishCoalescing => Ok(Expr::Logical {
                    op: LogicalOp::Coalesce,
                    left,
                    right,
                }),

                // Bitwise
                ast::BinaryOp::BitAnd => Ok(Expr::Binary {
                    op: BinaryOp::BitAnd,
                    left,
                    right,
                }),
                ast::BinaryOp::BitOr => Ok(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left,
                    right,
                }),
                ast::BinaryOp::BitXor => Ok(Expr::Binary {
                    op: BinaryOp::BitXor,
                    left,
                    right,
                }),
                ast::BinaryOp::LShift => Ok(Expr::Binary {
                    op: BinaryOp::Shl,
                    left,
                    right,
                }),
                ast::BinaryOp::RShift => Ok(Expr::Binary {
                    op: BinaryOp::Shr,
                    left,
                    right,
                }),
                ast::BinaryOp::ZeroFillRShift => Ok(Expr::Binary {
                    op: BinaryOp::UShr,
                    left,
                    right,
                }),

                _ => Err(anyhow!("Unsupported binary operator: {:?}", bin.op)),
            }
        }
        ast::Expr::Unary(unary) => {
            // AST-level typeof fold for `typeof Object.<known>` /
            // `typeof Array.<known>`. Lowering the operand would yield a
            // generic property-get on the global Object/Array (which
            // currently returns 0/undefined and makes `=== "function"`
            // checks fail). The static methods are real functions in
            // Node, so fold to the literal "function" string here.
            if matches!(unary.op, ast::UnaryOp::TypeOf) {
                // #677: bare `typeof Function` — Function is a JS built-in
                // constructor, so typeof is "function". Without this fold,
                // the bare ident lowers to `GlobalGet(0)` and typeof reads
                // "object" via the global-this short-circuit.
                if let ast::Expr::Ident(id) = unary.arg.as_ref() {
                    if id.sym.as_ref() == "Function" && ctx.lookup_local("Function").is_none() {
                        return Ok(Expr::String("function".to_string()));
                    }
                }
                if let ast::Expr::Member(member) = unary.arg.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let obj_name = obj_ident.sym.as_ref();
                            let prop_name = prop_ident.sym.as_ref();
                            if (obj_name == "Object" && is_known_object_static_method(prop_name))
                                || (obj_name == "Array" && is_known_array_static_method(prop_name))
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("async_hooks", "AsyncHook"))
                            ) && matches!(prop_name, "enable" | "disable")
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                            if matches!(
                                ctx.lookup_native_instance(obj_name),
                                Some(("async_hooks", "AsyncResource"))
                            ) && matches!(
                                prop_name,
                                "asyncId"
                                    | "triggerAsyncId"
                                    | "runInAsyncScope"
                                    | "emitDestroy"
                                    | "bind"
                            ) {
                                return Ok(Expr::String("function".to_string()));
                            }
                            // #677: `typeof Function.prototype` → "object".
                            // `Function.prototype` is the (immutable) prototype
                            // chain root for all functions; in Node typeof is
                            // "object". Other `Function.<X>` reads (`Function.name`,
                            // etc.) fall through to GlobalGet member-access,
                            // which today returns `undefined`.
                            if obj_name == "Function"
                                && prop_name == "prototype"
                                && ctx.lookup_local("Function").is_none()
                            {
                                return Ok(Expr::String("object".to_string()));
                            }
                        }
                    }
                    // `typeof "".methodName === "function"` — feature
                    // detection idiom. Generic PropertyGet on a string
                    // literal returns undefined in Perry today, so the
                    // typeof would be "undefined" and the test branch
                    // gets skipped. Fold to "function" when the property
                    // name is a known String.prototype method that the
                    // runtime actually dispatches.
                    if let (ast::Expr::Lit(ast::Lit::Str(_)), ast::MemberProp::Ident(prop_ident)) =
                        (member.obj.as_ref(), &member.prop)
                    {
                        let prop_name = prop_ident.sym.as_ref();
                        if is_known_string_prototype_method(prop_name) {
                            return Ok(Expr::String("function".to_string()));
                        }
                    }
                }
            }
            let operand = Box::new(lower_expr(ctx, &unary.arg)?);
            match unary.op {
                ast::UnaryOp::Minus => {
                    // Fold -Number into Number(-val) to simplify codegen
                    // (e.g., array literals with negative numbers avoid Unary wrapper)
                    if let Expr::Number(val) = *operand {
                        Ok(Expr::Number(-val))
                    } else if let Expr::Integer(val) = *operand {
                        // Special case: -0 must be preserved as -0.0 (negative zero)
                        // because integers collapse +0 and -0 into the same bit pattern.
                        // JS distinguishes these in `console.log`, `Object.is`, and
                        // `1/x` — so fold to Number(-0.0) instead of Integer(0).
                        if val == 0 {
                            Ok(Expr::Number(-0.0))
                        } else {
                            Ok(Expr::Integer(-val))
                        }
                    } else {
                        Ok(Expr::Unary {
                            op: UnaryOp::Neg,
                            operand,
                        })
                    }
                }
                ast::UnaryOp::Plus => Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    operand,
                }),
                ast::UnaryOp::Bang => Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    operand,
                }),
                ast::UnaryOp::Tilde => Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    operand,
                }),
                ast::UnaryOp::TypeOf => {
                    // Fast path: known Symbol-producing expressions resolve to "symbol"
                    // at compile time (avoids needing runtime js_value_typeof to
                    // recognize the SymbolHeader magic).
                    if matches!(&*operand, Expr::SymbolNew(_) | Expr::SymbolFor(_)) {
                        return Ok(Expr::String("symbol".to_string()));
                    }
                    Ok(Expr::TypeOf(operand))
                }
                ast::UnaryOp::Delete => {
                    // Proxy delete: rewrite `delete proxy.key` as ProxyDelete.
                    if let Expr::ProxyGet { proxy, key } = &*operand {
                        return Ok(Expr::ProxyDelete {
                            proxy: proxy.clone(),
                            key: key.clone(),
                        });
                    }
                    Ok(Expr::Delete(operand))
                }
                ast::UnaryOp::Void => Ok(Expr::Void(operand)),
                // #853: `ast::UnaryOp` is `#[non_exhaustive]` upstream — keep
                // this catch-all as a forward-compat safety net.
                #[allow(unreachable_patterns)]
                _ => Err(anyhow!("Unsupported unary operator: {:?}", unary.op)),
            }
        }
        ast::Expr::Call(call) => expr_call::lower_call(ctx, call),
        ast::Expr::Member(member) => expr_member::lower_member(ctx, member),
        ast::Expr::Paren(paren) => lower_expr(ctx, &paren.expr),
        ast::Expr::Assign(assign) => expr_assign::lower_assign(ctx, assign),
        ast::Expr::Cond(cond) => expr_misc::lower_cond(ctx, cond),
        ast::Expr::Array(array) => {
            // Check if any elements are spread elements
            let has_spread = array
                .elems
                .iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());

            if has_spread {
                // Use ArraySpread for arrays with spread elements.
                // If a spread source is a generator call, wrap it in IteratorToArray
                // so the codegen gets a real array to iterate.
                let elements = array
                    .elems
                    .iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| {
                        let expr = lower_expr(ctx, &elem.expr)?;
                        if elem.spread.is_some() {
                            // Wrap generator calls in IteratorToArray
                            if is_generator_call_expr(ctx, &expr) {
                                Ok(ArrayElement::Spread(Expr::IteratorToArray(Box::new(expr))))
                            } else {
                                Ok(ArrayElement::Spread(expr))
                            }
                        } else {
                            Ok(ArrayElement::Expr(expr))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::ArraySpread(elements))
            } else {
                // No spread elements, use regular Array
                let elements = array
                    .elems
                    .iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| lower_expr(ctx, &elem.expr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => expr_object::lower_object(ctx, obj),
        ast::Expr::This(_) => {
            // Always use Expr::This - the codegen will handle it with ThisContext
            Ok(Expr::This)
        }
        ast::Expr::New(new_expr) => expr_new::lower_new(ctx, new_expr),
        ast::Expr::Arrow(arrow) => expr_function::lower_arrow(ctx, arrow),
        ast::Expr::Fn(fn_expr) => expr_function::lower_fn_expr(ctx, fn_expr),
        ast::Expr::Await(await_expr) => expr_misc::lower_await(ctx, await_expr),
        ast::Expr::SuperProp(super_prop) => expr_misc::lower_super_prop(ctx, super_prop),
        ast::Expr::Update(update) => expr_misc::lower_update(ctx, update),
        ast::Expr::Tpl(tpl) => expr_misc::lower_tpl(ctx, tpl),
        ast::Expr::OptChain(opt_chain) => {
            // Optional chaining: obj?.prop or obj?.[index] or obj?.method()
            // Convert to: obj == null ? undefined : obj.prop
            match &*opt_chain.base {
                ast::OptChainBase::Member(member) => {
                    // Issue #449: `new.target?.<prop>` folds to a literal at
                    // lowering time — same shape as the direct
                    // `new.target.<prop>` fold in `expr_member::lower_member`,
                    // applied here BEFORE `lower_expr(&member.obj)` would
                    // otherwise route MetaProp(NewTarget) through the
                    // broken Object-literal synthesis path. Inside a
                    // constructor `new.target` is non-null/non-undefined,
                    // so the optional chain just resolves the property;
                    // outside a constructor it's undefined and the chain
                    // short-circuits.
                    if let ast::Expr::MetaProp(mp) = member.obj.as_ref() {
                        if matches!(mp.kind, ast::MetaPropKind::NewTarget) {
                            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                let prop_name = prop_ident.sym.as_ref();
                                if let Some(class_name) = ctx.in_constructor_class.clone() {
                                    return Ok(match prop_name {
                                        "name" => Expr::String(class_name),
                                        _ => Expr::Undefined,
                                    });
                                }
                                return Ok(Expr::Undefined);
                            }
                        }
                    }
                    // obj?.prop -> obj == null ? undefined : obj.prop
                    let obj_expr = lower_expr(ctx, &member.obj)?;

                    // Get the property access
                    let prop_expr = match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let prop_name = ident.sym.to_string();
                            // Mirror the eager-fold from
                            // `expr_member::lower_member` for `m.index` /
                            // `m.groups` when `m` is a tracked
                            // regex.exec()/string.match() result. The
                            // standard `lower_member` fires when the
                            // AST is `m.groups`; for the optional-chain
                            // form `m?.groups` SWC routes here, the
                            // `member.obj` has already been lowered to
                            // `LocalGet(...)`, so `lower_member`'s
                            // `ast::Expr::Ident` check fails. Intercept
                            // here so the optional-chain shape also
                            // resolves to the thread-local groups
                            // object instead of a generic property
                            // read that returns undefined.
                            if (prop_name == "groups" || prop_name == "index")
                                && match member.obj.as_ref() {
                                    ast::Expr::Ident(ident) => {
                                        ctx.regex_exec_locals.contains(&ident.sym.to_string())
                                    }
                                    ast::Expr::TsNonNull(nn) => match nn.expr.as_ref() {
                                        ast::Expr::Ident(ident) => {
                                            ctx.regex_exec_locals.contains(&ident.sym.to_string())
                                        }
                                        _ => false,
                                    },
                                    _ => false,
                                }
                            {
                                if prop_name == "groups" {
                                    Expr::RegExpExecGroups
                                } else {
                                    Expr::RegExpExecIndex
                                }
                            } else {
                                Expr::PropertyGet {
                                    object: Box::new(obj_expr.clone()),
                                    property: prop_name,
                                }
                            }
                        }
                        ast::MemberProp::Computed(comp) => {
                            let index = lower_expr(ctx, &comp.expr)?;
                            Expr::IndexGet {
                                object: Box::new(obj_expr.clone()),
                                index: Box::new(index),
                            }
                        }
                        _ => return Err(anyhow!("Unsupported optional chain property type")),
                    };

                    // Issue #388: optional chaining short-circuits on
                    // null OR undefined per spec. Use `LooseEq` so the
                    // comparison `obj == null` matches both — strict
                    // `===` only matches null, leaving undefined to
                    // fall through and dereference (returning
                    // `[object Object]` for Map.get's missing value).
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(obj_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(prop_expr),
                    })
                }
                ast::OptChainBase::Call(call) => {
                    // OptChain(Call) is `<expr>?.(args)` — the `?.` is between the
                    // callee and the call parens (e.g. `obj.method?.(args)`), NOT
                    // `obj?.method(args)` (which SWC parses as Call(OptChain(Member))
                    // and is handled via the regular Call lowering path).
                    //
                    // So the short-circuit must check the *function value* (the
                    // callee), not the receiver. Issue #830: previously this
                    // checked `obj == null`, which crashed when `obj.method` was
                    // undefined while `obj` itself was a valid object.
                    let callee = &call.callee;

                    // Check for spread arguments
                    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

                    let args = call
                        .args
                        .iter()
                        .map(|arg| lower_expr(ctx, &arg.expr))
                        .collect::<Result<Vec<_>>>()?;

                    // Lower callee as plain MemberExpr, unwrapping inner OptChain.
                    // SWC may wrap the callee member access in an OptChain too.
                    // We must NOT re-lower via lower_expr which would nest Conditionals.
                    let (check_expr, callee_expr) = {
                        let mut lower_member_flat =
                            |member: &ast::MemberExpr| -> Result<(Expr, Expr)> {
                                let obj = lower_expr(ctx, &member.obj)?;
                                let prop = match &member.prop {
                                    ast::MemberProp::Ident(id) => Expr::PropertyGet {
                                        object: Box::new(obj.clone()),
                                        property: id.sym.to_string(),
                                    },
                                    ast::MemberProp::Computed(c) => {
                                        let idx = lower_expr(ctx, &c.expr)?;
                                        Expr::IndexGet {
                                            object: Box::new(obj.clone()),
                                            index: Box::new(idx),
                                        }
                                    }
                                    _ => return Err(anyhow!("Unsupported optional chain member")),
                                };
                                Ok((obj, prop))
                            };
                        match &**callee {
                            // Simple `obj.method?.(args)`: check the function value
                            // (prop), call the function (prop) — codegen still sees
                            // a PropertyGet callee so `this` binds to obj.
                            ast::Expr::Member(m) => {
                                let (_obj, prop) = lower_member_flat(m)?;
                                (prop.clone(), prop)
                            }
                            ast::Expr::OptChain(inner) => match &*inner.base {
                                // Chained `foo?.bar?.(args)`: keep checking the
                                // receiver (foo) so the inner `?.` short-circuit
                                // still works. A separate null-check on the
                                // function value (foo.bar) is a known gap — see
                                // the comment above the final Conditional below.
                                ast::OptChainBase::Member(m) => lower_member_flat(m)?,
                                _ => {
                                    let ce = lower_expr(ctx, callee)?;
                                    (ce.clone(), ce)
                                }
                            },
                            _ => {
                                let ce = lower_expr(ctx, callee)?;
                                (ce.clone(), ce)
                            }
                        }
                    };

                    // If check_expr is already a Conditional from an inner optional chain,
                    // nest the outer call inside its else branch instead of creating another Conditional.
                    // This avoids duplicating side-effecting expressions (like ArrayShift/ArrayPop).
                    if let Expr::Conditional {
                        condition: inner_cond,
                        then_expr: inner_then,
                        else_expr: inner_else,
                    } = check_expr
                    {
                        // Build the callee with inner_else as the object (not the full Conditional)
                        let fixed_callee = match callee_expr {
                            Expr::PropertyGet { property, .. } => Expr::PropertyGet {
                                object: inner_else,
                                property,
                            },
                            Expr::IndexGet { index, .. } => Expr::IndexGet {
                                object: inner_else,
                                index,
                            },
                            other => other,
                        };
                        let outer_call = Expr::Call {
                            callee: Box::new(fixed_callee),
                            args,
                            type_args: Vec::new(),
                        };
                        return Ok(Expr::Conditional {
                            condition: inner_cond,
                            then_expr: inner_then,
                            else_expr: Box::new(outer_call),
                        });
                    }

                    // Build the call expression
                    let call_expr = if has_spread {
                        let spread_args: Vec<CallArg> = call
                            .args
                            .iter()
                            .zip(args.iter())
                            .map(|(ast_arg, lowered)| {
                                if ast_arg.spread.is_some() {
                                    CallArg::Spread(lowered.clone())
                                } else {
                                    CallArg::Expr(lowered.clone())
                                }
                            })
                            .collect();
                        Expr::CallSpread {
                            callee: Box::new(callee_expr),
                            args: spread_args,
                            type_args: Vec::new(),
                        }
                    } else {
                        // Try to fold known array methods (`.map`/`.filter`/etc.)
                        // into their dedicated HIR variants here, since the regular
                        // `lower_expr` Call array fast-path is on the AST CallExpr
                        // path and never sees the synthetic Expr::Call we build
                        // for `obj?.method(args)`.
                        try_fold_array_method_call(Expr::Call {
                            callee: Box::new(callee_expr),
                            args,
                            type_args: Vec::new(),
                        })
                    };

                    // Issue #388: optional chaining short-circuits on
                    // null OR undefined per spec. Use `LooseEq` so the
                    // comparison `check_expr == null` matches both —
                    // strict `===` only matches null, leaving
                    // undefined to fall through and produce
                    // `[object Object]` (or worse) when the receiver
                    // is `Map.get(missing)` etc.
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(check_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(call_expr),
                    })
                }
            }
        }
        ast::Expr::TsAs(ts_as) => {
            // TypeScript 'as' type assertion - at runtime, just evaluate the expression
            // The type assertion is compile-time only
            lower_expr(ctx, &ts_as.expr)
        }
        ast::Expr::TsNonNull(ts_non_null) => {
            // TypeScript non-null assertion (value!) - at runtime, just the expression
            lower_expr(ctx, &ts_non_null.expr)
        }
        ast::Expr::TsTypeAssertion(ts_assertion) => {
            // TypeScript angle-bracket type assertion (<Type>value) - same as 'as', compile-time only
            lower_expr(ctx, &ts_assertion.expr)
        }
        ast::Expr::TsConstAssertion(ts_const) => {
            // TypeScript 'as const' assertion - at runtime, just evaluate the expression
            // The const assertion only affects type inference, not runtime behavior
            lower_expr(ctx, &ts_const.expr)
        }
        ast::Expr::TsSatisfies(ts_satisfies) => {
            // TypeScript 'satisfies' operator - compile-time type check only
            lower_expr(ctx, &ts_satisfies.expr)
        }
        ast::Expr::TsInstantiation(ts_inst) => {
            // TypeScript generic instantiation (func<Type>) - at runtime, just the expression
            lower_expr(ctx, &ts_inst.expr)
        }
        ast::Expr::Seq(seq) => expr_misc::lower_seq(ctx, seq),
        ast::Expr::MetaProp(meta_prop) => expr_misc::lower_meta_prop(ctx, meta_prop),
        ast::Expr::Yield(y) => expr_misc::lower_yield(ctx, y),
        ast::Expr::TaggedTpl(tagged) => {
            // Tagged template literals: tag`Hello ${name},${42}!`
            // Two cases:
            //  (a) String.raw — kept as a fast-path string concatenation that
            //      preserves backslashes literally (no escape processing).
            //  (b) Any other tag function — desugar to a regular function call:
            //      tag(["Hello ", ",", "!"], name, 42)
            //      i.e. first arg is the array of cooked string literal parts,
            //      followed by each interpolated value as its own argument.
            //      The matches the JS spec for `tag` callbacks (sans `.raw`).
            let is_string_raw = match &*tagged.tag {
                ast::Expr::Member(member) => {
                    let obj_is_string = match &member.obj.as_ref() {
                        ast::Expr::Ident(id) => id.sym.as_ref() == "String",
                        _ => false,
                    };
                    let prop_is_raw = match &member.prop {
                        ast::MemberProp::Ident(id) => id.sym.as_ref() == "raw",
                        _ => false,
                    };
                    obj_is_string && prop_is_raw
                }
                _ => false,
            };

            let tpl = &*tagged.tpl;
            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            if is_string_raw {
                // Fast path: build string via direct concatenation using `raw` text
                let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
                let mut result = Expr::String(first_raw.to_string());

                for (i, expr) in tpl.exprs.iter().enumerate() {
                    let lowered = lower_expr(ctx, expr)?;
                    result = Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(result),
                        right: Box::new(lowered),
                    };

                    if let Some(quasi) = tpl.quasis.get(i + 1) {
                        let quasi_str: &str = quasi.raw.as_ref();
                        if !quasi_str.is_empty() {
                            result = Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(result),
                                right: Box::new(Expr::String(quasi_str.to_string())),
                            };
                        }
                    }
                }

                return Ok(result);
            }

            // General case: desugar to `tag(stringsArray, ...exprs)`. The
            // strings array carries the cooked text (escapes processed) AS
            // the array elements AND the raw text (escapes preserved) via
            // a thread-local side table populated at the call site —
            // `TaggedTemplateStrings` codegen emits both arrays + a
            // `js_tagged_template_register_raw` call so `strings.raw` reads
            // can resolve via the matching `Expr::TemplateRaw` fold below.
            let cooked_strings: Vec<Expr> = tpl
                .quasis
                .iter()
                .map(|q| {
                    let cooked_owned: Option<String> = q
                        .cooked
                        .as_ref()
                        .and_then(|c| c.as_str().map(|s| s.to_string()));
                    let s = cooked_owned.unwrap_or_else(|| q.raw.as_ref().to_string());
                    Expr::String(s)
                })
                .collect();
            let raw_strings: Vec<String> = tpl
                .quasis
                .iter()
                .map(|q| q.raw.as_ref().to_string())
                .collect();
            let strings_array = Expr::TaggedTemplateStrings {
                cooked: cooked_strings,
                raw: raw_strings,
            };

            let mut call_args: Vec<Expr> = Vec::with_capacity(tpl.exprs.len() + 1);
            call_args.push(strings_array);
            for e in &tpl.exprs {
                call_args.push(lower_expr(ctx, e)?);
            }

            let callee = lower_expr(ctx, &tagged.tag)?;
            Ok(Expr::Call {
                callee: Box::new(callee),
                args: call_args,
                type_args: vec![],
            })
        }
        // Class expression used as a value (not in `new` context) —
        // refs #740. JS semantics: a class expression evaluates to the
        // class constructor itself. Previously we emitted an empty `new`
        // here, which bound the local to a zero-arg instance instead of
        // the class — so `const C = class { ... }; new C(args)` ran the
        // ctor with no args, and `O.Inner` inside an object literal held
        // a stillborn instance instead of a constructor. Lower to a
        // `ClassRef` so the constructor identity survives the value path
        // and `new` site rerouting (via `local_class_aliases`) picks it
        // back up.
        ast::Expr::Class(class_expr) => {
            let ident_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
            let synthetic_name =
                ident_name.unwrap_or_else(|| format!("__anon_class_{}", ctx.fresh_class()));
            let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
            // Mixin factories like `function WithA(B) { return class extends B {} }`
            // produce a class whose super is the function-parameter `B` — a
            // runtime value, not a statically-known class. The class-decl arm
            // at the top of this file only pushes a `RegisterClassParentDynamic`
            // statement for top-level class declarations; an anonymous class
            // expression inside a function body never has that side effect
            // fire, so `new (class extends WithA(Base) {})().baseMethod()`
            // walks subclass → inner factory class and stops at the unwired
            // grandparent edge (TypeError on the inherited method). Sequence
            // the dynamic-parent registration in front of the ClassRef so the
            // edge is wired every time the factory function executes; the
            // Sequence yields its last element, so the value remains the
            // ClassRef the call site expects.
            let parent_expr = class.extends_expr.clone();
            // Issue #894: collect computed-Symbol-key static fields so
            // codegen emits a `RegisterClassStaticSymbol` registration
            // sequenced in front of the ClassRef. Without this, the
            // registration happens at module init via
            // `init_static_fields_late` — but the values referenced by
            // the key/init may not be valid yet (the factory hasn't been
            // called, so any function-local captures are zero) or the
            // class lookup may happen BEFORE module init's late phase
            // (within the same module's top-level expressions). Effect's
            // `make()` factory's `static [TypeId] = variance` is the
            // canonical case: `isSchema(C)` was called from Schema.ts's
            // own top-level `class extends transform(...)` chains, which
            // run before the module's `init_static_fields_late`.
            let static_symbol_registrations: Vec<(Expr, Expr)> = class
                .static_fields
                .iter()
                .filter_map(|sf| match (sf.key_expr.as_ref(), sf.init.as_ref()) {
                    (Some(k), Some(v)) => Some((k.clone(), v.clone())),
                    _ => None,
                })
                .collect();
            ctx.pending_classes.push(class);
            let mut seq: Vec<Expr> = Vec::new();
            if let Some(p) = parent_expr {
                seq.push(Expr::RegisterClassParentDynamic {
                    class_name: synthetic_name.clone(),
                    parent_expr: p,
                });
            }
            for (k, v) in static_symbol_registrations {
                seq.push(Expr::RegisterClassStaticSymbol {
                    class_name: synthetic_name.clone(),
                    key_expr: Box::new(k),
                    value_expr: Box::new(v),
                });
            }
            if seq.is_empty() {
                Ok(Expr::ClassRef(synthetic_name))
            } else {
                seq.push(Expr::ClassRef(synthetic_name));
                Ok(Expr::Sequence(seq))
            }
        }
        ast::Expr::JSXElement(jsx) => lower_jsx_element(ctx, jsx),
        ast::Expr::JSXFragment(jsx) => lower_jsx_fragment(ctx, jsx),
        _ => Err(anyhow!("Unsupported expression type: {:?}", expr)),
    }
}

/// Unescape template literal strings (handle \n, \t, etc.)
fn _unescape_template() {}

/// Lower a template literal AST node to its desugared string-concat HIR
/// expression: `\`pre${x}post\`` → `Expr::Binary(Add, "pre", x) + "post"`.
/// Mirrors the inline Tpl lowering at `ast::Expr::Tpl` — extracted so the
/// reactive-Text desugaring can re-lower the same template twice (once for
/// the initial widget value, once inside the rebuild closure).
fn lower_tpl_to_concat(ctx: &mut LoweringContext, tpl: &ast::Tpl) -> Result<Expr> {
    if tpl.quasis.is_empty() {
        return Ok(Expr::String(String::new()));
    }
    let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
    let mut result = Expr::String(unescape_template(first_raw));
    for (i, expr) in tpl.exprs.iter().enumerate() {
        let lowered = lower_expr(ctx, expr)?;
        result = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(lowered),
        };
        if let Some(quasi) = tpl.quasis.get(i + 1) {
            let quasi_str: &str = quasi.raw.as_ref();
            if !quasi_str.is_empty() {
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(Expr::String(unescape_template(quasi_str))),
                };
            }
        }
    }
    Ok(result)
}

/// If `call` matches `Text(\`...${state.value}...\`)` with at least one State
/// interpolation, desugar into an auto-reactive binding. Returns `Ok(None)`
/// for anything else so the generic Call lowering runs.
///
/// The promise (docs/src/ui/state.md): *"Perry detects `state.value` reads
/// inside template literals and creates reactive bindings."* Prior to this,
/// the detection existed nowhere and `count.set(...)` didn't update the
/// rendered label on any platform — most visibly on web/wasm (issue #104)
/// where users ran the counter example and saw static text.
///
/// Generated HIR shape:
/// ```text
/// Sequence([
///   LocalSet(__h, Text(initial_concat)),
///   stateOnChange(state1, closure((_v) -> textSetString(__h, fresh_concat))),
///   stateOnChange(state2, closure((_v) -> textSetString(__h, fresh_concat))),
///   ...,
///   LocalGet(__h),
/// ])
/// ```
///
/// The concat is re-lowered for each closure so each subscriber reads every
/// state freshly — correct for `Text(\`${a.value} and ${b.value}\`)` where a
/// change to `a` still needs the current value of `b`.
pub(super) fn try_desugar_reactive_text(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    // Callee must be the bare identifier `Text`.
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    if ident.sym.as_ref() != "Text" {
        return Ok(None);
    }
    // `Text` must resolve to `perry/ui`'s Text import. Rejects a user-defined
    // `function Text(...)` or an import from another module.
    match ctx.lookup_native_module("Text") {
        Some(("perry/ui", Some(m))) if m == "Text" => {}
        _ => return Ok(None),
    }
    // Only the 1-arg positional form. Spread or additional config args fall
    // through — avoids clobbering setter-chained call forms that we haven't
    // proven we can reproduce bit-for-bit.
    if call.args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    if call.args.len() != 1 {
        return Ok(None);
    }
    let ast::Expr::Tpl(tpl) = call.args[0].expr.as_ref() else {
        return Ok(None);
    };

    // Collect unique `<ident>.value` interpolations where `<ident>` is a
    // State binding. De-dup by name so two references to the same state
    // only register one subscriber.
    let mut state_names: Vec<String> = Vec::new();
    for expr in tpl.exprs.iter() {
        let ast::Expr::Member(member) = expr.as_ref() else {
            continue;
        };
        let ast::MemberProp::Ident(prop) = &member.prop else {
            continue;
        };
        if prop.sym.as_ref() != "value" {
            continue;
        }
        let ast::Expr::Ident(obj_ident) = member.obj.as_ref() else {
            continue;
        };
        let name = obj_ident.sym.to_string();
        let is_state = matches!(
            ctx.lookup_native_instance(&name),
            Some(("perry/ui", "State"))
        );
        if is_state && !state_names.contains(&name) {
            state_names.push(name);
        }
    }
    if state_names.is_empty() {
        return Ok(None);
    }

    // Emit as an IIFE closure so the widget handle can be a *real* function
    // local (backed by a WASM local or LLVM alloca) rather than a bare LocalId
    // floating inside an Expr::Sequence. The WASM backend only registers
    // locals via `Stmt::Let`; a LocalSet/LocalGet pair with no backing Let
    // falls through to TAG_UNDEFINED at read time, which silently drops the
    // widget from its parent container.
    //
    //   (() => {
    //     const __h = Text(concat);
    //     stateOnChange(state1, (__v) => textSetString(__h, concat));
    //     ...
    //     return __h;
    //   })()
    let outer_func_id = ctx.fresh_func();
    let outer_scope = ctx.enter_scope();
    let widget_id = ctx.define_local("__perry_reactive_text_h".to_string(), Type::Any);

    let initial_concat = lower_tpl_to_concat(ctx, tpl)?;
    let text_call = Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        method: "Text".to_string(),
        object: None,
        args: vec![initial_concat],
        class_name: None,
    };

    let mut outer_body: Vec<Stmt> = Vec::new();
    outer_body.push(Stmt::Let {
        id: widget_id,
        name: "__perry_reactive_text_h".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(text_call),
    });

    for state_name in &state_names {
        let state_local = ctx
            .lookup_local(state_name)
            .ok_or_else(|| anyhow!("reactive Text: state '{}' not in scope", state_name))?;

        // Inner rebuild closure: (__v) => textSetString(__h, <fresh concat>).
        // A fresh concat is required because the callback reads the *current*
        // state values at fire-time — re-using `initial_concat` would bind to
        // the HIR tree already consumed by the Let above.
        let inner_func_id = ctx.fresh_func();
        let inner_scope = ctx.enter_scope();
        let v_param_id = ctx.define_local("__v".to_string(), Type::Any);
        let v_param = Param {
            id: v_param_id,
            name: "__v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
        };
        let fresh_concat = lower_tpl_to_concat(ctx, tpl)?;
        let set_text_call = Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "textSetString".to_string(),
            object: None,
            args: vec![Expr::LocalGet(widget_id), fresh_concat],
            class_name: None,
        };
        let inner_body = vec![Stmt::Expr(set_text_call)];
        ctx.exit_scope(inner_scope);

        let mut inner_refs = Vec::new();
        let mut inner_visited = std::collections::HashSet::new();
        for stmt in &inner_body {
            collect_local_refs_stmt(stmt, &mut inner_refs, &mut inner_visited);
        }
        let mut inner_captures: Vec<LocalId> = inner_refs
            .into_iter()
            .filter(|id| *id != v_param_id)
            .collect();
        inner_captures.sort();
        inner_captures.dedup();
        inner_captures = ctx.filter_module_level_captures(inner_captures);

        let inner_closure = Expr::Closure {
            func_id: inner_func_id,
            params: vec![v_param],
            return_type: Type::Any,
            body: inner_body,
            captures: inner_captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };

        outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "stateOnChange".to_string(),
            object: None,
            args: vec![Expr::LocalGet(state_local), inner_closure],
            class_name: None,
        }));
    }

    outer_body.push(Stmt::Return(Some(Expr::LocalGet(widget_id))));
    ctx.exit_scope(outer_scope);

    let mut outer_refs = Vec::new();
    let mut outer_visited = std::collections::HashSet::new();
    for stmt in &outer_body {
        collect_local_refs_stmt(stmt, &mut outer_refs, &mut outer_visited);
    }
    let mut outer_captures: Vec<LocalId> = outer_refs
        .into_iter()
        .filter(|id| *id != widget_id)
        .collect();
    outer_captures.sort();
    outer_captures.dedup();
    outer_captures = ctx.filter_module_level_captures(outer_captures);

    let outer_closure = Expr::Closure {
        func_id: outer_func_id,
        params: vec![],
        return_type: Type::Any,
        body: outer_body,
        captures: outer_captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    Ok(Some(Expr::Call {
        callee: Box::new(outer_closure),
        args: vec![],
        type_args: vec![],
    }))
}

/// Walk an AST expression and collect identifiers used as `<ident>.value`
/// where `<ident>` resolves to a `perry/ui` State native instance. Callers
/// use the collected names to register `stateOnChange` subscribers.
///
/// Covers the expression shapes most commonly found in animation arguments:
/// ternaries, binary/logical ops, parens, template literals, unary,
/// assignment RHS, call args, array/object literals, and member reads. The
/// catch-all silently skips unhandled shapes — worst case, a state read
/// inside an exotic expression just won't trigger reactivity (same
/// conservative failure mode as #104's template walker).
fn collect_state_value_reads(ctx: &LoweringContext, expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Member(member) => {
            // `<ident>.value` where ident is a registered State.
            if let ast::MemberProp::Ident(prop) = &member.prop {
                if prop.sym.as_ref() == "value" {
                    if let ast::Expr::Ident(obj) = member.obj.as_ref() {
                        let name = obj.sym.to_string();
                        if matches!(
                            ctx.lookup_native_instance(&name),
                            Some(("perry/ui", "State"))
                        ) && !out.contains(&name)
                        {
                            out.push(name);
                            return;
                        }
                    }
                }
            }
            collect_state_value_reads(ctx, member.obj.as_ref(), out);
        }
        ast::Expr::Paren(p) => collect_state_value_reads(ctx, &p.expr, out),
        ast::Expr::Cond(c) => {
            collect_state_value_reads(ctx, &c.test, out);
            collect_state_value_reads(ctx, &c.cons, out);
            collect_state_value_reads(ctx, &c.alt, out);
        }
        ast::Expr::Bin(b) => {
            collect_state_value_reads(ctx, &b.left, out);
            collect_state_value_reads(ctx, &b.right, out);
        }
        ast::Expr::Unary(u) => collect_state_value_reads(ctx, &u.arg, out),
        ast::Expr::Tpl(t) => {
            for e in &t.exprs {
                collect_state_value_reads(ctx, e, out);
            }
        }
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(ce) = &c.callee {
                collect_state_value_reads(ctx, ce, out);
            }
            for a in &c.args {
                collect_state_value_reads(ctx, &a.expr, out);
            }
        }
        ast::Expr::Array(a) => {
            for el in a.elems.iter().flatten() {
                collect_state_value_reads(ctx, &el.expr, out);
            }
        }
        ast::Expr::Seq(s) => {
            for e in &s.exprs {
                collect_state_value_reads(ctx, e, out);
            }
        }
        ast::Expr::TsNonNull(n) => collect_state_value_reads(ctx, &n.expr, out),
        ast::Expr::TsAs(a) => collect_state_value_reads(ctx, &a.expr, out),
        ast::Expr::TsTypeAssertion(a) => collect_state_value_reads(ctx, &a.expr, out),
        _ => {}
    }
}

/// Desugar `widget.animateOpacity(<expr>, dur)` / `.animatePosition(...)`
/// into an IIFE that runs the initial animation and registers a
/// `stateOnChange` subscriber per `State` read in the args, so toggling the
/// state re-fires the animation.
///
/// Generated HIR shape (animateOpacity with one state dependency):
/// ```text
/// (() => {
///     const __h = <widget>;
///     widgetAnimateOpacity(__h, target, dur);       // initial
///     stateOnChange(state1, (__v) => widgetAnimateOpacity(__h, fresh_target, dur));
///     return undefined;
/// })()
/// ```
///
/// Like the reactive-Text desugar (#104), the target expression is re-lowered
/// for the subscriber body so it reads the *current* state value at fire time.
pub(super) fn try_desugar_reactive_animate(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    let (method_name, expected_arity) = match prop.sym.as_ref() {
        "animateOpacity" => ("widgetAnimateOpacity", 2),
        "animatePosition" => ("widgetAnimatePosition", 3),
        _ => return Ok(None),
    };
    if call.args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    if call.args.len() != expected_arity {
        return Ok(None);
    }

    // Collect unique state names whose `.value` is read anywhere in the args.
    // Preserving insertion order keeps subscriber registration deterministic.
    let mut state_names: Vec<String> = Vec::new();
    for arg in &call.args {
        collect_state_value_reads(ctx, &arg.expr, &mut state_names);
    }
    if state_names.is_empty() {
        return Ok(None);
    }

    // Lower the receiver once; store in an IIFE local so the initial call and
    // every subscriber share the same widget handle without re-evaluating
    // side-effectful receiver expressions.
    let widget_expr = lower_expr(ctx, member.obj.as_ref())?;

    let outer_func_id = ctx.fresh_func();
    let outer_scope = ctx.enter_scope();
    let widget_id = ctx.define_local("__perry_anim_widget".to_string(), Type::Any);

    let mut outer_body: Vec<Stmt> = Vec::new();
    outer_body.push(Stmt::Let {
        id: widget_id,
        name: "__perry_anim_widget".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(widget_expr),
    });

    let mut initial_args: Vec<Expr> = Vec::with_capacity(expected_arity + 1);
    initial_args.push(Expr::LocalGet(widget_id));
    for a in &call.args {
        initial_args.push(lower_expr(ctx, &a.expr)?);
    }
    outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        method: method_name.to_string(),
        object: None,
        args: initial_args,
        class_name: None,
    }));

    for state_name in &state_names {
        let state_local = ctx
            .lookup_local(state_name)
            .ok_or_else(|| anyhow!("reactive animate: state '{}' not in scope", state_name))?;

        let inner_func_id = ctx.fresh_func();
        let inner_scope = ctx.enter_scope();
        let v_param_id = ctx.define_local("__v".to_string(), Type::Any);
        let v_param = Param {
            id: v_param_id,
            name: "__v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
        };

        let mut fresh_args: Vec<Expr> = Vec::with_capacity(expected_arity + 1);
        fresh_args.push(Expr::LocalGet(widget_id));
        for a in &call.args {
            fresh_args.push(lower_expr(ctx, &a.expr)?);
        }
        let animate_call = Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: method_name.to_string(),
            object: None,
            args: fresh_args,
            class_name: None,
        };
        let inner_body = vec![Stmt::Expr(animate_call)];
        ctx.exit_scope(inner_scope);

        let mut inner_refs = Vec::new();
        let mut inner_visited = std::collections::HashSet::new();
        for stmt in &inner_body {
            collect_local_refs_stmt(stmt, &mut inner_refs, &mut inner_visited);
        }
        let mut inner_captures: Vec<LocalId> = inner_refs
            .into_iter()
            .filter(|id| *id != v_param_id)
            .collect();
        inner_captures.sort();
        inner_captures.dedup();
        inner_captures = ctx.filter_module_level_captures(inner_captures);

        let inner_closure = Expr::Closure {
            func_id: inner_func_id,
            params: vec![v_param],
            return_type: Type::Any,
            body: inner_body,
            captures: inner_captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };

        outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "stateOnChange".to_string(),
            object: None,
            args: vec![Expr::LocalGet(state_local), inner_closure],
            class_name: None,
        }));
    }

    outer_body.push(Stmt::Return(Some(Expr::Undefined)));
    ctx.exit_scope(outer_scope);

    let mut outer_refs = Vec::new();
    let mut outer_visited = std::collections::HashSet::new();
    for stmt in &outer_body {
        collect_local_refs_stmt(stmt, &mut outer_refs, &mut outer_visited);
    }
    let mut outer_captures: Vec<LocalId> = outer_refs
        .into_iter()
        .filter(|id| *id != widget_id)
        .collect();
    outer_captures.sort();
    outer_captures.dedup();
    outer_captures = ctx.filter_module_level_captures(outer_captures);

    let outer_closure = Expr::Closure {
        func_id: outer_func_id,
        params: vec![],
        return_type: Type::Any,
        body: outer_body,
        captures: outer_captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    Ok(Some(Expr::Call {
        callee: Box::new(outer_closure),
        args: vec![],
        type_args: vec![],
    }))
}

/// Try to lower a Widget({...}) call from perry/widget into a WidgetDecl.
/// Returns Some(WidgetDecl) if this is a widget declaration, None otherwise.
fn try_lower_widget_decl(ctx: &LoweringContext, call_expr: &ast::CallExpr) -> Option<WidgetDecl> {
    // Check callee is a function imported from perry/widget named "Widget"
    let callee = match &call_expr.callee {
        ast::Callee::Expr(expr) => expr,
        _ => return None,
    };
    let func_name = match callee.as_ref() {
        ast::Expr::Ident(ident) => ident.sym.as_ref(),
        _ => return None,
    };
    let (module, method) = ctx.lookup_native_module(func_name)?;
    if module != "perry/widget" {
        return None;
    }
    let method_name = method.unwrap_or(func_name);
    if method_name != "Widget" {
        return None;
    }

    // First arg should be the config object literal
    let config_obj = match call_expr.args.first() {
        Some(arg) => match arg.expr.as_ref() {
            ast::Expr::Object(obj) => obj,
            _ => return None,
        },
        None => return None,
    };

    let mut kind = String::new();
    let mut display_name = String::new();
    let mut description = String::new();
    let mut supported_families: Vec<String> = Vec::new();
    let mut entry_fields: Vec<(String, WidgetFieldType)> = Vec::new();
    let mut render_body: Vec<WidgetNode> = Vec::new();
    let mut entry_param_name = "entry".to_string();
    let mut config_params: Vec<WidgetConfigParam> = Vec::new();
    let mut provider_func_name: Option<String> = None;
    let mut placeholder: Option<Vec<(String, WidgetPlaceholderValue)>> = None;
    let mut family_param_name: Option<String> = None;
    let mut app_group: Option<String> = None;
    let reload_after_seconds: Option<u32> = None;

    for prop in &config_obj.props {
        let kv = match prop {
            ast::PropOrSpread::Prop(p) => match p.as_ref() {
                ast::Prop::KeyValue(kv) => kv,
                ast::Prop::Method(method) => {
                    let key = prop_name_to_string(&method.key);
                    if key == "render" {
                        // Extract parameter name
                        if let Some(param) = method.function.params.first() {
                            if let ast::Pat::Ident(ident) = &param.pat {
                                entry_param_name = ident.id.sym.to_string();
                            }
                        }
                        // Check for 2nd parameter (family)
                        if let Some(param) = method.function.params.get(1) {
                            if let ast::Pat::Ident(ident) = &param.pat {
                                family_param_name = Some(ident.id.sym.to_string());
                            }
                        }
                        // Extract type annotation for entry fields (only if not already specified via entryFields)
                        if entry_fields.is_empty() {
                            if let Some(param) = method.function.params.first() {
                                extract_entry_fields_from_param(&param.pat, &mut entry_fields);
                            }
                        }
                        // Parse render body — detect family switches
                        if let Some(body) = &method.function.body {
                            let nodes = parse_render_body_stmts(&body.stmts, &family_param_name);
                            render_body = nodes;
                        }
                    } else if key == "provider" {
                        // Provider as method: provider(config) { ... }
                        let func_name = format!("__widget_provider_{}", kind);
                        provider_func_name = Some(func_name);
                    }
                    continue;
                }
                _ => continue,
            },
            _ => continue,
        };

        let key = prop_name_to_string(&kv.key);
        match key.as_str() {
            "kind" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    kind = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "displayName" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    display_name = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "description" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    description = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "supportedFamilies" => {
                if let ast::Expr::Array(arr) = kv.value.as_ref() {
                    for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                        if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                            supported_families.push(s.value.as_str().unwrap_or("").to_string());
                        }
                    }
                }
            }
            "appGroup" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    app_group = Some(s.value.as_str().unwrap_or("").to_string());
                }
            }
            "config" => {
                // Parse config object → Vec<WidgetConfigParam>
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let param_name = prop_name_to_string(&field_kv.key);
                                if let Some(param) =
                                    parse_widget_config_param(&param_name, &field_kv.value)
                                {
                                    config_params.push(param);
                                }
                            }
                        }
                    }
                }
            }
            "provider" => {
                // Arrow function provider: provider: async (config) => { ... }
                if let ast::Expr::Arrow(_arrow) = kv.value.as_ref() {
                    let func_name = if kind.is_empty() {
                        "__widget_provider_widget".to_string()
                    } else {
                        let safe = kind.rsplit('.').next().unwrap_or(&kind);
                        format!("__widget_provider_{}", safe)
                    };
                    provider_func_name = Some(func_name);
                }
            }
            "placeholder" => {
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    let mut fields = Vec::new();
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let field_name = prop_name_to_string(&field_kv.key);
                                let val = parse_placeholder_value(&field_kv.value);
                                fields.push((field_name, val));
                            }
                        }
                    }
                    placeholder = Some(fields);
                }
            }
            "entryFields" => {
                // Allow explicit entry field declarations
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let field_name = prop_name_to_string(&field_kv.key);
                                let field_type = match field_kv.value.as_ref() {
                                    ast::Expr::Lit(ast::Lit::Str(s)) => {
                                        match s.value.as_str().unwrap_or("") {
                                            "number" => WidgetFieldType::Number,
                                            "boolean" => WidgetFieldType::Boolean,
                                            _ => WidgetFieldType::String,
                                        }
                                    }
                                    _ => WidgetFieldType::String,
                                };
                                entry_fields.push((field_name, field_type));
                            }
                        }
                    }
                }
            }
            "render" => {
                // Arrow function: render: (entry) => VStack(...)
                if let ast::Expr::Arrow(arrow) = kv.value.as_ref() {
                    // Extract parameter name
                    if let Some(param) = arrow.params.first() {
                        if let ast::Pat::Ident(ident) = param {
                            entry_param_name = ident.id.sym.to_string();
                        }
                    }
                    // Check for 2nd parameter (family)
                    if let Some(param) = arrow.params.get(1) {
                        if let ast::Pat::Ident(ident) = param {
                            family_param_name = Some(ident.id.sym.to_string());
                        }
                    }
                    // Extract entry fields from type annotation (only if not already specified via entryFields)
                    if entry_fields.is_empty() {
                        if let Some(param) = arrow.params.first() {
                            extract_entry_fields_from_param(param, &mut entry_fields);
                        }
                    }
                    // Parse body
                    match arrow.body.as_ref() {
                        ast::BlockStmtOrExpr::Expr(expr) => {
                            if let Some(node) = parse_widget_node(expr) {
                                render_body.push(node);
                            }
                        }
                        ast::BlockStmtOrExpr::BlockStmt(block) => {
                            let nodes = parse_render_body_stmts(&block.stmts, &family_param_name);
                            render_body = nodes;
                        }
                    }
                }
            }
            _ => {} // Skip timeline and other fields handled differently
        }
    }

    if kind.is_empty() {
        kind = "com.perry.widget".to_string();
    }

    // Fix provider func name if kind was set after provider was parsed
    if let Some(ref mut pfn) = provider_func_name {
        if pfn == "__widget_provider_widget" && kind != "com.perry.widget" {
            let safe = kind.rsplit('.').next().unwrap_or(&kind);
            *pfn = format!("__widget_provider_{}", safe);
        }
    }

    Some(WidgetDecl {
        kind,
        display_name,
        description,
        supported_families,
        entry_fields,
        render_body,
        entry_param_name,
        config_params,
        provider_func_name,
        placeholder,
        family_param_name,
        app_group,
        reload_after_seconds,
    })
}

/// Extract entry fields from a typed parameter pattern (e.g., `entry: MyEntry`)
fn extract_entry_fields_from_param(pat: &ast::Pat, fields: &mut Vec<(String, WidgetFieldType)>) {
    // Try to get type annotation
    let type_ann = match pat {
        ast::Pat::Ident(ident) => ident.type_ann.as_ref(),
        _ => None,
    };
    if let Some(ann) = type_ann {
        if let ast::TsType::TsTypeLit(lit) = ann.type_ann.as_ref() {
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let field_name = ident.sym.to_string();
                        // Skip 'date' as it's always present in TimelineEntry
                        if field_name == "date" {
                            continue;
                        }
                        let is_optional = prop.optional;
                        let field_type = if let Some(ann) = &prop.type_ann {
                            parse_widget_field_type(ann.type_ann.as_ref())
                        } else {
                            WidgetFieldType::String
                        };
                        let field_type = if is_optional {
                            WidgetFieldType::Optional(Box::new(field_type))
                        } else {
                            field_type
                        };
                        fields.push((field_name, field_type));
                    }
                }
            }
        }
    }
}

/// Recursively parse a TypeScript type annotation into a WidgetFieldType
fn parse_widget_field_type(ts_type: &ast::TsType) -> WidgetFieldType {
    match ts_type {
        ast::TsType::TsKeywordType(kw) => match kw.kind {
            ast::TsKeywordTypeKind::TsNumberKeyword => WidgetFieldType::Number,
            ast::TsKeywordTypeKind::TsBooleanKeyword => WidgetFieldType::Boolean,
            ast::TsKeywordTypeKind::TsStringKeyword => WidgetFieldType::String,
            _ => WidgetFieldType::String,
        },
        ast::TsType::TsArrayType(arr) => {
            let inner = parse_widget_field_type(arr.elem_type.as_ref());
            WidgetFieldType::Array(Box::new(inner))
        }
        ast::TsType::TsTypeLit(lit) => {
            // Nested object type: { url: string, clicks: number }
            let mut obj_fields = Vec::new();
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let name = ident.sym.to_string();
                        let inner = if let Some(ann) = &prop.type_ann {
                            parse_widget_field_type(ann.type_ann.as_ref())
                        } else {
                            WidgetFieldType::String
                        };
                        let inner = if prop.optional {
                            WidgetFieldType::Optional(Box::new(inner))
                        } else {
                            inner
                        };
                        obj_fields.push((name, inner));
                    }
                }
            }
            WidgetFieldType::Object(obj_fields)
        }
        ast::TsType::TsUnionOrIntersectionType(ast::TsUnionOrIntersectionType::TsUnionType(
            union,
        )) => {
            // Check for T | null or T | undefined → Optional(T)
            let mut non_null_types: Vec<&ast::TsType> = Vec::new();
            let mut has_null = false;
            for member in &union.types {
                match member.as_ref() {
                    ast::TsType::TsKeywordType(kw)
                        if matches!(
                            kw.kind,
                            ast::TsKeywordTypeKind::TsNullKeyword
                                | ast::TsKeywordTypeKind::TsUndefinedKeyword
                        ) =>
                    {
                        has_null = true;
                    }
                    other => non_null_types.push(other),
                }
            }
            if has_null && non_null_types.len() == 1 {
                WidgetFieldType::Optional(Box::new(parse_widget_field_type(non_null_types[0])))
            } else if !non_null_types.is_empty() {
                parse_widget_field_type(non_null_types[0])
            } else {
                WidgetFieldType::String
            }
        }
        _ => WidgetFieldType::String,
    }
}

/// Parse a widget node from an AST expression.
/// Recognizes calls like Text("hello"), VStack({...}, [...]), Image({systemName: "star"}), etc.
fn parse_widget_node(expr: &ast::Expr) -> Option<WidgetNode> {
    match expr {
        ast::Expr::Call(call) => {
            let func_name = match &call.callee {
                ast::Callee::Expr(e) => match e.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    _ => return None,
                },
                _ => return None,
            };

            match func_name.as_str() {
                "Text" => {
                    let content = call
                        .args
                        .first()
                        .map(|arg| parse_text_content(&arg.expr))
                        .unwrap_or(WidgetTextContent::Literal(String::new()));
                    let modifiers = parse_modifiers_from_args(&call.args, 1);
                    Some(WidgetNode::Text { content, modifiers })
                }
                "VStack" | "HStack" | "ZStack" => {
                    let kind = match func_name.as_str() {
                        "VStack" => WidgetStackKind::VStack,
                        "HStack" => WidgetStackKind::HStack,
                        "ZStack" => WidgetStackKind::ZStack,
                        _ => unreachable!(),
                    };
                    parse_stack_node(kind, &call.args)
                }
                "Image" => parse_image_node(&call.args),
                "Spacer" => Some(WidgetNode::Spacer),
                "Divider" => Some(WidgetNode::Divider),
                "ForEach" => parse_foreach_node(&call.args),
                "Label" => parse_label_node(&call.args),
                "Gauge" => parse_gauge_node(&call.args),
                _ => None,
            }
        }
        ast::Expr::Cond(cond) => {
            // Ternary: condition ? then : else
            parse_conditional_node(cond)
        }
        _ => None,
    }
}

/// Parse text content from an expression
fn parse_text_content(expr: &ast::Expr) -> WidgetTextContent {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            WidgetTextContent::Literal(s.value.as_str().unwrap_or("").to_string())
        }
        ast::Expr::Member(member) => {
            // entry.fieldName
            if let ast::MemberProp::Ident(prop) = &member.prop {
                WidgetTextContent::Field(prop.sym.to_string())
            } else {
                WidgetTextContent::Literal(String::new())
            }
        }
        ast::Expr::Tpl(tpl) => {
            // Template literal: `Score: ${entry.score}`
            let mut parts = Vec::new();
            for (i, quasi) in tpl.quasis.iter().enumerate() {
                let raw = quasi.raw.as_ref().to_string();
                if !raw.is_empty() {
                    parts.push(WidgetTemplatePart::Literal(raw));
                }
                if i < tpl.exprs.len() {
                    if let ast::Expr::Member(member) = tpl.exprs[i].as_ref() {
                        if let ast::MemberProp::Ident(prop) = &member.prop {
                            parts.push(WidgetTemplatePart::Field(prop.sym.to_string()));
                        }
                    }
                }
            }
            WidgetTextContent::Template(parts)
        }
        _ => WidgetTextContent::Literal(String::new()),
    }
}

/// Parse a stack node (VStack, HStack, ZStack) from call arguments.
/// Supports two patterns:
///   VStack([child1, child2])
///   VStack({ spacing: 8 }, [child1, child2])
fn parse_stack_node(kind: WidgetStackKind, args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let mut spacing = None;
    let mut children = Vec::new();
    let mut modifiers = Vec::new();
    let mut children_arg_idx = 0;

    // Check if first arg is config object
    if let Some(first) = args.first() {
        match first.expr.as_ref() {
            ast::Expr::Object(obj) => {
                // First arg is config: { spacing: 8 }
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let key = prop_name_to_string(&kv.key);
                            if key == "spacing" {
                                if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                    spacing = Some(n.value);
                                }
                            }
                        }
                    }
                }
                children_arg_idx = 1;
            }
            ast::Expr::Array(_) => {
                // First arg is children array directly
                children_arg_idx = 0;
            }
            _ => {}
        }
    }

    // Parse children array
    if let Some(arg) = args.get(children_arg_idx) {
        if let ast::Expr::Array(arr) = arg.expr.as_ref() {
            for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                if let Some(node) = parse_widget_node(expr) {
                    children.push(node);
                }
            }
        }
    }

    // Parse modifiers from remaining args
    let modifier_start = children_arg_idx + 1;
    modifiers = parse_modifiers_from_args(args, modifier_start);

    Some(WidgetNode::Stack {
        kind,
        spacing,
        children,
        modifiers,
    })
}

/// Parse an Image node from call arguments.
/// Image({ systemName: "star.fill" })
fn parse_image_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let first = args.first()?;
    let system_name = match first.expr.as_ref() {
        ast::Expr::Object(obj) => {
            let mut name = String::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if key == "systemName" {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                name = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                    }
                }
            }
            name
        }
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
        _ => return None,
    };

    let modifiers = parse_modifiers_from_args(args, 1);
    Some(WidgetNode::Image {
        system_name,
        modifiers,
    })
}

/// Parse a conditional node from a ternary expression
fn parse_conditional_node(cond: &ast::CondExpr) -> Option<WidgetNode> {
    // Parse condition: entry.field > value, entry.field == value, etc.
    let (field, op, value) = parse_condition(&cond.test)?;
    let then_node = parse_widget_node(&cond.cons)?;
    let else_node = parse_widget_node(&cond.alt);

    Some(WidgetNode::Conditional {
        field,
        op,
        value,
        then_node: Box::new(then_node),
        else_node: else_node.map(Box::new),
    })
}

/// Parse a binary condition expression
fn parse_condition(expr: &ast::Expr) -> Option<(String, WidgetConditionOp, WidgetTextContent)> {
    match expr {
        ast::Expr::Bin(bin) => {
            let field = match bin.left.as_ref() {
                ast::Expr::Member(member) => {
                    if let ast::MemberProp::Ident(prop) = &member.prop {
                        prop.sym.to_string()
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let op = match bin.op {
                ast::BinaryOp::Gt => WidgetConditionOp::GreaterThan,
                ast::BinaryOp::Lt => WidgetConditionOp::LessThan,
                ast::BinaryOp::EqEq | ast::BinaryOp::EqEqEq => WidgetConditionOp::Equals,
                ast::BinaryOp::NotEq | ast::BinaryOp::NotEqEq => WidgetConditionOp::NotEquals,
                _ => return None,
            };
            let value = parse_text_content(&bin.right);
            Some((field, op, value))
        }
        ast::Expr::Member(member) => {
            // Truthy check: entry.isActive
            if let ast::MemberProp::Ident(prop) = &member.prop {
                Some((
                    prop.sym.to_string(),
                    WidgetConditionOp::Truthy,
                    WidgetTextContent::Literal(String::new()),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse modifiers from a chained method call or from arguments.
/// In the TypeScript API, modifiers are passed as the last argument (object):
///   Text("hello", { font: "title", fontWeight: "bold", foregroundColor: "blue" })
fn parse_modifiers_from_args(args: &[ast::ExprOrSpread], start_idx: usize) -> Vec<WidgetModifier> {
    let mut modifiers = Vec::new();
    if let Some(arg) = args.get(start_idx) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if let Some(m) = parse_single_modifier(&key, &kv.value) {
                            modifiers.push(m);
                        }
                    }
                }
            }
        }
    }
    modifiers
}

/// Returns true if `name` is a known widget modifier key (used to detect
/// unsupported method-chain modifier calls, e.g. `Text("hi").font("title")`).
pub(super) fn is_widget_modifier_name(name: &str) -> bool {
    matches!(
        name,
        "font"
            | "fontWeight"
            | "weight"
            | "foregroundColor"
            | "color"
            | "foreground"
            | "padding"
            | "cornerRadius"
            | "background"
            | "backgroundColor"
            | "opacity"
            | "lineLimit"
            | "frame"
            | "minimumScaleFactor"
            | "containerBackground"
            | "maxWidth"
            | "url"
            | "bold"
            | "italic"
            | "underline"
            | "fontSize"
            | "strikethrough"
            | "multilineTextAlignment"
            | "lineSpacing"
    )
}

/// Parse a single modifier from key/value
fn parse_single_modifier(key: &str, value: &ast::Expr) -> Option<WidgetModifier> {
    match key {
        "font" => match value {
            ast::Expr::Lit(ast::Lit::Str(s)) => {
                let font = match s.value.as_str().unwrap_or("") {
                    "headline" => WidgetFont::Headline,
                    "title" => WidgetFont::Title,
                    "title2" => WidgetFont::Title2,
                    "title3" => WidgetFont::Title3,
                    "body" => WidgetFont::Body,
                    "caption" => WidgetFont::Caption,
                    "caption2" => WidgetFont::Caption2,
                    "footnote" => WidgetFont::Footnote,
                    "subheadline" => WidgetFont::Subheadline,
                    "largeTitle" => WidgetFont::LargeTitle,
                    name => WidgetFont::Named(name.to_string()),
                };
                Some(WidgetModifier::Font(font))
            }
            ast::Expr::Lit(ast::Lit::Num(n)) => {
                Some(WidgetModifier::Font(WidgetFont::System(n.value)))
            }
            _ => None,
        },
        "fontWeight" | "weight" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::FontWeight(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "foregroundColor" | "color" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ForegroundColor(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "padding" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Padding(n.value))
            } else {
                None
            }
        }
        "cornerRadius" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::CornerRadius(n.value))
            } else {
                None
            }
        }
        "background" | "backgroundColor" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::Background(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "opacity" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Opacity(n.value))
            } else {
                None
            }
        }
        "lineLimit" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::LineLimit(n.value as u32))
            } else {
                None
            }
        }
        "frame" => {
            if let ast::Expr::Object(obj) = value {
                let mut width = None;
                let mut height = None;
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let k = prop_name_to_string(&kv.key);
                            if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                match k.as_str() {
                                    "width" => width = Some(n.value),
                                    "height" => height = Some(n.value),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Some(WidgetModifier::Frame { width, height })
            } else {
                None
            }
        }
        "minimumScaleFactor" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::MinimumScaleFactor(n.value))
            } else {
                None
            }
        }
        "containerBackground" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ContainerBackground(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "maxWidth" => {
            // maxWidth: true or maxWidth: "infinity"
            Some(WidgetModifier::FrameMaxWidth)
        }
        "url" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::WidgetURL(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a ForEach node: ForEach(entry.items, (item) => HStack([...]))
fn parse_foreach_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    // First arg: entry.items (member expression)
    let collection_field = match args.first()?.expr.as_ref() {
        ast::Expr::Member(member) => {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                prop.sym.to_string()
            } else {
                return None;
            }
        }
        _ => return None,
    };

    // Second arg: arrow function (item) => ...
    let arrow = match args.get(1)?.expr.as_ref() {
        ast::Expr::Arrow(arrow) => arrow,
        _ => return None,
    };

    let item_param = if let Some(param) = arrow.params.first() {
        if let ast::Pat::Ident(ident) = param {
            ident.id.sym.to_string()
        } else {
            "item".to_string()
        }
    } else {
        "item".to_string()
    };

    let body = match arrow.body.as_ref() {
        ast::BlockStmtOrExpr::Expr(expr) => parse_widget_node(expr)?,
        ast::BlockStmtOrExpr::BlockStmt(block) => {
            for stmt in &block.stmts {
                if let ast::Stmt::Return(ret) = stmt {
                    if let Some(arg) = &ret.arg {
                        if let Some(node) = parse_widget_node(arg) {
                            return Some(WidgetNode::ForEach {
                                collection_field,
                                item_param,
                                body: Box::new(node),
                            });
                        }
                    }
                }
            }
            return None;
        }
    };

    Some(WidgetNode::ForEach {
        collection_field,
        item_param,
        body: Box::new(body),
    })
}

/// Parse a Label node: Label("text", { systemImage: "star.fill" })
fn parse_label_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let text = args
        .first()
        .map(|arg| parse_text_content(&arg.expr))
        .unwrap_or(WidgetTextContent::Literal(String::new()));

    let mut system_image = String::new();
    let mut modifiers = Vec::new();

    // Second arg: { systemImage: "star.fill", font: "caption" }
    if let Some(arg) = args.get(1) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if key == "systemImage" {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                system_image = s.value.as_str().unwrap_or("").to_string();
                            }
                        } else if let Some(m) = parse_single_modifier(&key, &kv.value) {
                            modifiers.push(m);
                        }
                    }
                }
            }
        }
    }

    Some(WidgetNode::Label {
        text,
        system_image,
        modifiers,
    })
}

/// Parse a Gauge node: Gauge(value, { label: "Clicks", style: "circular" })
fn parse_gauge_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    // First arg: value expression (entry.field / entry.field, or numeric expression)
    let value_expr = match args.first()?.expr.as_ref() {
        ast::Expr::Member(member) => {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                prop.sym.to_string()
            } else {
                return None;
            }
        }
        ast::Expr::Bin(bin) => {
            // entry.totalClicks / entry.clicksGoal
            let left = match bin.left.as_ref() {
                ast::Expr::Member(m) => {
                    if let ast::MemberProp::Ident(p) = &m.prop {
                        p.sym.to_string()
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let right = match bin.right.as_ref() {
                ast::Expr::Member(m) => {
                    if let ast::MemberProp::Ident(p) = &m.prop {
                        p.sym.to_string()
                    } else {
                        return None;
                    }
                }
                ast::Expr::Lit(ast::Lit::Num(n)) => format!("{}", n.value),
                _ => return None,
            };
            let op = match bin.op {
                ast::BinaryOp::Div => "/",
                ast::BinaryOp::Mul => "*",
                ast::BinaryOp::Sub => "-",
                ast::BinaryOp::Add => "+",
                _ => return None,
            };
            format!("{} {} {}", left, op, right)
        }
        _ => return None,
    };

    let mut label = String::new();
    let mut style = GaugeStyle::Circular;
    let mut modifiers = Vec::new();

    // Second arg: config object
    if let Some(arg) = args.get(1) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        match key.as_str() {
                            "label" => {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                    label = s.value.as_str().unwrap_or("").to_string();
                                }
                            }
                            "style" => {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                    style = match s.value.as_str().unwrap_or("") {
                                        "linear" | "linearCapacity" => GaugeStyle::LinearCapacity,
                                        _ => GaugeStyle::Circular,
                                    };
                                }
                            }
                            _ => {
                                if let Some(m) = parse_single_modifier(&key, &kv.value) {
                                    modifiers.push(m);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some(WidgetNode::Gauge {
        value_expr,
        label,
        style,
        modifiers,
    })
}

/// Parse render body statements, detecting family-switch patterns (if/else on family param)
fn parse_render_body_stmts(stmts: &[ast::Stmt], family_param: &Option<String>) -> Vec<WidgetNode> {
    let mut nodes = Vec::new();

    // Check for if (family === "systemSmall") { ... } else if ... pattern
    if let Some(family_name) = family_param {
        if let Some(family_switch) = try_parse_family_switch(stmts, family_name) {
            nodes.push(family_switch);
            return nodes;
        }
    }

    // Fall back to regular return-based parsing
    for stmt in stmts {
        if let ast::Stmt::Return(ret) = stmt {
            if let Some(arg) = &ret.arg {
                if let Some(node) = parse_widget_node(arg) {
                    nodes.push(node);
                }
            }
        }
    }
    nodes
}

/// Try to parse a series of if (family === "X") { return ... } statements into a FamilySwitch
fn try_parse_family_switch(stmts: &[ast::Stmt], family_name: &str) -> Option<WidgetNode> {
    let mut cases: Vec<(String, WidgetNode)> = Vec::new();
    let mut default_node: Option<Box<WidgetNode>> = None;

    for stmt in stmts {
        match stmt {
            ast::Stmt::If(if_stmt) => {
                // Check: if (family === "systemSmall") { return VStack([...]) }
                if let Some((family_value, node)) =
                    try_parse_family_case(&if_stmt.test, &if_stmt.cons, family_name)
                {
                    cases.push((family_value, node));
                }
                // Check else branch for more cases or default
                if let Some(alt) = &if_stmt.alt {
                    match alt.as_ref() {
                        ast::Stmt::Block(block) => {
                            // else { return ... } — this is the default
                            for s in &block.stmts {
                                if let ast::Stmt::Return(ret) = s {
                                    if let Some(arg) = &ret.arg {
                                        if let Some(node) = parse_widget_node(arg) {
                                            default_node = Some(Box::new(node));
                                        }
                                    }
                                }
                            }
                        }
                        ast::Stmt::If(nested_if) => {
                            // else if — extract more cases
                            if let Some((family_value, node)) =
                                try_parse_family_case(&nested_if.test, &nested_if.cons, family_name)
                            {
                                cases.push((family_value, node));
                            }
                        }
                        ast::Stmt::Return(ret) => {
                            if let Some(arg) = &ret.arg {
                                if let Some(node) = parse_widget_node(arg) {
                                    default_node = Some(Box::new(node));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ast::Stmt::Return(ret) => {
                // Trailing return is the default case
                if let Some(arg) = &ret.arg {
                    if let Some(node) = parse_widget_node(arg) {
                        if cases.is_empty() {
                            // No family switch, just a regular return
                            return None;
                        }
                        default_node = Some(Box::new(node));
                    }
                }
            }
            _ => {}
        }
    }

    if cases.is_empty() {
        return None;
    }

    Some(WidgetNode::FamilySwitch {
        cases,
        default: default_node,
    })
}

/// Try to parse a single if (family === "value") { return node } case
fn try_parse_family_case(
    test: &ast::Expr,
    cons: &ast::Stmt,
    family_name: &str,
) -> Option<(String, WidgetNode)> {
    // Check: family === "systemSmall"
    let family_value = match test {
        ast::Expr::Bin(bin) if matches!(bin.op, ast::BinaryOp::EqEqEq | ast::BinaryOp::EqEq) => {
            let is_family_left = match bin.left.as_ref() {
                ast::Expr::Ident(ident) => ident.sym.as_ref() == family_name,
                _ => false,
            };
            if !is_family_left {
                return None;
            }
            match bin.right.as_ref() {
                ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };

    // Extract return value from consequent block
    let node = match cons {
        ast::Stmt::Block(block) => {
            let mut result = None;
            for s in &block.stmts {
                if let ast::Stmt::Return(ret) = s {
                    if let Some(arg) = &ret.arg {
                        result = parse_widget_node(arg);
                    }
                }
            }
            result?
        }
        ast::Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                parse_widget_node(arg)?
            } else {
                return None;
            }
        }
        _ => return None,
    };

    Some((family_value, node))
}

/// Parse a WidgetConfigParam from a config field value
fn parse_widget_config_param(name: &str, value: &ast::Expr) -> Option<WidgetConfigParam> {
    if let ast::Expr::Object(obj) = value {
        let mut param_type_str = String::new();
        let mut title = name.to_string();
        let mut values: Vec<String> = Vec::new();
        let mut default_str = String::new();
        let mut default_bool = false;

        for prop in &obj.props {
            if let ast::PropOrSpread::Prop(p) = prop {
                if let ast::Prop::KeyValue(kv) = p.as_ref() {
                    let key = prop_name_to_string(&kv.key);
                    match key.as_str() {
                        "type" => {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                param_type_str = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                        "title" => {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                title = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                        "default" => match kv.value.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => {
                                default_str = s.value.as_str().unwrap_or("").to_string();
                            }
                            ast::Expr::Lit(ast::Lit::Bool(b)) => {
                                default_bool = b.value;
                            }
                            _ => {}
                        },
                        "values" => {
                            if let ast::Expr::Array(arr) = kv.value.as_ref() {
                                for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                                    if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                                        values.push(s.value.as_str().unwrap_or("").to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let param_type = match param_type_str.as_str() {
            "enum" => WidgetConfigParamType::Enum {
                values,
                default: if default_str.is_empty() {
                    "".to_string()
                } else {
                    default_str
                },
            },
            "bool" | "boolean" => WidgetConfigParamType::Bool {
                default: default_bool,
            },
            "string" => WidgetConfigParamType::String {
                default: default_str,
            },
            _ => WidgetConfigParamType::String {
                default: default_str,
            },
        };

        Some(WidgetConfigParam {
            name: name.to_string(),
            title,
            param_type,
        })
    } else {
        None
    }
}

/// Parse a placeholder value from an expression
fn parse_placeholder_value(expr: &ast::Expr) -> WidgetPlaceholderValue {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            WidgetPlaceholderValue::String(s.value.as_str().unwrap_or("").to_string())
        }
        ast::Expr::Lit(ast::Lit::Num(n)) => WidgetPlaceholderValue::Number(n.value),
        ast::Expr::Lit(ast::Lit::Bool(b)) => WidgetPlaceholderValue::Bool(b.value),
        ast::Expr::Lit(ast::Lit::Null(_)) => WidgetPlaceholderValue::Null,
        ast::Expr::Array(arr) => {
            let items: Vec<WidgetPlaceholderValue> = arr
                .elems
                .iter()
                .filter_map(|e| e.as_ref())
                .map(|e| parse_placeholder_value(&e.expr))
                .collect();
            WidgetPlaceholderValue::Array(items)
        }
        ast::Expr::Object(obj) => {
            let mut fields = Vec::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let name = prop_name_to_string(&kv.key);
                        let val = parse_placeholder_value(&kv.value);
                        fields.push((name, val));
                    }
                }
            }
            WidgetPlaceholderValue::Object(fields)
        }
        _ => WidgetPlaceholderValue::Null,
    }
}

/// Extract a property name from a PropName
fn prop_name_to_string(name: &ast::PropName) -> String {
    match name {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        ast::PropName::Num(n) => format!("{}", n.value),
        _ => String::new(),
    }
}

/// Detect whether an AST expression statically produces a string value.
///
/// Used to specialize `for...of` and array-spread lowering when the iterable is
/// a string — in that case we need char-by-char iteration via `str[i]` rather
/// than array-element access.
/// Check if a lowered HIR expression is a call to a generator function.
pub(super) fn is_generator_call_expr(ctx: &LoweringContext, expr: &Expr) -> bool {
    if let Expr::Call { callee, .. } = expr {
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            // Look up the function name by its ID
            for (name, id) in &ctx.functions {
                if *id == *func_id && ctx.generator_func_names.contains(name) {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn is_ast_string_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match expr {
        // String literals: "hello"
        ast::Expr::Lit(ast::Lit::Str(_)) => true,
        // Template literals: `hello ${x}`
        ast::Expr::Tpl(_) => true,
        // String identifier: look up the declared type in the current scope
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            matches!(ctx.lookup_local_type(&name), Some(Type::String))
        }
        // Parenthesized expression: recurse
        ast::Expr::Paren(p) => is_ast_string_expr(ctx, &p.expr),
        // Type assertions (`x as string`): check inner
        ast::Expr::TsAs(ts_as) => {
            if matches!(&*ts_as.type_ann,
                ast::TsType::TsKeywordType(kw)
                    if matches!(kw.kind, ast::TsKeywordTypeKind::TsStringKeyword))
            {
                return true;
            }
            is_ast_string_expr(ctx, &ts_as.expr)
        }
        ast::Expr::TsNonNull(nn) => is_ast_string_expr(ctx, &nn.expr),
        ast::Expr::TsTypeAssertion(ta) => is_ast_string_expr(ctx, &ta.expr),
        // String-returning method calls on string receivers
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee_expr) = &call.callee {
                if let ast::Expr::Member(member) = callee_expr.as_ref() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let prop = prop_ident.sym.as_ref();
                        if matches!(
                            prop,
                            "charAt"
                                | "slice"
                                | "substring"
                                | "substr"
                                | "trim"
                                | "trimStart"
                                | "trimEnd"
                                | "toLowerCase"
                                | "toUpperCase"
                                | "replace"
                                | "replaceAll"
                                | "padStart"
                                | "padEnd"
                                | "repeat"
                                | "normalize"
                                | "concat"
                                | "toString"
                                | "toLocaleLowerCase"
                                | "toLocaleUpperCase"
                        ) {
                            return is_ast_string_expr(ctx, &member.obj);
                        }
                    }
                }
            }
            false
        }
        // String concatenation: "a" + x or x + "a"
        ast::Expr::Bin(bin) if matches!(bin.op, ast::BinaryOp::Add) => {
            is_ast_string_expr(ctx, &bin.left) || is_ast_string_expr(ctx, &bin.right)
        }
        _ => false,
    }
}

/// Detect whether a var initializer is `regex.exec(str)` (after stripping
/// non-null assertion `!`). Used to mark locals so subsequent `.index`/`.groups`
/// accesses can route to the bare RegExpExecIndex/Groups HIR variants.
fn is_regex_exec_init(ctx: &LoweringContext, init: &ast::Expr) -> bool {
    let expr = match init {
        ast::Expr::TsNonNull(nn) => nn.expr.as_ref(),
        other => other,
    };
    if let ast::Expr::Call(call) = expr {
        if let ast::Callee::Expr(callee) = &call.callee {
            if let ast::Expr::Member(member) = callee.as_ref() {
                if let ast::MemberProp::Ident(method) = &member.prop {
                    let name = method.sym.as_ref();
                    // `regex.exec(str)` — receiver is RegExp.
                    if name == "exec" {
                        return match member.obj.as_ref() {
                            ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                            ast::Expr::Ident(ident) => ctx
                                .lookup_local_type(ident.sym.as_ref())
                                .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                                .unwrap_or(false),
                            _ => false,
                        };
                    }
                    // `str.match(regex)` — receiver is String (or
                    // untyped/Any). The match result has the same
                    // .index / .groups shape as exec() when the regex
                    // isn't global, so reuse the same thread-local
                    // pickup (LAST_EXEC_GROUPS). The match runtime
                    // stores groups there alongside exec.
                    if name == "match" {
                        let recv_ok = match member.obj.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(_)) | ast::Expr::Tpl(_) => true,
                            ast::Expr::Ident(ident) => {
                                // Accept String OR Any/unknown — false
                                // positives are limited to user-defined
                                // `.match()` on Any-typed receivers and
                                // their `.groups` reads naturally fall
                                // through to undefined when no string-
                                // match preceded them.
                                let ty = ctx.lookup_local_type(ident.sym.as_ref());
                                matches!(ty, Some(Type::String) | Some(Type::Any) | None)
                            }
                            _ => false,
                        };
                        // Skip global regex matches — match() with /g
                        // returns a flat array of full matches with no
                        // group metadata. Detect via a regex-literal
                        // arg with `g` flag; an Ident arg (regex stored
                        // in a variable) is optimistically treated as
                        // non-global since the common shape is `str.match(/.../)`.
                        if recv_ok {
                            let first_arg_is_global_regex =
                                call.args.first().and_then(|arg| match arg.expr.as_ref() {
                                    ast::Expr::Lit(ast::Lit::Regex(rx)) => {
                                        Some(rx.flags.as_ref().contains('g'))
                                    }
                                    _ => None,
                                });
                            return !matches!(first_arg_is_global_regex, Some(true));
                        }
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_types::Type;

    fn make_ctx() -> LoweringContext {
        LoweringContext::new("test.ts")
    }

    #[test]
    fn test_lower_define_and_lookup_local() {
        let mut ctx = make_ctx();
        let id = ctx.define_local("x".to_string(), Type::Number);
        assert_eq!(ctx.lookup_local("x"), Some(id));
        assert_eq!(ctx.lookup_local("y"), None);
        // Verify the type is stored correctly
        assert_eq!(ctx.lookup_local_type("x"), Some(&Type::Number));
    }

    #[test]
    fn test_lower_function_registration() {
        let mut ctx = make_ctx();
        let func_id = ctx.fresh_func();
        ctx.register_func("myFunc".to_string(), func_id);

        assert_eq!(ctx.lookup_func("myFunc"), Some(func_id));
        assert_eq!(ctx.lookup_func("nonExistent"), None);
        // Reverse lookup by id
        assert_eq!(ctx.lookup_func_name(func_id), Some("myFunc"));
    }

    #[test]
    fn test_lower_class_registration() {
        let mut ctx = make_ctx();
        let class_id = ctx.fresh_class();
        ctx.register_class("MyClass".to_string(), class_id);

        assert_eq!(ctx.lookup_class("MyClass"), Some(class_id));
        assert_eq!(ctx.lookup_class("Missing"), None);
    }

    #[test]
    fn test_lower_local_shadowing() {
        let mut ctx = make_ctx();
        let id1 = ctx.define_local("x".to_string(), Type::Number);
        let id2 = ctx.define_local("x".to_string(), Type::String);

        // lookup_local uses .rev() so the latest definition wins
        assert_eq!(ctx.lookup_local("x"), Some(id2));
        assert_ne!(id1, id2);

        // The shadowed type should be String (the latest)
        assert_eq!(ctx.lookup_local_type("x"), Some(&Type::String));

        // Both entries still exist in the vec
        assert_eq!(ctx.locals.len(), 2);
    }

    #[test]
    fn test_lower_function_shadowing() {
        let mut ctx = make_ctx();
        let id1 = ctx.fresh_func();
        let id2 = ctx.fresh_func();
        ctx.register_func("f".to_string(), id1);
        ctx.register_func("f".to_string(), id2);

        // lookup_func uses .rev() so the latest definition wins
        assert_eq!(ctx.lookup_func("f"), Some(id2));
    }

    #[test]
    fn test_lower_imported_function_registration() {
        let mut ctx = make_ctx();
        ctx.register_imported_func("myRead".to_string(), "readFileSync".to_string());

        assert_eq!(ctx.lookup_imported_func("myRead"), Some("readFileSync"));
        assert_eq!(ctx.lookup_imported_func("unknown"), None);
    }

    #[test]
    fn test_lower_builtin_module_alias() {
        let mut ctx = make_ctx();
        ctx.register_builtin_module_alias("myFs".to_string(), "fs".to_string());

        assert_eq!(ctx.lookup_builtin_module_alias("myFs"), Some("fs"));
        assert_eq!(ctx.lookup_builtin_module_alias("nope"), None);
    }

    #[test]
    fn test_lower_enum_registration_and_member_lookup() {
        let mut ctx = make_ctx();
        let enum_id = ctx.fresh_enum();
        ctx.define_enum(
            "Color".to_string(),
            enum_id,
            vec![
                ("Red".to_string(), EnumValue::Number(0)),
                ("Green".to_string(), EnumValue::Number(1)),
                ("Blue".to_string(), EnumValue::Number(2)),
            ],
        );

        let (looked_up_id, members) = ctx.lookup_enum("Color").unwrap();
        assert_eq!(looked_up_id, enum_id);
        assert_eq!(members.len(), 3);

        assert!(matches!(
            ctx.lookup_enum_member("Color", "Red"),
            Some(EnumValue::Number(0))
        ));
        assert!(ctx.lookup_enum_member("Color", "Yellow").is_none());
        assert!(ctx.lookup_enum("Missing").is_none());
    }

    #[test]
    fn test_lower_class_statics() {
        let mut ctx = make_ctx();
        ctx.register_class_statics(
            "MyClass".to_string(),
            vec!["count".to_string()],
            vec!["create".to_string()],
        );

        assert!(ctx.has_static_field("MyClass", "count"));
        assert!(!ctx.has_static_field("MyClass", "missing"));
        assert!(ctx.has_static_method("MyClass", "create"));
        assert!(!ctx.has_static_method("MyClass", "missing"));
        assert!(!ctx.has_static_field("Other", "count"));
    }

    #[test]
    fn test_lower_native_module_registration() {
        let mut ctx = make_ctx();
        // Namespace import: import * as fs from "fs"
        ctx.register_native_module("fs".to_string(), "fs".to_string(), None);
        // Named import: import { v4 as uuid } from "uuid"
        ctx.register_native_module(
            "uuid".to_string(),
            "uuid".to_string(),
            Some("v4".to_string()),
        );

        let (module, method) = ctx.lookup_native_module("fs").unwrap();
        assert_eq!(module, "fs");
        assert_eq!(method, None);

        let (module, method) = ctx.lookup_native_module("uuid").unwrap();
        assert_eq!(module, "uuid");
        assert_eq!(method, Some("v4"));

        assert!(ctx.lookup_native_module("missing").is_none());
    }

    #[test]
    fn test_lower_type_param_scoping() {
        let mut ctx = make_ctx();
        assert!(!ctx.is_type_param("T"));

        ctx.enter_type_param_scope(&[TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }]);
        assert!(ctx.is_type_param("T"));
        assert!(!ctx.is_type_param("U"));

        // Nested scope
        ctx.enter_type_param_scope(&[TypeParam {
            name: "U".to_string(),
            constraint: None,
            default: None,
        }]);
        assert!(ctx.is_type_param("T")); // outer scope still visible
        assert!(ctx.is_type_param("U"));

        ctx.exit_type_param_scope();
        assert!(ctx.is_type_param("T"));
        assert!(!ctx.is_type_param("U")); // inner scope gone

        ctx.exit_type_param_scope();
        assert!(!ctx.is_type_param("T")); // all scopes gone
    }

    #[test]
    fn test_lower_fresh_ids_increment() {
        let mut ctx = make_ctx();
        assert_eq!(ctx.fresh_local(), 0);
        assert_eq!(ctx.fresh_local(), 1);
        assert_eq!(ctx.fresh_local(), 2);

        assert_eq!(ctx.fresh_func(), 0);
        assert_eq!(ctx.fresh_func(), 1);

        // Classes start at 1 (default for new())
        assert_eq!(ctx.fresh_class(), 1);
        assert_eq!(ctx.fresh_class(), 2);
    }

    #[test]
    fn test_lower_namespace_var_lookup() {
        let mut ctx = make_ctx();
        let local_id = ctx.define_local("Utils_helper".to_string(), Type::Number);
        ctx.namespace_vars
            .push(("Utils".to_string(), "helper".to_string(), local_id));

        assert_eq!(ctx.lookup_namespace_var("Utils", "helper"), Some(local_id));
        assert_eq!(ctx.lookup_namespace_var("Utils", "missing"), None);
        assert_eq!(ctx.lookup_namespace_var("Other", "helper"), None);
    }
}
