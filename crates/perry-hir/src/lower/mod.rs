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
mod context;
mod expr_assign;
mod expr_call;
mod expr_function;
mod expr_member;
mod expr_misc;
mod expr_new;
mod expr_object;
mod unimpl_hints;
pub(crate) use context::*;
mod stmt;
pub(crate) use stmt::*;
mod module_decl;
pub(crate) use module_decl::*;
mod misc;
pub(crate) use misc::*;
mod pre_scan;
pub(crate) use pre_scan::*;
mod closure_analysis;
pub(crate) use closure_analysis::*;
mod decorators;
pub(crate) use decorators::*;
mod template;
pub(crate) use template::*;
mod widget_decl;
pub(crate) use widget_decl::*;

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

pub fn lower_module_with_class_id(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_and_types(ast_module, name, source_file_path, start_class_id, None)
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
