//! `LoweringContext` struct definition.
//!
//! Extracted from `lower/mod.rs` so the entry-point file stays under the
//! 2,000-LOC soft cap. The `impl LoweringContext { ... }` blocks live in
//! `lower/context.rs` (this file holds *only* the struct shape). All
//! field visibility stays `pub(crate)`; downstream code keeps reaching
//! the struct via `crate::lower::LoweringContext`.

use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};

use crate::ir::*;

pub struct LoweringContext {
    /// Counter for generating unique local IDs
    pub(crate) next_local_id: LocalId,
    /// Counter for generating unique global IDs
    // #854: initialized in `new` but not yet read by the lowerer (globals are
    // allocated through a different path today). Kept for the ID-counter set.
    #[allow(dead_code)]
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
    // #854: initialized in `new` but currently unread (globals tracked
    // elsewhere). Retained alongside `next_global_id` for the global table.
    #[allow(dead_code)]
    pub(crate) globals: Vec<(String, GlobalId, Type)>,
    /// Functions: name -> id
    pub(crate) functions: Vec<(String, FuncId)>,
    /// Function parameter defaults: func_id -> (defaults, param_local_ids)
    /// Per-function param-default info used by the call-site fill pass:
    /// `(func_id, [Option<default> per param], [LocalId per param], Option<rest_param_index>, has_synthetic_arguments)`.
    /// The rest-param index (if any) is the position of `...rest`; the fill
    /// loop must stop before it because rest params get bundled at runtime
    /// from trailing positional args, not filled with `undefined`.
    /// `has_synthetic_arguments` is true when the trailing rest param was
    /// inserted by `append_synthetic_arguments_param` because the body
    /// references the magic `arguments` identifier — in that case the call
    /// site must NOT pad missing fixed-param slots with `undefined`, because
    /// the codegen synth-args bundle path uses `args.len()` to size the
    /// runtime `arguments` array. Padding inflates `arguments.length` to
    /// the declared fixed-param count regardless of how many args the
    /// caller actually passed (issue #1069).
    pub(crate) func_defaults: Vec<(FuncId, Vec<Option<Expr>>, Vec<LocalId>, Option<usize>, bool)>,
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
    /// Stack of type parameter CONSTRAINTS, paired with `type_param_scopes`.
    /// `type_param_constraints[i]` maps type-parameter name → its declared
    /// upper-bound constraint (`<T extends string>` → `String`). Used to
    /// resolve a `Named(T)`/`TypeVar(T)` reference to the *runtime* type
    /// the value must satisfy, so callers and the body see e.g.
    /// `Type::String` instead of `Type::Named("T")`.
    ///
    /// Why this matters at lowering (not codegen): perry monomorphizes
    /// `FuncRef`-targeted generic calls (issue #321 scaffolding) but does
    /// NOT monomorphize closures/arrow functions and does NOT monomorphize
    /// generic functions reached through a function-typed local. The
    /// emitted body of such a function therefore lowers `self[0]` (for
    /// `<T extends string>(self: T)`) with no string-typing on `self`,
    /// and codegen falls through to `js_object_get_index_polymorphic`
    /// which reads a `StringHeader*` as `ArrayHeader*` and returns the
    /// header bytes as a subnormal f64 — surfacing as the `1.5E-323oo`
    /// pattern in effect's `Str.capitalize`/`Capitalize<T>` utilities.
    /// Resolving the constraint at the `TsTypeRef` site for `T` makes the
    /// param type `String` everywhere downstream, so the IndexGet/Member
    /// fast paths fire.
    pub(crate) type_param_constraints: Vec<HashMap<String, Type>>,
    /// Native class instances: local_name -> (module_name, class_name)
    /// Tracks variables that hold instances of native module classes (e.g., EventEmitter)
    pub(crate) native_instances: Vec<(String, String, String)>,
    /// #1483: type-only perry/ui widget import aliases — local_name ->
    /// canonical widget name. `import { type Canvas as CanvasType }` records
    /// `CanvasType -> Canvas` so a `canvas: CanvasType` parameter can be
    /// tagged as a perry/ui native instance (handle-based method dispatch),
    /// exactly like a `canvas: Canvas` param or a `const canvas = Canvas(...)`
    /// local. Type-only native specifiers are otherwise dropped at import.
    pub(crate) ui_widget_type_aliases: HashMap<String, String>,
    /// Current class being lowered (for arrow function `this` capture)
    pub(crate) current_class: Option<String>,
    /// Extern function types: name -> (param_types, return_type)
    /// Stores type information for declare function statements (FFI)
    pub(crate) extern_func_types: Vec<(String, Vec<Type>, Type)>,
    /// Source file path (for import.meta.url)
    pub(crate) source_file_path: String,
    /// Variables that hold closures or other values needing cross-module export globals
    /// (arrow functions, object literals, call expressions, arrays, new expressions)
    // #854: initialized in `new` but not yet read on this lowering path.
    #[allow(dead_code)]
    pub(crate) exportable_object_vars: HashSet<String>,
    /// Functions created during expression lowering (e.g., object literal methods)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_functions: Vec<Function>,
    /// Issue #2076: display-name overrides keyed by FuncId. Populated for
    /// named function expressions (own ident) and object-literal methods
    /// (static property key); flushed into `Module.closure_display_names`
    /// alongside `pending_functions`.
    pub(crate) closure_display_names: HashMap<FuncId, String>,
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
    /// #1723 — one-shot flag set by an enclosing `ns[dynamicKey].staticMember`
    /// access to tell the *immediately-nested* computed-member lowering to skip
    /// the #503 dynamic-stdlib-dispatch refusal. The dynamic index there only
    /// selects a stdlib SUB-namespace (e.g. `path.win32` / `path.posix`) and the
    /// actual member is a source-visible static name, so it is auditable — not
    /// the `ns[runtimeVar]()` obfuscation #503 targets. The guard consumes
    /// (clears) it on read so a dynamic key *inside the index* (`ns[fs[evil]]`)
    /// is still refused.
    pub(crate) suppress_stdlib_dispatch_guard_once: bool,
    pub(crate) var_hoisted_ids: HashSet<LocalId>,
    /// Shadow index: function name -> index in `functions` Vec (last entry for shadowing)
    pub(crate) functions_index: HashMap<String, usize>,
    /// Shadow index: class name -> index in `classes` Vec
    pub(crate) classes_index: HashMap<String, usize>,
    /// Shadow index: local import name -> index in `imported_functions` Vec
    pub(crate) imported_functions_index: HashMap<String, usize>,
    /// Shadow index: local alias name -> index in `builtin_module_aliases` Vec
    pub(crate) builtin_module_aliases_index: HashMap<String, usize>,
    /// Local names bound to a `path` sub-namespace (`const w = path.win32`).
    /// Maps the local name -> (root identifier name, sub "win32"|"posix").
    /// Resolution of the root identifier to the `path` module is deferred to
    /// call-lowering time because imports aren't registered yet during the
    /// pre-scan that populates this; see `try_path_subnamespace`. #1750.
    pub(crate) subns_path_aliases: HashMap<String, (String, String)>,
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
    /// Local names bound by a wildcard namespace import (`import * as X from
    /// "mod"`). These are module namespaces, NOT classes — so `X.member(args)`
    /// must resolve+call the namespace member, never lower to a
    /// `StaticMethodCall`. The uppercase-imported-identifier class heuristic in
    /// `expr_call::static_and_instance` consults this set to skip namespaces
    /// (an uppercase `import * as Effect` would otherwise be mistaken for a
    /// class and the member call would return the function uncalled). Named /
    /// default imports of actual classes (`import { MongoClient }`) are NOT in
    /// this set and keep the static-method path.
    pub(crate) namespace_import_locals: HashSet<String>,
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
    // #854: initialized in `new` but unread — anon-shape classes are now named
    // by content-addressed FNV hash (see `synthesize_anon_shape_class`), not by
    // this counter. Kept for the struct's field set.
    #[allow(dead_code)]
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
