//! AST to HIR lowering
//!
//! Converts SWC's TypeScript AST into our HIR representation.

use anyhow::{anyhow, Result};
use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};
use swc_ecma_ast as ast;
use std::collections::{HashMap, HashSet};

use crate::ir::*;

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
    pub(crate) func_defaults: Vec<(FuncId, Vec<Option<Expr>>, Vec<LocalId>)>,
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
}

impl LoweringContext {
    pub fn new(source_file_path: impl Into<String>) -> Self {
        Self::with_class_id_start(source_file_path, 1)
    }

    pub fn with_class_id_start(source_file_path: impl Into<String>, start_class_id: ClassId) -> Self {
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
            proxy_revoke_locals: HashMap::new(),
            proxy_target_classes: HashMap::new(),
            class_expr_aliases: HashMap::new(),
            in_constructor_class: None,
            mixin_funcs: HashMap::new(),
            anon_shape_classes: HashMap::new(),
            next_anon_shape_id: 0,
            class_method_return_types: Vec::new(),
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
        self.type_param_scopes.iter().any(|scope| scope.contains(name))
    }

    /// Look up a type alias by name and return its resolved type (if found).
    /// This is used during type extraction to resolve type aliases like
    /// `type BlockTag = 'latest' | number | string` so the compiler sees
    /// the underlying Union type instead of Named("BlockTag").
    pub(crate) fn resolve_type_alias(&self, name: &str) -> Option<perry_types::Type> {
        self.type_aliases.iter()
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
fn extract_typed_parse_source_order(
    ts_type: &swc_ecma_ast::TsType,
    ctx: &LoweringContext,
) -> Option<Vec<String>> {
    use swc_ecma_ast as ast;
    match ts_type {
        ast::TsType::TsArrayType(arr) => {
            extract_typed_parse_source_order(&arr.elem_type, ctx)
        }
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
            if keys.is_empty() { None } else { Some(keys) }
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
fn resolve_typed_parse_ty(ctx: &LoweringContext, ty: Type) -> Type {
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

    pub(crate) fn fresh_global(&mut self) -> GlobalId {
        let id = self.next_global_id;
        self.next_global_id += 1;
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
    pub(crate) fn maybe_wrap_builtin_callback(&mut self, cb: Expr, ast_arg: &swc_ecma_ast::ExprOrSpread) -> Expr {
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

    /// Register declared instance field names for a class. Used by subclasses to skip
    /// re-declaring inherited fields when inferring from ctor body `this.x = ...` assignments.
    pub(crate) fn register_class_field_names(&mut self, class_name: String, field_names: Vec<String>) {
        // Replace existing entry if present; otherwise append.
        if let Some(entry) = self.class_field_names.iter_mut().find(|(n, _)| *n == class_name) {
            entry.1 = field_names;
        } else {
            self.class_field_names.push((class_name, field_names));
        }
    }

    /// Look up the list of instance field names declared on a class (NOT including inherited).
    pub(crate) fn lookup_class_field_names(&self, class_name: &str) -> Option<&[String]> {
        self.class_field_names.iter().find(|(n, _)| n == class_name).map(|(_, f)| f.as_slice())
    }

    pub(crate) fn register_class_statics(&mut self, class_name: String, static_fields: Vec<String>, static_methods: Vec<String>) {
        self.class_statics.push((class_name, static_fields, static_methods));
    }

    pub(crate) fn has_static_field(&self, class_name: &str, field_name: &str) -> bool {
        self.class_statics.iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, fields, _)| fields.contains(&field_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn has_static_method(&self, class_name: &str, method_name: &str) -> bool {
        self.class_statics.iter()
            .find(|(cn, _, _)| cn == class_name)
            .map(|(_, _, methods)| methods.contains(&method_name.to_string()))
            .unwrap_or(false)
    }

    pub(crate) fn lookup_namespace_var(&self, ns_name: &str, member_name: &str) -> Option<LocalId> {
        self.namespace_vars.iter()
            .find(|(ns, member, _)| ns == ns_name && member == member_name)
            .map(|(_, _, id)| *id)
    }

    pub(crate) fn define_enum(&mut self, name: String, id: EnumId, members: Vec<(String, EnumValue)>) {
        self.enums.push((name, id, members));
    }

    pub(crate) fn lookup_enum(&self, name: &str) -> Option<(EnumId, &[(String, EnumValue)])> {
        self.enums.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, members)| (*id, members.as_slice()))
    }

    pub(crate) fn lookup_enum_member(&self, enum_name: &str, member_name: &str) -> Option<&EnumValue> {
        self.enums.iter()
            .find(|(n, _, _)| n == enum_name)
            .and_then(|(_, _, members)| {
                members.iter()
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
        captures.into_iter()
            .filter(|id| !self.module_level_ids.contains(id))
            .collect()
    }

    pub(crate) fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.locals.iter().rev().find(|(n, _, _)| n == name).map(|(_, id, _)| *id)
    }

    pub(crate) fn lookup_local_type(&self, name: &str) -> Option<&Type> {
        self.locals.iter().rev().find(|(n, _, _)| n == name).map(|(_, _, ty)| ty)
    }

    pub(crate) fn lookup_func(&self, name: &str) -> Option<FuncId> {
        self.functions_index.get(name).map(|&idx| self.functions[idx].1)
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

        let anon_id = self.next_anon_shape_id;
        self.next_anon_shape_id += 1;
        let class_name = format!("__AnonShape_{}", anon_id);
        let class_id = self.fresh_class();

        // Fields have `init: None` — each literal's values are passed as
        // positional constructor args, so the class stays shape-only (no
        // per-literal state). See the method doc comment for why this
        // matters under shape-deduplication.
        let fields: Vec<ClassField> = fields_with_types
            .iter()
            .map(|(name, ty, _init_expr_unused)| ClassField {
                name: name.clone(),
                ty: ty.clone(),
                init: None,
                is_private: false,
                is_readonly: false,
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
            fields,
            constructor: Some(constructor),
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            is_exported: false,
        });

        self.anon_shape_classes.insert(shape_key, class_name.clone());
        class_name
    }

    pub(crate) fn lookup_func_name(&self, func_id: FuncId) -> Option<&str> {
        self.functions.iter().find(|(_, id)| *id == func_id).map(|(name, _)| name.as_str())
    }

    pub(crate) fn lookup_func_defaults(&self, func_id: FuncId) -> Option<(&[Option<Expr>], &[LocalId])> {
        self.func_defaults.iter()
            .find(|(id, _, _)| *id == func_id)
            .map(|(_, defaults, param_ids)| (defaults.as_slice(), param_ids.as_slice()))
    }

    /// Substitute parameter references in a default expression.
    /// Replaces LocalGet(callee_param_id) with the corresponding caller argument expression.
    pub(crate) fn substitute_param_refs_in_default(expr: &Expr, param_map: &[(LocalId, Expr)]) -> Expr {
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
            Expr::Array(elements) => {
                Expr::Array(elements.iter().map(|e| Self::substitute_param_refs_in_default(e, param_map)).collect())
            }
            Expr::Object(fields) => {
                Expr::Object(fields.iter().map(|(k, v)| (k.clone(), Self::substitute_param_refs_in_default(v, param_map))).collect())
            }
            Expr::Binary { op, left, right } => {
                Expr::Binary {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Compare { op, left, right } => {
                Expr::Compare {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Logical { op, left, right } => {
                Expr::Logical {
                    op: *op,
                    left: Box::new(Self::substitute_param_refs_in_default(left, param_map)),
                    right: Box::new(Self::substitute_param_refs_in_default(right, param_map)),
                }
            }
            Expr::Unary { op, operand } => {
                Expr::Unary {
                    op: *op,
                    operand: Box::new(Self::substitute_param_refs_in_default(operand, param_map)),
                }
            }
            Expr::Call { callee, args, type_args } => {
                Expr::Call {
                    callee: Box::new(Self::substitute_param_refs_in_default(callee, param_map)),
                    args: args.iter().map(|a| Self::substitute_param_refs_in_default(a, param_map)).collect(),
                    type_args: type_args.clone(),
                }
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                Expr::Conditional {
                    condition: Box::new(Self::substitute_param_refs_in_default(condition, param_map)),
                    then_expr: Box::new(Self::substitute_param_refs_in_default(then_expr, param_map)),
                    else_expr: Box::new(Self::substitute_param_refs_in_default(else_expr, param_map)),
                }
            }
            Expr::PropertyGet { object, property } => {
                Expr::PropertyGet {
                    object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                    property: property.clone(),
                }
            }
            Expr::IndexGet { object, index } => {
                Expr::IndexGet {
                    object: Box::new(Self::substitute_param_refs_in_default(object, param_map)),
                    index: Box::new(Self::substitute_param_refs_in_default(index, param_map)),
                }
            }
            Expr::New { class_name, args, type_args } => {
                Expr::New {
                    class_name: class_name.clone(),
                    args: args.iter().map(|a| Self::substitute_param_refs_in_default(a, param_map)).collect(),
                    type_args: type_args.clone(),
                }
            }
            // Leaf expressions that don't contain LocalGet - return as-is
            _ => expr.clone(),
        }
    }

    pub(crate) fn lookup_imported_func(&self, name: &str) -> Option<&str> {
        self.imported_functions_index.get(name).map(|&idx| self.imported_functions[idx].1.as_str())
    }

    pub(crate) fn register_imported_func(&mut self, local_name: String, original_name: String) {
        let idx = self.imported_functions.len();
        self.imported_functions_index.insert(local_name.clone(), idx);
        self.imported_functions.push((local_name, original_name));
    }

    pub(crate) fn register_extern_func_types(&mut self, name: String, param_types: Vec<Type>, return_type: Type) {
        self.extern_func_types.push((name, param_types, return_type));
    }

    pub(crate) fn lookup_extern_func_types(&self, name: &str) -> Option<(&Vec<Type>, &Type)> {
        self.extern_func_types
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, params, ret)| (params, ret))
    }

    pub(crate) fn register_native_module(&mut self, local_name: String, module_name: String, method_name: Option<String>) {
        self.native_modules.push((local_name, module_name, method_name));
    }

    pub(crate) fn lookup_native_module(&self, name: &str) -> Option<(&str, Option<&str>)> {
        self.native_modules.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, m, method)| (m.as_str(), method.as_ref().map(|s| s.as_str())))
    }

    pub(crate) fn register_builtin_module_alias(&mut self, local_name: String, module_name: String) {
        let idx = self.builtin_module_aliases.len();
        self.builtin_module_aliases_index.insert(local_name.clone(), idx);
        self.builtin_module_aliases.push((local_name, module_name));
    }

    pub(crate) fn lookup_builtin_module_alias(&self, name: &str) -> Option<&str> {
        self.builtin_module_aliases_index.get(name).map(|&idx| self.builtin_module_aliases[idx].1.as_str())
    }

    pub(crate) fn register_native_instance(&mut self, local_name: String, module_name: String, class_name: String) {
        self.native_instances.push((local_name, module_name, class_name));
    }

    pub(crate) fn lookup_native_instance(&self, name: &str) -> Option<(&str, &str)> {
        // Check scoped instances first (function-local variables)
        self.native_instances.iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
            .or_else(|| {
                // Check module-level instances (survive scope exits)
                self.module_native_instances.iter()
                    .find(|(n, _, _)| n == name)
                    .map(|(_, module, class)| (module.as_str(), class.as_str()))
            })
    }

    pub(crate) fn lookup_func_return_native_instance(&self, func_name: &str) -> Option<(&str, &str)> {
        self.func_return_native_instances.iter()
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
        Type::Generic { base, type_args } if base == "Promise" => {
            type_args.first().unwrap_or(ty)
        }
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
        self.func_return_types.iter().rev()
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
        self.class_method_return_types.push((class_name, method_name, ty));
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
        self.class_method_return_types.iter().rev()
            .find(|(c, m, _)| c == class_name && m == method_name)
            .map(|(_, _, ty)| ty)
    }

    pub(crate) fn enter_scope(&mut self) -> (usize, usize, usize) {
        // Function/closure boundary: new locals are no longer module-level.
        self.scope_depth += 1;
        (self.locals.len(), self.native_instances.len(), self.functions.len())
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
        debug_assert!(self.inside_block_scope > 0, "pop_block_scope without matching push");
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
pub(crate) use crate::lower_types::*;
pub(crate) use crate::lower_patterns::*;
pub(crate) use crate::destructuring::*;
pub(crate) use crate::lower_decl::*;
pub(crate) use crate::analysis::*;
pub(crate) use crate::jsx::*;

pub fn lower_module(ast_module: &ast::Module, name: &str, source_file_path: &str) -> Result<Module> {
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
        callee: Box::new(Expr::PropertyGet { object: obj, property: prop }),
        args,
        type_args: Vec::new(),
    };
    match property.as_str() {
        "map" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayMap { array: object, callback: Box::new(cb) }
        }
        "filter" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFilter { array: object, callback: Box::new(cb) }
        }
        "forEach" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayForEach { array: object, callback: Box::new(cb) }
        }
        "find" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFind { array: object, callback: Box::new(cb) }
        }
        "findIndex" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindIndex { array: object, callback: Box::new(cb) }
        }
        "findLast" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLast { array: object, callback: Box::new(cb) }
        }
        "findLastIndex" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayFindLastIndex { array: object, callback: Box::new(cb) }
        }
        "some" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArraySome { array: object, callback: Box::new(cb) }
        }
        "every" if args.len() >= 1 => {
            let cb = args.into_iter().next().unwrap();
            Expr::ArrayEvery { array: object, callback: Box::new(cb) }
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
        "keys" | "values" | "entries" | "fromEntries" | "assign" | "is"
        | "hasOwn" | "freeze" | "seal" | "preventExtensions" | "create"
        | "isFrozen" | "isSealed" | "isExtensible" | "getPrototypeOf"
        | "setPrototypeOf" | "defineProperty" | "defineProperties"
        | "getOwnPropertyDescriptor" | "getOwnPropertyDescriptors"
        | "getOwnPropertyNames" | "getOwnPropertySymbols" | "groupBy"
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
    fn unwrap_init<'a>(mut e: &'a ast::Expr) -> &'a ast::Expr {
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
                    Some("WeakRef") => { ctx.weakref_locals.insert(name); }
                    Some("FinalizationRegistry") => { ctx.finreg_locals.insert(name); }
                    Some("WeakMap") => { ctx.weakmap_locals.insert(name); }
                    Some("WeakSet") => { ctx.weakset_locals.insert(name); }
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
        ctx.mixin_funcs.insert(
            fn_name,
            (param_name, Box::new((*class_expr.class).clone())),
        );
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

pub fn lower_module_with_class_id(ast_module: &ast::Module, name: &str, source_file_path: &str, start_class_id: ClassId) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_and_types(ast_module, name, source_file_path, start_class_id, None)
}

pub fn lower_module_with_class_id_and_types(ast_module: &ast::Module, name: &str, source_file_path: &str, start_class_id: ClassId, resolved_types: Option<std::collections::HashMap<u32, Type>>) -> Result<(Module, ClassId)> {
    let mut ctx = LoweringContext::with_class_id_start(source_file_path, start_class_id);
    ctx.resolved_types = resolved_types;
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
                ImportSpecifier::Named { local: "__jsx".to_string(), imported: "jsx".to_string() },
                ImportSpecifier::Named { local: "__jsxs".to_string(), imported: "jsxs".to_string() },
            ],
            is_native: false,
            module_kind: ModuleKind::NativeCompiled,
            resolved_path: None,
        });
    }

    // Pre-scan: Find all function names that have implementations (bodies)
    // This is needed to properly handle TypeScript function overloads where
    // multiple signature-only declarations precede a single implementation
    let mut functions_with_bodies: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                let param_types: Vec<Type> = fn_decl.function.params.iter()
                    .map(|param| extract_param_type_with_ctx(&param.pat, None))
                    .collect();

                // Extract return type
                let return_type = fn_decl.function.return_type.as_ref()
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
                        let ty = ident.type_ann.as_ref()
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
                        static_method_names.push(format!("#{}", method.key.name.to_string()));
                    }
                    ast::ClassMember::ClassProp(prop) if prop.is_static => {
                        if let ast::PropName::Ident(ident) = &prop.key {
                            static_field_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                        static_field_names.push(format!("#{}", prop.key.name.to_string()));
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
            module.classes.push(class);
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
        let is_exported = module.functions.iter().any(|f| f.name == *func_name && f.is_exported);
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

    Ok((module, ctx.next_class_id))
}

/// Post-lowering pass that widens every `Expr::Closure`'s `mutable_captures`
/// to include any capture that is assigned to inside a sibling closure in the
/// same lexical scope. Then recurses into each closure body so nested scopes
/// get the same treatment. This ensures that when multiple closures share a
/// captured binding and any one of them mutates it, all of them treat it as
/// boxed so reads and writes observe the same storage slot.
fn widen_mutable_captures_stmts(stmts: &mut [Stmt]) {
    let mut scope_mutable: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for stmt in stmts.iter() {
        collect_closure_assigned_stmt(stmt, &mut scope_mutable);
    }
    // Also detect variables that are captured by closures AND assigned at the
    // scope level (not inside a closure). This handles the pattern:
    //   let x = 0;
    //   fns.push(() => x);
    //   x = 10;               // assignment at scope level
    //   fns.push(() => x);
    // All closures should see the final value of x (capture-by-reference).
    let mut scope_captured: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for stmt in stmts.iter() {
        collect_closure_captures_stmt(stmt, &mut scope_captured);
    }
    let mut scope_assigned_at_level: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for stmt in stmts.iter() {
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

fn widen_mutable_captures_stmt(stmt: &mut Stmt, scope_mutable: &std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { init: Some(expr), .. } => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Expr(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Return(Some(expr)) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Throw(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::If { condition, then_branch, else_branch } => {
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
        Stmt::For { init, condition, update, body } => {
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
        Stmt::Try { body, catch, finally } => {
            widen_mutable_captures_stmts(body);
            if let Some(catch_clause) = catch {
                widen_mutable_captures_stmts(&mut catch_clause.body);
            }
            if let Some(finally_stmts) = finally {
                widen_mutable_captures_stmts(finally_stmts);
            }
        }
        Stmt::Switch { discriminant, cases } => {
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

fn widen_mutable_captures_expr(expr: &mut Expr, scope_mutable: &std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { captures, mutable_captures, body, .. } => {
            let mut mset: std::collections::HashSet<LocalId> = mutable_captures.iter().copied().collect();
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
                    CallArg::Expr(e) | CallArg::Spread(e) => widen_mutable_captures_expr(e, scope_mutable),
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
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => widen_mutable_captures_expr(x, scope_mutable),
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
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
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
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
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
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            widen_mutable_captures_expr(delete_count, scope_mutable);
            for item in items {
                widen_mutable_captures_expr(item, scope_mutable);
            }
        }
        Expr::ArrayWith { array, index, value } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
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
        Expr::JsCreateCallback { closure, .. } => widen_mutable_captures_expr(closure, scope_mutable),
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
        Stmt::Let { init: Some(expr), .. } => collect_closure_assigned_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::If { condition, then_branch, else_branch } => {
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
        Stmt::For { init, condition, update, body } => {
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
        Stmt::Try { body, catch, finally } => {
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
        Stmt::Switch { discriminant, cases } => {
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
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => collect_closure_assigned_expr(x, out),
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
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
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
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
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
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(start, out);
            collect_closure_assigned_expr(delete_count, out);
            for item in items {
                collect_closure_assigned_expr(item, out);
            }
        }
        Expr::ArrayWith { array, index, value } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(index, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
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
        Stmt::Let { init: Some(expr), .. } => collect_closure_captures_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_captures_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_captures_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_captures_expr(expr, out),
        Stmt::If { condition, then_branch, else_branch } => {
            collect_closure_captures_expr(condition, out);
            for s in then_branch { collect_closure_captures_stmt(s, out); }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts { collect_closure_captures_stmt(s, out); }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_captures_expr(condition, out);
            for s in body { collect_closure_captures_stmt(s, out); }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init { collect_closure_captures_stmt(init_stmt, out); }
            if let Some(cond) = condition { collect_closure_captures_expr(cond, out); }
            if let Some(upd) = update { collect_closure_captures_expr(upd, out); }
            for s in body { collect_closure_captures_stmt(s, out); }
        }
        Stmt::Try { body, catch, finally } => {
            for s in body { collect_closure_captures_stmt(s, out); }
            if let Some(cc) = catch { for s in &cc.body { collect_closure_captures_stmt(s, out); } }
            if let Some(fs) = finally { for s in fs { collect_closure_captures_stmt(s, out); } }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_closure_captures_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test { collect_closure_captures_expr(test, out); }
                for s in &case.body { collect_closure_captures_stmt(s, out); }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_captures_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_captures_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { captures, body, .. } => {
            for id in captures { out.insert(*id); }
            // Also recurse into nested closures
            for stmt in body { collect_closure_captures_stmt(stmt, out); }
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
            for arg in args { collect_closure_captures_expr(arg, out); }
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
            for e in elements { collect_closure_captures_expr(e, out); }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => collect_closure_captures_expr(x, out),
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields { collect_closure_captures_expr(v, out); }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts { collect_closure_captures_expr(v, out); }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => {
            for arg in args { collect_closure_captures_expr(arg, out); }
        }
        Expr::ArrayPush { value, .. } | Expr::Await(value) | Expr::TypeOf(value)
        | Expr::Void(value) | Expr::Delete(value) => {
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
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(callback, out);
            if let Some(init) = initial { collect_closure_captures_expr(init, out); }
        }
        Expr::ArrayToReversed { array } => {
            collect_closure_captures_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_captures_expr(array, out);
            if let Some(cmp) = comparator { collect_closure_captures_expr(cmp, out); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(start, out);
            collect_closure_captures_expr(delete_count, out);
            for item in items { collect_closure_captures_expr(item, out); }
        }
        Expr::ArrayWith { array, index, value } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            collect_closure_captures_expr(target, out);
            collect_closure_captures_expr(start, out);
            if let Some(e) = end { collect_closure_captures_expr(e, out); }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_captures_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object { collect_closure_captures_expr(obj, out); }
            for arg in args { collect_closure_captures_expr(arg, out); }
        }
        Expr::JsCreateCallback { closure, .. } => collect_closure_captures_expr(closure, out),
        Expr::Sequence(exprs) => {
            for e in exprs { collect_closure_captures_expr(e, out); }
        }
        _ => {}
    }
}

/// Collect LocalIds that are assigned to at the current scope level
/// (via LocalSet or Update), but NOT inside closure bodies.
fn collect_scope_level_assigns_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { init: Some(expr), .. } => collect_scope_level_assigns_expr(expr, out),
        Stmt::Expr(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Throw(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::If { condition, then_branch, else_branch } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in then_branch { collect_scope_level_assigns_stmt(s, out); }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts { collect_scope_level_assigns_stmt(s, out); }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in body { collect_scope_level_assigns_stmt(s, out); }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init { collect_scope_level_assigns_stmt(init_stmt, out); }
            if let Some(cond) = condition { collect_scope_level_assigns_expr(cond, out); }
            if let Some(upd) = update { collect_scope_level_assigns_expr(upd, out); }
            for s in body { collect_scope_level_assigns_stmt(s, out); }
        }
        Stmt::Try { body, catch, finally } => {
            for s in body { collect_scope_level_assigns_stmt(s, out); }
            if let Some(cc) = catch { for s in &cc.body { collect_scope_level_assigns_stmt(s, out); } }
            if let Some(fs) = finally { for s in fs { collect_scope_level_assigns_stmt(s, out); } }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_scope_level_assigns_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test { collect_scope_level_assigns_expr(test, out); }
                for s in &case.body { collect_scope_level_assigns_stmt(s, out); }
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
            for arg in args { collect_scope_level_assigns_expr(arg, out); }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_scope_level_assigns_expr(condition, out);
            collect_scope_level_assigns_expr(then_expr, out);
            collect_scope_level_assigns_expr(else_expr, out);
        }
        _ => {}
    }
}

/// Walk a closure body collecting every LocalSet/Update target AND any
/// assigns inside nested closures within this body.
fn collect_closure_assigned_in_closure_body_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { init: Some(expr), .. } => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::If { condition, then_branch, else_branch } => {
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
        Stmt::For { init, condition, update, body } => {
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
        Stmt::Try { body, catch, finally } => {
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
        Stmt::Switch { discriminant, cases } => {
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

fn collect_closure_assigned_in_body_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
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
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_closure_assigned_in_body_expr(e, out),
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
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => collect_closure_assigned_in_body_expr(x, out),
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
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
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
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
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
            if let Some(cmp) = comparator { collect_closure_assigned_in_body_expr(cmp, out); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(start, out);
            collect_closure_assigned_in_body_expr(delete_count, out);
            for item in items { collect_closure_assigned_in_body_expr(item, out); }
        }
        Expr::ArrayWith { array, index, value } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(index, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            collect_closure_assigned_in_body_expr(target, out);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(e) = end { collect_closure_assigned_in_body_expr(e, out); }
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
        Expr::JsCreateCallback { closure, .. } => collect_closure_assigned_in_body_expr(closure, out),
        // Array mutation methods may reallocate the array pointer, so they
        // count as assignments to the array_id for mutable-capture widening.
        Expr::ArrayPush { array_id, value } | Expr::ArrayUnshift { array_id, value } | Expr::ArrayPushSpread { array_id, source: value } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            out.insert(*array_id);
        }
        Expr::ArraySplice { array_id, start, delete_count, items } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(dc) = delete_count { collect_closure_assigned_in_body_expr(dc, out); }
            for item in items { collect_closure_assigned_in_body_expr(item, out); }
        }
        _ => {}
    }
}

fn lower_module_decl(
    ctx: &mut LoweringContext,
    module: &mut Module,
    decl: &ast::ModuleDecl,
) -> Result<()> {
    match decl {
        ast::ModuleDecl::Import(import_decl) => {
            // Skip type-only imports (import type { ... } from '...') - they have no runtime value
            if import_decl.type_only {
                return Ok(());
            }

            // Get the source module path
            let raw_source = import_decl.src.value.as_str().unwrap_or("").to_string();
            // Normalize "node:" prefix (e.g., "node:async_hooks" -> "async_hooks")
            let source = raw_source.strip_prefix("node:").unwrap_or(&raw_source).to_string();

            // Check if this is a native module import
            let is_native = is_native_module(&source);

            // Parse import specifiers
            let mut specifiers = Vec::new();
            for spec in &import_decl.specifiers {
                match spec {
                    ast::ImportSpecifier::Named(named) => {
                        // Skip individual type-only specifiers (import { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = named.local.sym.to_string();
                        let imported = named.imported
                            .as_ref()
                            .map(|i| match i {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        if is_native {
                            // Register as native module function with the original method name
                            // e.g., import { v4 as uuid } from 'uuid' -> uuid maps to uuid.v4
                            ctx.register_native_module(local.clone(), source.clone(), Some(imported.clone()));
                            // Auto-register parentPort from worker_threads as a native instance
                            // (it's a singleton, not created via `new`)
                            if source == "worker_threads" && imported == "parentPort" {
                                ctx.register_native_instance(local.clone(), "worker_threads".to_string(), "MessagePort".to_string());
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
                            // Default import from JS module - register so calls resolve to ExternFuncRef
                            // Use "default" as the original name since default imports map to the "default" export
                            ctx.register_imported_func(local.clone(), "default".to_string());
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
                    if let Some((module, class)) = native_instance_from_return_type(&func.return_type) {
                        ctx.func_return_native_instances.push((
                            func_name.clone(), module.to_string(), class.to_string()
                        ));
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> = func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    ctx.func_defaults.push((func.id, defaults, param_ids));
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
                            // Check if this is a native class instantiation and register it
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name = class_ident.sym.as_ref();
                                    // Map class names to their modules
                                    let module_name = match class_name {
                                        "EventEmitter" => Some("events"),
                                        "AsyncLocalStorage" => Some("async_hooks"),
                                        "WebSocket" | "WebSocketServer" => Some("ws"),
                                        "Redis" => Some("ioredis"),
                                        "LRUCache" => Some("lru-cache"),
                                        "Command" => Some("commander"),
                                        "Big" => Some("big.js"),
                                        "Decimal" => Some("decimal.js"),
                                        "BigNumber" => Some("bignumber.js"),
                                        // Database clients
                                        "Pool" => Some("pg"),
                                        "Client" => Some("pg"),
                                        "MongoClient" => Some("mongodb"),
                                        _ => None,
                                    };
                                    if let Some(native_module) = module_name {
                                        ctx.register_native_instance(name.clone(), native_module.to_string(), class_name.to_string());
                                    }
                                }
                            }

                            // Check if this is an awaited native class instantiation (e.g., await new Redis())
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                        let class_name = class_ident.sym.as_ref();
                                        // Map class names to their modules
                                        let module_name = match class_name {
                                            "EventEmitter" => Some("events"),
                                            "AsyncLocalStorage" => Some("async_hooks"),
                                            "WebSocket" | "WebSocketServer" => Some("ws"),
                                            "Redis" => Some("ioredis"),
                                            "LRUCache" => Some("lru-cache"),
                                            "Command" => Some("commander"),
                                            "Big" => Some("big.js"),
                                            "Decimal" => Some("decimal.js"),
                                            "BigNumber" => Some("bignumber.js"),
                                            // Database clients
                                            "Pool" => Some("pg"),
                                            "Client" => Some("pg"),
                                            "MongoClient" => Some("mongodb"),
                                            _ => None,
                                        };
                                        if let Some(native_module) = module_name {
                                            ctx.register_native_instance(name.clone(), native_module.to_string(), class_name.to_string());
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
                                            let native_mod = ctx.lookup_native_module(obj_name)
                                                .map(|(m, _)| m.to_string());
                                            if let Some(module_name_owned) = native_mod {
                                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                    let method_name = method_ident.sym.as_ref();
                                                    // Map factory functions to their class names
                                                    let class_name = match (module_name_owned.as_str(), method_name) {
                                                        ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                                        ("mysql2" | "mysql2/promise", "createConnection") => Some("Connection"),
                                                        ("pg", "connect") => Some("Client"),
                                                        ("http" | "https", "request" | "get") => Some("ClientRequest"),
                                                        // net.createConnection(host, port) returns a Socket handle.
                                                        // Without registering this, subsequent `sock.write/on/end/destroy`
                                                        // calls fall through to dynamic dispatch and never reach
                                                        // the `js_net_socket_*` FFI functions.
                                                        ("net", "createConnection") => Some("Socket"),
                                                        // node-cron's `cron.schedule(expr, cb)` returns a job
                                                        // handle whose `start()`/`stop()`/`isRunning()` etc.
                                                        // dispatch via the ("node-cron", true, METHOD) entries
                                                        // in expr.rs's native_module dispatch table. Without
                                                        // registering the handle as a "CronJob" native instance,
                                                        // `job.stop()` falls through to dynamic dispatch and the
                                                        // stop never reaches js_cron_job_stop.
                                                        ("node-cron", "schedule") => Some("CronJob"),
                                                        _ => None,
                                                    };
                                                    if let Some(class_name) = class_name {
                                                        ctx.register_native_instance(name.clone(), module_name_owned.clone(), class_name.to_string());
                                                        // Also register as module-level native instance so it survives scope exits.
                                                        // Without this, pool = mysql.createPool() at module top level loses
                                                        // its native tracking when function scopes are entered/exited,
                                                        // causing pool.query() inside functions to miss the Pool dispatch.
                                                        ctx.module_native_instances.push((name.clone(), module_name_owned, class_name.to_string()));
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
                                        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                                            // Register as native instance - the "class" is the module name for default exports
                                            ctx.register_native_instance(name.clone(), module_name.to_string(), "App".to_string());
                                        }
                                        // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                                        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
                                            if module_name == "perry/ui" {
                                                match method_name {
                                                    "State" | "Sheet" | "Toolbar" | "Window" | "LazyVStack"
                                                    | "NavigationStack" | "Picker" | "Table" | "TabBar" => {
                                                        ctx.register_native_instance(name.clone(), module_name.to_string(), method_name.to_string());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                                let obj_name = obj_ident.sym.as_ref();
                                                if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                        let class_name = match (module_name, method_ident.sym.as_ref()) {
                                                            ("mongodb", "connect") => Some("MongoClient"),
                                                            ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                                            ("mysql2" | "mysql2/promise", "createConnection") => Some("Connection"),
                                                            ("pg", "connect") => Some("Client"),
                                                            ("http" | "https", "request" | "get") => Some("ClientRequest"),
                                                            ("axios", "get" | "post" | "put" | "delete" | "patch" | "request") => Some("Response"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = class_name {
                                                            ctx.register_native_instance(name.clone(), module_name.to_string(), class_name.to_string());
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
                                    let native_info = ctx.lookup_native_module(class_name_str)
                                        .map(|(m, _)| m.to_string());
                                    if let Some(module_name) = native_info {
                                        ctx.register_native_instance(name.clone(), module_name.clone(), class_name_str.to_string());
                                        ctx.module_native_instances.push((name.clone(), module_name, class_name_str.to_string()));
                                    }
                                }
                            }

                            // Check if this is a method call on a registered native instance (chaining).
                            // e.g., const db = client.db(name) where client is a mongodb native instance.
                            {
                                // Unwrap await if present
                                let actual_init = if let ast::Expr::Await(await_expr) = init.as_ref() {
                                    await_expr.arg.as_ref()
                                } else {
                                    init.as_ref()
                                };
                                if let ast::Expr::Call(call_expr) = actual_init {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                                let obj_name = obj_ident.sym.to_string();
                                                if let Some((module_name, _class)) = ctx.lookup_native_instance(&obj_name)
                                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                        let method_name = method_ident.sym.as_ref();
                                                        // Determine if the method returns a handle (another native instance)
                                                        let returns_handle = match (module_name.as_str(), method_name) {
                                                            ("mongodb", "db") => Some("Database"),
                                                            ("mongodb", "collection") => Some("Collection"),
                                                            ("mysql2" | "mysql2/promise", "getConnection") => Some("PoolConnection"),
                                                            ("better-sqlite3", "prepare") => Some("Statement"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = returns_handle {
                                                            ctx.register_native_instance(name.clone(), module_name, class_name.to_string());
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
                                    let return_type = extract_ts_type_with_ctx(&rt.type_ann, Some(ctx));
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
                                            "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
                                            "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
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
                                                    if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
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
                                                name.clone(), module.to_string(), class.to_string()
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
                                if let Some((_, _, existing_ty)) = ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name) {
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
                                mutable: matches!(var_decl.kind, ast::VarDeclKind::Let | ast::VarDeclKind::Var),
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
                    module.classes.push(class);
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
                                ast::TsModuleName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            };
                            let class = lower_namespace_as_class(ctx, module, &ns_name, body, true)?;
                            let class_name = class.name.clone();
                            module.classes.push(class);
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
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        };
                        let exported = named.exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::ReExport {
                            source: source.clone(),
                            imported: local,
                            exported,
                        });
                    }
                }
            } else {
                // Local export: export { foo, bar as baz }
                for spec in &export_named.specifiers {
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only { continue; }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        };
                        let exported = named.exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::Named { local: local.clone(), exported: exported.clone() });

                        // If the local name refers to a function, add it to exported_functions
                        // so that a wrapper function is generated for cross-module calls
                        if let Some(func_id) = ctx.lookup_func(&local) {
                            module.exported_functions.push((exported.clone(), func_id));
                        }

                        // Check if the variable is a closure or other exportable object
                        // by looking through init statements
                        for stmt in &module.init {
                            if let Stmt::Let { name, init: Some(init_expr), .. } = stmt {
                                if name == &local {
                                    let is_exportable = matches!(init_expr,
                                        Expr::Closure { .. } |
                                        Expr::Object(_) |
                                        Expr::Array(_) |
                                        Expr::Call { .. } |
                                        Expr::New { .. } |
                                        Expr::JsNew { .. }
                                    );
                                    if is_exportable {
                                        module.exported_objects.push(exported.clone());
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
                        // Named function: export default function foo() {}
                        // Create a function and mark it as default export
                        let func_name = ident.sym.to_string();
                        // TODO: properly lower function expression
                        module.exports.push(Export::Named {
                            local: func_name,
                            exported: "default".to_string(),
                        });
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
                module.exported_functions.push(("default".to_string(), func_id));
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
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                getters: Vec::new(),
                setters: Vec::new(),
                static_fields: Vec::new(),
                static_methods: Vec::new(),
                is_exported,
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
                            let ty = ident.type_ann.as_ref()
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
                            ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                        }
                        if let Some((module, class)) = native_instance_from_return_type(&func.return_type) {
                            ctx.func_return_native_instances.push((
                                func.name.clone(), module.to_string(), class.to_string()
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
                                    if let Some((_, _, existing_ty)) = ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name) {
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
                                ctx.namespace_vars.push((ns_name.to_string(), name.clone(), id));
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
                        module.classes.push(class);
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
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods,
        is_exported,
    })
}

fn lower_stmt(
    ctx: &mut LoweringContext,
    module: &mut Module,
    stmt: &ast::Stmt,
) -> Result<()> {
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
                    if let Some((module, class)) = native_instance_from_return_type(&func.return_type) {
                        ctx.func_return_native_instances.push((
                            func.name.clone(), module.to_string(), class.to_string()
                        ));
                    }
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> = func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    ctx.func_defaults.push((func.id, defaults, param_ids));
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
                                            if ctx.generator_func_names.contains(ident.sym.as_ref()) {
                                                // Lower the generator call, wrap in IteratorToArray, assign to temp
                                                let gen_expr = lower_expr(ctx, init)?;
                                                let arr_expr = Expr::IteratorToArray(Box::new(gen_expr));
                                                let temp_id = ctx.fresh_local();
                                                ctx.locals.push((format!("__gen_arr_{}", temp_id), temp_id, Type::Array(Box::new(Type::Any))));
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
                                                                    let name = ident.id.sym.to_string();
                                                                    let id = ctx.define_local(name.clone(), Type::Any);
                                                                    module.init.push(Stmt::Let {
                                                                        id,
                                                                        name,
                                                                        ty: Type::Any,
                                                                        mutable,
                                                                        init: Some(Expr::IndexGet {
                                                                            object: Box::new(Expr::LocalGet(temp_id)),
                                                                            index: Box::new(Expr::Number(idx as f64)),
                                                                        }),
                                                                    });
                                                                    idx += 1;
                                                                }
                                                                ast::Pat::Rest(rest) => {
                                                                    if let ast::Pat::Ident(rest_ident) = &*rest.arg {
                                                                        let name = rest_ident.id.sym.to_string();
                                                                        let id = ctx.define_local(name.clone(), Type::Array(Box::new(Type::Any)));
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
                                                                _ => { idx += 1; }
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
                                    let target_ast = call.args.get(0).map(|a| a.expr.clone());
                                    let handler_ast = call.args.get(1).map(|a| a.expr.clone());
                                    let target = if let Some(t) = target_ast { lower_expr(ctx, &t)? } else { Expr::Undefined };
                                    let handler = if let Some(h) = handler_ast { lower_expr(ctx, &h)? } else { Expr::Object(vec![]) };
                                    let mut proxy_alias: Option<String> = None;
                                    let mut revoke_alias: Option<String> = None;
                                    for prop in &obj_pat.props {
                                        match prop {
                                            ast::ObjectPatProp::KeyValue(kv) => {
                                                let key_name = match &kv.key {
                                                    ast::PropName::Ident(i) => i.sym.to_string(),
                                                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                    _ => continue,
                                                };
                                                if let ast::Pat::Ident(alias) = &*kv.value {
                                                    let alias_name = alias.id.sym.to_string();
                                                    if key_name == "proxy" { proxy_alias = Some(alias_name); }
                                                    else if key_name == "revoke" { revoke_alias = Some(alias_name); }
                                                }
                                            }
                                            ast::ObjectPatProp::Assign(a) => {
                                                let name = a.key.sym.to_string();
                                                if name == "proxy" { proxy_alias = Some(name); }
                                                else if name == "revoke" { revoke_alias = Some(name); }
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
                                            init: Some(Expr::ProxyNew { target: Box::new(target), handler: Box::new(handler) }),
                                        });
                                        ctx.proxy_locals.insert(p_name.clone());
                                        if let Some(r_name) = revoke_alias {
                                            ctx.proxy_revoke_locals.insert(r_name.clone(), p_name);
                                            let rev_id = ctx.define_local(r_name.clone(), Type::Any);
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
                                    // Lower the class with the binding name so
                                    // `new BindName(...)` works unchanged.
                                    let lowered_class = crate::lower_decl::lower_class_from_ast(
                                        ctx,
                                        &class_expr.class,
                                        &bind_name,
                                        false,
                                    )?;
                                    module.classes.push(lowered_class);
                                    // Register the alias so `new X()` → `new X()`
                                    // (no-op lookup, but marks the binding as a class).
                                    ctx.class_expr_aliases.insert(bind_name.clone(), bind_name.clone());
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
                                        if let Some((_param_name, mixin_class_box)) = ctx.mixin_funcs.get(&fn_name).cloned() {
                                            if call.args.len() == 1 {
                                                if let ast::Expr::Ident(base_ident) = call.args[0].expr.as_ref() {
                                                    let base_class_name = base_ident.sym.to_string();
                                                    if ctx.lookup_class(&base_class_name).is_some() {
                                                        let bind_name = ident.id.sym.to_string();
                                                        if ctx.lookup_class(&bind_name).is_none() {
                                                            let mut new_class = (*mixin_class_box).clone();
                                                            let base_id = ast::Ident::new(
                                                                base_class_name.clone().into(),
                                                                base_ident.span,
                                                                base_ident.ctxt,
                                                            );
                                                            new_class.super_class = Some(Box::new(ast::Expr::Ident(base_id)));
                                                            let lowered_class = crate::lower_decl::lower_class_from_ast(
                                                                ctx,
                                                                &new_class,
                                                                &bind_name,
                                                                false,
                                                            )?;
                                                            module.classes.push(lowered_class);
                                                            ctx.class_expr_aliases.insert(bind_name.clone(), bind_name.clone());
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
                            if let Stmt::Let { name, init: Some(Expr::Await(inner)), .. } = s {
                                if let Expr::NativeMethodCall { module: mod_name, method, .. } = inner.as_ref() {
                                    let class_name = match (mod_name.as_str(), method.as_str()) {
                                        ("axios", "get" | "post" | "put" | "delete" | "patch" | "request") => Some("Response"),
                                        ("mongodb", "connect") => Some("MongoClient"),
                                        ("pg", "connect") => Some("Client"),
                                        _ => None,
                                    };
                                    if let Some(cn) = class_name {
                                        ctx.register_native_instance(name.clone(), mod_name.clone(), cn.to_string());
                                    }
                                }
                            }
                            // Track synchronous native module factories as native instances.
                            // Added for workstream A1.5 so `const sock = net.createConnection(...)`
                            // registers `sock` as a Socket instance; without this, subsequent
                            // `sock.write/on/end/destroy` miss the NATIVE_MODULE_TABLE dispatch
                            // and never reach the `js_net_socket_*` FFI in perry-stdlib.
                            if let Stmt::Let { name, init: Some(Expr::NativeMethodCall { module: mod_name, method, object: None, .. }), .. } = s {
                                let class_name = match (mod_name.as_str(), method.as_str()) {
                                    ("net", "createConnection" | "connect") => Some("Socket"),
                                    // tls.connect returns the same Socket class — reuses
                                    // all the write/end/destroy/on/upgradeToTLS dispatch.
                                    ("tls", "connect") => Some("Socket"),
                                    _ => None,
                                };
                                if let Some(cn) = class_name {
                                    // Register under `"net"` (the module the Socket class belongs to)
                                    // regardless of which module the factory lived in, so method
                                    // dispatch resolves correctly.
                                    ctx.register_native_instance(name.clone(), "net".to_string(), cn.to_string());
                                    let _ = mod_name; // suppress unused on tls branch
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
                            if let Stmt::Let { name, init: Some(Expr::Call { callee, .. }), .. } = s {
                                if let Expr::FuncRef(func_id) = callee.as_ref() {
                                    let func_name_owned = ctx.lookup_func_name(*func_id).map(|s| s.to_string());
                                    if let Some(func_name) = func_name_owned {
                                        let lookup = ctx.lookup_func_return_native_instance(&func_name)
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
                    module.classes.push(class);
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
                    let is_var = false;
                    for decl in &fake_var.decls {
                        if let Some(init) = &decl.init {
                            match &decl.name {
                                ast::Pat::Ident(bind_ident) => {
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
                                _ => {}
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
                                ast::TsModuleName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            };
                            let class = lower_namespace_as_class(ctx, module, &ns_name, body, false)?;
                            module.classes.push(class);
                        }
                    }
                }
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
            let else_branch = if_stmt.alt.as_ref()
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
                module.init.push(Stmt::Labeled { label, body: Box::new(body) });
            } else {
                let mut inner = inner;
                let last = inner.pop().unwrap();
                for s in inner {
                    module.init.push(s);
                }
                module.init.push(Stmt::Labeled { label, body: Box::new(last) });
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
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                ctx.var_hoisted_ids.insert(id);
                                module.init.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
                            }
                            None
                        } else {
                            for decl in var_decl.decls.iter().skip(1) {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                module.init.push(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr });
                            }
                            if let Some(decl) = var_decl.decls.first() {
                                let name = get_binding_name(&decl.name)?;
                                let init_expr = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                Some(Box::new(Stmt::Let { id, name, ty: Type::Any, mutable: true, init: init_expr }))
                            } else { None }
                        }
                    }
                    ast::VarDeclOrExpr::Expr(expr) => { Some(Box::new(Stmt::Expr(lower_expr(ctx, expr)?))) }
                }
            } else { None };
            let condition = for_stmt.test.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let update = for_stmt.update.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let body = lower_body_stmt(ctx, &for_stmt.body)?;
            ctx.pop_block_scope(for_scope_mark);
            module.init.push(Stmt::For { init, condition, update, body });
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

                Some(CatchClause { param, body: catch_body })
            } else {
                None
            };

            // finally block is its own lexical scope
            let finally = if let Some(ref finally_block) = try_stmt.finalizer {
                Some(lower_block_stmt_scoped(ctx, finally_block)?)
            } else {
                None
            };

            module.init.push(Stmt::Try { body, catch, finally });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            module.init.push(Stmt::Throw(expr));
        }
        ast::Stmt::Switch(switch_stmt) => {
            let discriminant = lower_expr(ctx, &switch_stmt.discriminant)?;
            let mut cases = Vec::new();

            for case in &switch_stmt.cases {
                let test = case.test.as_ref()
                    .map(|e| lower_expr(ctx, e))
                    .transpose()?;

                let mut body = Vec::new();
                for stmt in &case.cons {
                    body.extend(lower_body_stmt(ctx, stmt)?);
                }

                cases.push(SwitchCase { test, body });
            }

            module.init.push(Stmt::Switch { discriminant, cases });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // --- Iterator protocol path for generators ---
            // Detect: for (const x of genFunc(...)) where genFunc is function*
            let is_generator_call = if let ast::Expr::Call(call) = &*for_of_stmt.right {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = &**callee_expr {
                        ctx.generator_func_names.contains(ident.sym.as_ref())
                    } else { false }
                } else { false }
            } else { false };

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
                    } else { false }
                } else { false }
            } else { false };
            let needs_await = for_of_stmt.is_await || callee_is_async_gen;

            // Also detect: for (const x of new Range(...)) where Range
            // defines `*[Symbol.iterator]()`. We lowered that method as
            // a synthesized top-level generator function taking `this`
            // as its first parameter; the for-of here dispatches by
            // calling that function with the lowered receiver.
            let iter_from_class: Option<perry_types::FuncId> = if let ast::Expr::New(new_expr) = &*for_of_stmt.right {
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
                ctx.locals.push((format!("__iter_{}", iter_id), iter_id, Type::Any));
                module.init.push(Stmt::Let {
                    id: iter_id,
                    name: format!("__iter_{}", iter_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(iter_expr),
                });

                let result_id = ctx.fresh_local();
                ctx.locals.push((format!("__result_{}", result_id), result_id, Type::Any));
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
                        } else { format!("__gen_item") }
                    } else { format!("__gen_item") }
                } else { format!("__gen_item") };
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
                let init_before = module.init.len();
                if let ast::Stmt::Block(block) = &*for_of_stmt.body {
                    for s in &block.stmts {
                        lower_stmt(ctx, module, s)?;
                    }
                }
                let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
                body_stmts.append(&mut user_body);
                // __result = __iter.next()
                body_stmts.push(Stmt::Expr(Expr::LocalSet(
                    result_id,
                    Box::new(next_call),
                )));

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

            // Lower the iterable expression (the array)
            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;

            // If the iterable is a Map, wrap in MapEntries to convert to array
            // This handles: for (const [k, v] of myMap) { ... }
            // Also extract the Map's key/value type args for proper type propagation.
            let mut map_key_type: Option<Type> = None;
            let mut map_val_type: Option<Type> = None;
            let arr_expr = if let ast::Expr::Ident(ident) = &*for_of_stmt.right {
                let name = ident.sym.to_string();
                let local_type = ctx.lookup_local_type(&name);
                let map_type_args = local_type.as_ref()
                    .and_then(|ty| {
                        if let Type::Generic { base, type_args } = ty {
                            if base == "Map" { Some(type_args.clone()) } else { None }
                        } else {
                            None
                        }
                    });
                let is_set = local_type.as_ref()
                    .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                    .unwrap_or(false);
                if let Some(type_args) = map_type_args {
                    if type_args.len() >= 2 {
                        map_key_type = Some(type_args[0].clone());
                        map_val_type = Some(type_args[1].clone());
                    }
                    Expr::MapEntries(Box::new(arr_expr))
                } else if is_set {
                    // Convert Set to Array for iteration: for (const x of mySet)
                    Expr::SetValues(Box::new(arr_expr))
                } else {
                    arr_expr
                }
            } else {
                arr_expr
            };

            // Determine the array element type: String for strings, Tuple(K, V) for Maps, Any otherwise.
            // For an identifier iterable like `for (const word of words)` where
            // `words: string[]`, extract the element type from the local's
            // declared Array<T> so the synthesized iteration variable gets
            // the right type (was always Any, breaking `word.length` etc.).
            let elem_type = if is_string_iter {
                Type::String
            } else if let (Some(ref k), Some(ref v)) = (&map_key_type, &map_val_type) {
                Type::Tuple(vec![k.clone(), v.clone()])
            } else if let ast::Expr::Ident(ident) = &*for_of_stmt.right {
                let name = ident.sym.to_string();
                match ctx.lookup_local_type(&name) {
                    Some(Type::Array(elem)) => (**elem).clone(),
                    Some(Type::Generic { base, type_args }) if base == "Array" && type_args.len() == 1 => {
                        type_args[0].clone()
                    }
                    _ => Type::Any,
                }
            } else {
                Type::Any
            };
            // The __arr holder's type: String for string iteration (so codegen uses
            // string.length and the str[i] char-access path), Array otherwise.
            let arr_type = if is_string_iter {
                Type::String
            } else {
                Type::Array(Box::new(elem_type.clone()))
            };

            // Create internal variables for the array and index
            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            // Register these in the context so they can be looked up
            ctx.locals.push((format!("__arr_{}", arr_id), arr_id, arr_type.clone()));
            ctx.locals.push((format!("__idx_{}", idx_id), idx_id, Type::Number));

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
            ctx.locals.push((format!("__item_{}", item_id), item_id, elem_type.clone()));

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
                                            let var_type = if let Type::Tuple(ref types) = elem_type {
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
                        let item_expr = Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        };

                        match &decl.name {
                            ast::Pat::Ident(_) => {
                                // Simple binding: for (const x of arr)
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: elem_type.clone(),
                                    mutable: false,
                                    init: Some(item_expr),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
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
                                            let var_type = if let Type::Tuple(ref types) = elem_type {
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
                                            let init_value = if let Some(default_expr) = &assign.value {
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
                                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                _ => continue,
                                            };
                                            if let ast::Pat::Ident(_) = &*kv.value {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: Type::Any,
                                                    mutable: false,
                                                    init: Some(Expr::PropertyGet {
                                                        object: Box::new(Expr::LocalGet(item_id)),
                                                        property: key,
                                                    }),
                                                });
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
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(arr_id)),
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
            loop_body.insert(0, Stmt::Let {
                id: key_id,
                name: key_name,
                ty: Type::String,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(keys_id)),
                    index: Box::new(Expr::LocalGet(idx_id)),
                }),
            });

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
fn lower_expr_assignment(ctx: &mut LoweringContext, expr: &ast::Expr, value: Box<Expr>) -> Result<Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else {
                eprintln!("  Warning: Assignment to undeclared variable '{}', creating implicit local", name);
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
                    Ok(Expr::PropertySet { object, property, value })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Ok(Expr::IndexSet { object, index, value })
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name.to_string());
                    Ok(Expr::PropertySet { object, property, value })
                }
            }
        }
        // Recursively unwrap parens and type annotations
        ast::Expr::Paren(paren) => lower_expr_assignment(ctx, &paren.expr, value),
        ast::Expr::TsAs(ts_as) => lower_expr_assignment(ctx, &ts_as.expr, value),
        ast::Expr::TsNonNull(ts_nn) => lower_expr_assignment(ctx, &ts_nn.expr, value),
        ast::Expr::TsTypeAssertion(ts_ta) => lower_expr_assignment(ctx, &ts_ta.expr, value),
        ast::Expr::TsSatisfies(ts_sat) => lower_expr_assignment(ctx, &ts_sat.expr, value),
        _ => Err(anyhow!("Unsupported expression as assignment target: {:?}", expr)),
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
                let (param_types, return_type) = ctx.lookup_extern_func_types(orig_name)
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
            } else {
                // GlobalGet(0) is a sentinel: codegen routes by name from the
                // parent PropertyGet/Call/Member context. Bare uses lower to
                // 0.0 (perry-codegen/src/expr.rs Expr::GlobalGet arm).
                if name != "console" && name != "process" && name != "globalThis" && name != "Buffer"
                    && name != "Date" && name != "JSON" && name != "Math" && name != "Object"
                    && name != "Array" && name != "String" && name != "Number" && name != "Boolean"
                    && name != "Error" && name != "TypeError" && name != "RangeError" && name != "Promise"
                    && name != "Map" && name != "Set" && name != "RegExp" && name != "Symbol"
                    && name != "WeakMap" && name != "WeakSet" && name != "WeakRef" && name != "FinalizationRegistry" && name != "Proxy" && name != "Reflect"
                    && name != "Uint8Array" && name != "Int8Array" && name != "Int16Array" && name != "Uint16Array"
                    && name != "Int32Array" && name != "Uint32Array" && name != "Float32Array" && name != "Float64Array"
                    && name != "TextEncoder" && name != "TextDecoder"
                    && name != "URL" && name != "URLSearchParams" && name != "AbortController" && name != "FormData"
                    && name != "Headers" && name != "fetch" && name != "crypto" && name != "performance"
                    && name != "queueMicrotask" && name != "structuredClone" && name != "atob" && name != "btoa"
                    && name != "BigInt" {
                    eprintln!(
                        "  Warning: unknown identifier '{}' — assuming global; member access will dispatch by name at runtime, bare reads lower to 0",
                        name
                    );
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
                            let is_match = (class_name == "WeakRef" && ctx.weakref_locals.contains(&local_name))
                                || (class_name == "FinalizationRegistry" && ctx.finreg_locals.contains(&local_name));
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
                return Ok(Expr::InstanceOf { expr, ty });
            }

            let left = Box::new(lower_expr(ctx, &bin.left)?);
            let right = Box::new(lower_expr(ctx, &bin.right)?);

            match bin.op {
                // Arithmetic
                ast::BinaryOp::Add => Ok(Expr::Binary { op: BinaryOp::Add, left, right }),
                ast::BinaryOp::Sub => Ok(Expr::Binary { op: BinaryOp::Sub, left, right }),
                ast::BinaryOp::Mul => Ok(Expr::Binary { op: BinaryOp::Mul, left, right }),
                ast::BinaryOp::Div => Ok(Expr::Binary { op: BinaryOp::Div, left, right }),
                ast::BinaryOp::Mod => Ok(Expr::Binary { op: BinaryOp::Mod, left, right }),
                ast::BinaryOp::Exp => Ok(Expr::Binary { op: BinaryOp::Pow, left, right }),

                // Comparison (treat == same as === for typed code)
                ast::BinaryOp::EqEq => {
                    // Proxy/Reflect fold: `Reflect.getPrototypeOf(x) === <Class>.prototype`
                    // always true in our model (we don't maintain real prototypes).
                    // Same fold for `Object.getPrototypeOf(x) === <Class>.prototype`.
                    if matches!(&*left, Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)) {
                        if matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype") {
                            return Ok(Expr::Bool(true));
                        }
                    }
                    Ok(Expr::Compare { op: CompareOp::LooseEq, left, right })
                }
                ast::BinaryOp::EqEqEq => {
                    if matches!(&*left, Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)) {
                        if matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype") {
                            return Ok(Expr::Bool(true));
                        }
                    }
                    Ok(Expr::Compare { op: CompareOp::Eq, left, right })
                }
                ast::BinaryOp::NotEq => Ok(Expr::Compare { op: CompareOp::LooseNe, left, right }),
                ast::BinaryOp::NotEqEq => Ok(Expr::Compare { op: CompareOp::Ne, left, right }),
                ast::BinaryOp::Lt => Ok(Expr::Compare { op: CompareOp::Lt, left, right }),
                ast::BinaryOp::LtEq => Ok(Expr::Compare { op: CompareOp::Le, left, right }),
                ast::BinaryOp::Gt => Ok(Expr::Compare { op: CompareOp::Gt, left, right }),
                ast::BinaryOp::GtEq => Ok(Expr::Compare { op: CompareOp::Ge, left, right }),

                // Logical
                ast::BinaryOp::LogicalAnd => Ok(Expr::Logical { op: LogicalOp::And, left, right }),
                ast::BinaryOp::LogicalOr => Ok(Expr::Logical { op: LogicalOp::Or, left, right }),
                ast::BinaryOp::NullishCoalescing => Ok(Expr::Logical { op: LogicalOp::Coalesce, left, right }),

                // Bitwise
                ast::BinaryOp::BitAnd => Ok(Expr::Binary { op: BinaryOp::BitAnd, left, right }),
                ast::BinaryOp::BitOr => Ok(Expr::Binary { op: BinaryOp::BitOr, left, right }),
                ast::BinaryOp::BitXor => Ok(Expr::Binary { op: BinaryOp::BitXor, left, right }),
                ast::BinaryOp::LShift => Ok(Expr::Binary { op: BinaryOp::Shl, left, right }),
                ast::BinaryOp::RShift => Ok(Expr::Binary { op: BinaryOp::Shr, left, right }),
                ast::BinaryOp::ZeroFillRShift => Ok(Expr::Binary { op: BinaryOp::UShr, left, right }),

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
                        Ok(Expr::Unary { op: UnaryOp::Neg, operand })
                    }
                }
                ast::UnaryOp::Plus => Ok(Expr::Unary { op: UnaryOp::Pos, operand }),
                ast::UnaryOp::Bang => Ok(Expr::Unary { op: UnaryOp::Not, operand }),
                ast::UnaryOp::Tilde => Ok(Expr::Unary { op: UnaryOp::BitNot, operand }),
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
                        return Ok(Expr::ProxyDelete { proxy: proxy.clone(), key: key.clone() });
                    }
                    Ok(Expr::Delete(operand))
                }
                ast::UnaryOp::Void => Ok(Expr::Void(operand)),
                _ => Err(anyhow!("Unsupported unary operator: {:?}", unary.op)),
            }
        }
        ast::Expr::Call(call) => {
            // Check if any argument has spread
            let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

            // Pre-scan: if this call is `<fastify app>.get|post|...|addHook(path, handler)`,
            // the handler is an arrow function whose first two params are
            // the FastifyRequest and FastifyReply. Register them as native
            // instances BEFORE lowering the arrow so that `request.header(...)`
            // and `request.headers[...]` inside the handler dispatch through
            // `Expr::NativeMethodCall` instead of generic object access.
            //
            // In v0.4.51 this was (presumably) handled by the old codegen's
            // per-method dispatch table; in v0.5.x the dispatch happens at
            // HIR lower time via `lookup_native_instance(name)`, so we need
            // the annotation here for the lookup to succeed.
            let fastify_handler_names: Option<(String, String)> = pre_scan_fastify_handler_params(ctx, call);
            if let Some((req_name, reply_name)) = &fastify_handler_names {
                ctx.register_native_instance(req_name.clone(), "fastify".to_string(), "Request".to_string());
                if !reply_name.is_empty() {
                    ctx.register_native_instance(reply_name.clone(), "fastify".to_string(), "Reply".to_string());
                }
            }

            // perry/ui reactive Text: `Text(\`...${state.value}...\`)` where at least one
            // interpolation is `<ident>.value` on a State binding. Desugars to
            // `{ __h = Text(concat); stateOnChange(state, v => textSetString(__h, concat)); __h }`
            // so the label updates when state.set(...) fires subscribers. Closes #104.
            if let Some(desugared) = try_desugar_reactive_text(ctx, call)? {
                return Ok(desugared);
            }

            // perry/ui reactive animation: `widget.animateOpacity(<expr reading
            // state.value>, dur)` or `.animatePosition(...)` desugars to an IIFE
            // that runs the initial animation and registers a `stateOnChange`
            // subscriber per referenced State so the animation re-fires when
            // any read state changes. Closes the follow-up to #109.
            if let Some(desugared) = try_desugar_reactive_animate(ctx, call)? {
                return Ok(desugared);
            }

            let mut args = call.args.iter()
                .map(|arg| lower_expr(ctx, &arg.expr))
                .collect::<Result<Vec<_>>>()?;

            // --- Proxy apply / revoke fast path ---
            if !has_spread {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                        let name = ident.sym.to_string();
                        if ctx.proxy_locals.contains(&name) {
                            if let Some(id) = ctx.lookup_local(&name) {
                                return Ok(Expr::ProxyApply {
                                    proxy: Box::new(Expr::LocalGet(id)),
                                    args,
                                });
                            }
                        }
                        if let Some(proxy_name) = ctx.proxy_revoke_locals.get(&name).cloned() {
                            if let Some(id) = ctx.lookup_local(&proxy_name) {
                                return Ok(Expr::ProxyRevoke(Box::new(Expr::LocalGet(id))));
                            }
                        }
                    }
                }
            }

            // --- Object.prototype.toString.call(x) → js_object_to_string(x) ---
            // AST shape is a four-level member expression:
            //   call.call(x)
            //   ^^^^^^^^^^ outer member: (Object.prototype.toString).call
            // The runtime helper consults the class's `Symbol.toStringTag`
            // getter (registered at module init via `__perry_wk_tostringtag_*`)
            // and returns `[object <tag>]` or the default `[object Object]`.
            if !has_spread && args.len() == 1 {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                        if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                            (&outer.prop, outer.obj.as_ref())
                        {
                            if outer_prop.sym.as_ref() == "call" {
                                if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Member(inner)) =
                                    (&mid.prop, mid.obj.as_ref())
                                {
                                    if mid_prop.sym.as_ref() == "toString" {
                                        if let (
                                            ast::MemberProp::Ident(inner_prop),
                                            ast::Expr::Ident(inner_obj),
                                        ) = (&inner.prop, inner.obj.as_ref())
                                        {
                                            if inner_obj.sym.as_ref() == "Object"
                                                && inner_prop.sym.as_ref() == "prototype"
                                            {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::ExternFuncRef {
                                                        name: "js_object_to_string".to_string(),
                                                        param_types: Vec::new(),
                                                        return_type: Type::Any,
                                                    }),
                                                    args: vec![arg],
                                                    type_args: Vec::new(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // If spread is present, create CallSpread instead of Call
            let spread_args: Option<Vec<CallArg>> = if has_spread {
                Some(call.args.iter().zip(args.iter())
                    .map(|(ast_arg, lowered)| {
                        if ast_arg.spread.is_some() {
                            CallArg::Spread(lowered.clone())
                        } else {
                            CallArg::Expr(lowered.clone())
                        }
                    })
                    .collect())
            } else {
                None
            };

            match &call.callee {
                ast::Callee::Super(_) => {
                    // super() call in constructor
                    Ok(Expr::SuperCall(args))
                }
                ast::Callee::Expr(expr) => {
                    // Check for super.method() call
                    if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                        if let ast::SuperProp::Ident(ident) = &super_prop.prop {
                            return Ok(Expr::SuperMethodCall {
                                method: ident.sym.to_string(),
                                args,
                            });
                        }
                    }

                    // Check for nested process member calls like process.hrtime.bigint()
                    if let ast::Expr::Member(outer_member) = expr.as_ref() {
                        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
                            if let ast::Expr::Ident(inner_obj) = inner_member.obj.as_ref() {
                                if inner_obj.sym.as_ref() == "process" {
                                    if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                                        if inner_prop.sym.as_ref() == "hrtime" {
                                            if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                                if method_ident.sym.as_ref() == "bigint" {
                                                    return Ok(Expr::ProcessHrtimeBigint);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for native module method calls (e.g., mysql.createConnection())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();

                            // Check for process module methods
                            if obj_name == "process" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "uptime" => return Ok(Expr::ProcessUptime),
                                        "cwd" => return Ok(Expr::ProcessCwd),
                                        "memoryUsage" => return Ok(Expr::ProcessMemoryUsage),
                                        "nextTick" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ProcessNextTick(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "on" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let event = iter.next().unwrap();
                                                let handler = iter.next().unwrap();
                                                return Ok(Expr::ProcessOn {
                                                    event: Box::new(event),
                                                    handler: Box::new(handler),
                                                });
                                            }
                                        }
                                        "chdir" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ProcessChdir(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "kill" => {
                                            if args.len() >= 1 {
                                                let mut iter = args.into_iter();
                                                let pid = iter.next().unwrap();
                                                let signal = iter.next().map(Box::new);
                                                return Ok(Expr::ProcessKill {
                                                    pid: Box::new(pid),
                                                    signal,
                                                });
                                            }
                                        }
                                        "exit" => {
                                            // process.exit() / process.exit(code) — never
                                            // returns, terminates the process. Until now this
                                            // fell through to generic NativeMethodCall which
                                            // silently no-op'd, so scripts that rely on it to
                                            // end the event loop (e.g. `main().then(() =>
                                            // process.exit(0))` in a net-socket driver) would
                                            // hang with the socket still keeping the loop alive.
                                            let code = if args.len() >= 1 {
                                                Some(Box::new(args.into_iter().next().unwrap()))
                                            } else {
                                                None
                                            };
                                            return Ok(Expr::ProcessExit(code));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os module methods FIRST (before generic NativeMethodCall)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => return Ok(Expr::OsPlatform),
                                        "arch" => return Ok(Expr::OsArch),
                                        "hostname" => return Ok(Expr::OsHostname),
                                        "homedir" => return Ok(Expr::OsHomedir),
                                        "tmpdir" => return Ok(Expr::OsTmpdir),
                                        "totalmem" => return Ok(Expr::OsTotalmem),
                                        "freemem" => return Ok(Expr::OsFreemem),
                                        "uptime" => return Ok(Expr::OsUptime),
                                        "type" => return Ok(Expr::OsType),
                                        "release" => return Ok(Expr::OsRelease),
                                        "cpus" => return Ok(Expr::OsCpus),
                                        "networkInterfaces" => return Ok(Expr::OsNetworkInterfaces),
                                        "userInfo" => return Ok(Expr::OsUserInfo),
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer static methods
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding
                                            });
                                        }
                                        "alloc" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            let fill = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferAlloc {
                                                size: Box::new(size),
                                                fill
                                            });
                                        }
                                        "allocUnsafe" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            return Ok(Expr::BufferAllocUnsafe(Box::new(size)));
                                        }
                                        "concat" => {
                                            let list = args.get(0).cloned().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::BufferConcat(Box::new(list)));
                                        }
                                        "isBuffer" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::BufferIsBuffer(Box::new(obj)));
                                        }
                                        "byteLength" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::String("".to_string()));
                                            return Ok(Expr::BufferByteLength(Box::new(data)));
                                        }
                                        // `Buffer.compare(a, b)` → `a.compare(b)` instance call
                                        // (handled by runtime buffer dispatch).
                                        "compare" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let a = iter.next().unwrap();
                                                let b = iter.next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(a),
                                                        property: "compare".to_string(),
                                                    }),
                                                    args: vec![b],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Uint8Array static methods
                            if obj_name == "Uint8Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::Uint8ArrayFrom(Box::new(data)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Object static methods
                            if obj_name == "Object" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "keys" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectKeys(Box::new(obj)));
                                        }
                                        "values" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectValues(Box::new(obj)));
                                        }
                                        "entries" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectEntries(Box::new(obj)));
                                        }
                                        // Object.assign(target, src1, src2, ...) - treat as object spread
                                        // Each non-object arg is spread; object literal args are inlined
                                        "assign" => {
                                            let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                                            for arg in &args {
                                                match arg {
                                                    Expr::Object(props) => {
                                                        // Inline object literal props as static key-value pairs
                                                        for (key, val) in props {
                                                            parts.push((Some(key.clone()), val.clone()));
                                                        }
                                                    }
                                                    _ => {
                                                        // Spread non-object expression
                                                        parts.push((None, arg.clone()));
                                                    }
                                                }
                                            }
                                            // If no spreads and only static props, return plain Object
                                            let has_spread = parts.iter().any(|(k, _)| k.is_none());
                                            if !has_spread {
                                                let static_props: Vec<(String, Expr)> = parts.into_iter()
                                                    .filter_map(|(k, v)| k.map(|key| (key, v)))
                                                    .collect();
                                                return Ok(Expr::Object(static_props));
                                            }
                                            return Ok(Expr::ObjectSpread { parts });
                                        }
                                        "fromEntries" => {
                                            let entries = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectFromEntries(Box::new(entries)));
                                        }
                                        "groupBy" => {
                                            // Object.groupBy(items, keyFn) — Node 22+ static method
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let items = iter.next().unwrap();
                                                let key_fn = iter.next().unwrap();
                                                let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                                                return Ok(Expr::ObjectGroupBy {
                                                    items: Box::new(items),
                                                    key_fn: Box::new(key_fn),
                                                });
                                            }
                                        }
                                        "is" => {
                                            let mut iter = args.into_iter();
                                            let a = iter.next().unwrap_or(Expr::Undefined);
                                            let b = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectIs(Box::new(a), Box::new(b)));
                                        }
                                        "hasOwn" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key)));
                                        }
                                        "freeze" => { return Ok(Expr::ObjectFreeze(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "seal" => { return Ok(Expr::ObjectSeal(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "preventExtensions" => { return Ok(Expr::ObjectPreventExtensions(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "create" => { return Ok(Expr::ObjectCreate(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isFrozen" => { return Ok(Expr::ObjectIsFrozen(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isSealed" => { return Ok(Expr::ObjectIsSealed(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isExtensible" => { return Ok(Expr::ObjectIsExtensible(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "getPrototypeOf" => { return Ok(Expr::ObjectGetPrototypeOf(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "defineProperty" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            let descriptor = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectDefineProperty(Box::new(obj), Box::new(key), Box::new(descriptor)));
                                        }
                                        "getOwnPropertyDescriptor" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectGetOwnPropertyDescriptor(Box::new(obj), Box::new(key)));
                                        }
                                        "getOwnPropertyNames" => {
                                            return Ok(Expr::ObjectGetOwnPropertyNames(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined))));
                                        }
                                        "getOwnPropertySymbols" => {
                                            return Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined))));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Symbol static methods: Symbol.for / Symbol.keyFor
                            if obj_name == "Symbol" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "for" => {
                                            let key = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::SymbolFor(Box::new(key)));
                                        }
                                        "keyFor" => {
                                            let sym = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::SymbolKeyFor(Box::new(sym)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            if obj_name == "Reflect" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "get" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectGet { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "set" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            let value = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectSet { target: Box::new(target), key: Box::new(key), value: Box::new(value) });
                                        }
                                        "has" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectHas { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "deleteProperty" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectDelete { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "ownKeys" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectOwnKeys(Box::new(target)));
                                        }
                                        "apply" => {
                                            let mut it = args.into_iter();
                                            let func = it.next().unwrap_or(Expr::Undefined);
                                            let this_arg = it.next().unwrap_or(Expr::Undefined);
                                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::ReflectApply { func: Box::new(func), this_arg: Box::new(this_arg), args: Box::new(args_arr) });
                                        }
                                        "construct" => {
                                            // Special case: `Reflect.construct(ClassName, [args...])`
                                            // where ClassName is a known class — fold to a direct
                                            // `new ClassName(...args)` expression.
                                            if call.args.len() >= 2 {
                                                if let ast::Expr::Ident(cls_ident) = call.args[0].expr.as_ref() {
                                                    let cls_name = cls_ident.sym.to_string();
                                                    if ctx.lookup_class(&cls_name).is_some() {
                                                        if let ast::Expr::Array(arr_lit) = call.args[1].expr.as_ref() {
                                                            let new_args: Vec<Expr> = arr_lit.elems.iter()
                                                                .filter_map(|e| e.as_ref())
                                                                .map(|e| lower_expr(ctx, &e.expr))
                                                                .collect::<Result<Vec<_>>>()?;
                                                            return Ok(Expr::New {
                                                                class_name: cls_name,
                                                                args: new_args,
                                                                type_args: vec![],
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::ReflectConstruct { target: Box::new(target), args: Box::new(args_arr) });
                                        }
                                        "defineProperty" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            let descriptor = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectDefineProperty { target: Box::new(target), key: Box::new(key), descriptor: Box::new(descriptor) });
                                        }
                                        "getPrototypeOf" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectGetPrototypeOf(Box::new(target)));
                                        }
                                        "setPrototypeOf" => return Ok(Expr::Bool(true)),
                                        "isExtensible" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectIsExtensible(Box::new(target)));
                                        }
                                        "preventExtensions" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectPreventExtensions(Box::new(target)));
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if obj_name == "Proxy" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    if method_ident.sym.as_ref() == "revocable" {
                                        let mut it = args.into_iter();
                                        let target = it.next().unwrap_or(Expr::Undefined);
                                        let handler = it.next().unwrap_or(Expr::Object(vec![]));
                                        return Ok(Expr::ProxyRevocable { target: Box::new(target), handler: Box::new(handler) });
                                    }
                                }
                            }

                            // Check for Array static methods
                            if obj_name == "Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "isArray" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ArrayIsArray(Box::new(value)));
                                        }
                                        "from" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            // `Array.from(iterable, mapFn)` uses a dedicated HIR
                                            // variant so codegen can handle Map/Set/Array sources
                                            // uniformly (materialize + js_array_map).
                                            if let Some(map_fn) = args.get(1).cloned() {
                                                return Ok(Expr::ArrayFromMapped {
                                                    iterable: Box::new(value),
                                                    map_fn: Box::new(map_fn),
                                                });
                                            }
                                            // Check if the source is a generator call — use iterator protocol
                                            let is_gen = is_generator_call_expr(ctx, &value);
                                            if is_gen {
                                                return Ok(Expr::IteratorToArray(Box::new(value)));
                                            }
                                            return Ok(Expr::ArrayFrom(Box::new(value)));
                                        }
                                        "of" => {
                                            // Array.of(1,2,3) is equivalent to [1,2,3]
                                            return Ok(Expr::Array(args));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net module methods
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let options = args.get(0).cloned().map(Box::new);
                                            let connection_listener = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::NetCreateServer { options, connection_listener });
                                        }
                                        // createConnection/connect fall through to generic NativeMethodCall
                                        // so they dispatch via NATIVE_MODULE_TABLE to the new
                                        // event-driven `js_net_socket_connect` in perry-stdlib (A1/A1.5).
                                        // The dedicated `Expr::NetCreateConnection` variant was never
                                        // lowered by the LLVM backend and remained as vestigial HIR;
                                        // the generic path gives us working codegen for free.
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            if let Some((module_name, _imported_method)) = ctx.lookup_native_module(&obj_name) {
                                // Skip modules handled specifically below (path, fs, child_process, etc.)
                                // `net` used to be in this list back when its method calls
                                // were short-circuited into `Expr::NetCreateConnection` etc.
                                // After A1.5 `net` goes through the generic NativeMethodCall
                                // path so the LLVM backend's NATIVE_MODULE_TABLE dispatches
                                // to `js_net_socket_*` in perry-stdlib.
                                let is_handled_module = module_name == "path" || module_name == "node:path"
                                    || module_name == "fs" || module_name == "node:fs"
                                    || module_name == "child_process" || module_name == "node:child_process"
                                    || module_name == "crypto" || module_name == "node:crypto"
                                    || module_name == "os" || module_name == "node:os";
                                if !is_handled_module {
                                    // This is a call on a native module (e.g., mysql.createConnection)
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.to_string();
                                        return Ok(Expr::NativeMethodCall {
                                            module: module_name.to_string(),
                                            class_name: None,  // Will be set by js_transform if needed
                                            object: None,  // Static call on module itself
                                            method: method_name,
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Check for static method calls (e.g., Counter.increment())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            if ctx.lookup_class(&obj_name).is_some() {
                                match &member.prop {
                                    ast::MemberProp::Ident(method_ident) => {
                                        let method_name = method_ident.sym.to_string();
                                        if ctx.has_static_method(&obj_name, &method_name) {
                                            return Ok(Expr::StaticMethodCall {
                                                class_name: obj_name,
                                                method_name,
                                                args,
                                            });
                                        }
                                    }
                                    // Private static method: WithPrivateStatic.#helper()
                                    ast::MemberProp::PrivateName(priv_ident) => {
                                        let method_name = format!("#{}", priv_ident.name.to_string());
                                        if ctx.has_static_method(&obj_name, &method_name) {
                                            return Ok(Expr::StaticMethodCall {
                                                class_name: obj_name,
                                                method_name,
                                                args,
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    // Check for native instance method calls (e.g., emitter.on(), ws.send())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            // Clone module_name and class_name to avoid borrow issues
                            let native_instance = ctx.lookup_native_instance(&obj_name)
                                .map(|(m, c)| (m.to_string(), c.to_string()));
                            if obj_name == "pool" {
                            }
                            if let Some((module_name, class_name)) = native_instance {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.to_string();
                                    // Get the object expression (the instance variable)
                                    let object_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::NativeMethodCall {
                                        module: module_name,
                                        class_name: Some(class_name),  // Use the registered class name
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }

                        // issue #195: WidgetCtor(...).modifierName(...) is silently dropped.
                        // Reject at compile time so users discover the options-object form.
                        if let ast::Expr::Call(inner_call) = member.obj.as_ref() {
                            if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                                if let ast::Expr::Ident(widget_ident) = inner_callee.as_ref() {
                                    let widget_name = widget_ident.sym.as_ref();
                                    if matches!(widget_name,
                                        "Text" | "VStack" | "HStack" | "ZStack" |
                                        "Image" | "Spacer" | "Divider" |
                                        "ForEach" | "Label" | "Gauge"
                                    ) {
                                        if matches!(ctx.lookup_native_module(widget_name),
                                            Some(("perry/ui", _))
                                        ) {
                                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                let modifier_name = method_ident.sym.as_ref();
                                                if is_widget_modifier_name(modifier_name) {
                                                    return Err(anyhow!(
                                                        "modifier '{}' must be passed as an option-object on the widget constructor; use: {}(\"...\", {{ {}: ... }})",
                                                        modifier_name, widget_name, modifier_name
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Check for method calls on new Big/Decimal/BigNumber() expressions
                        // e.g., new Big("100").div(2)
                        if let Some(module_name) = detect_native_instance_expr(&member.obj) {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.to_string();
                                let object_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name.to_string(),
                                    class_name: None,  // Will be set by js_transform if needed
                                    object: Some(Box::new(object_expr)),
                                    method: method_name,
                                    args,
                                });
                            }
                        }

                        // Check for chained method calls on registered native instances
                        // e.g., r1.times(...).times(...) where r1 is a Big
                        // The inner call might lower to a NativeMethodCall, and we need to chain properly
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.to_string();
                            // Lower the object expression first
                            let object_expr = lower_expr(ctx, &member.obj)?;
                            // Check if it's a NativeMethodCall for a fluent-API native module
                            if let Expr::NativeMethodCall { module, class_name, .. } = &object_expr {
                                // Methods that return the same type (builder pattern)
                                let is_math_lib = matches!(module.as_str(), "big.js" | "decimal.js" | "bignumber.js");
                                let is_math_method = matches!(method_name.as_str(),
                                    // arithmetic + chainable rounding/formatting
                                    "plus" | "minus" | "times" | "div" | "mod" |
                                    "pow" | "sqrt" | "abs" | "neg" | "round" | "floor" | "ceil" | "toFixed" |
                                    // decimal.js: terminal-shape methods that still need
                                    // NativeMethodCall dispatch (so a.plus(b).eq(c) etc.
                                    // doesn't fall back to the generic Call+PropertyGet path).
                                    "toString" | "toNumber" | "valueOf" |
                                    "eq" | "lt" | "lte" | "gt" | "gte" | "cmp" |
                                    "isZero" | "isPositive" | "isNegative"
                                );
                                // commander Command — every fluent method either
                                // returns the same handle (name/version/description/
                                // option/requiredOption/action) or a sub-Command with
                                // the same module + class (.command(name)). Either way
                                // the next chained call must dispatch through the
                                // commander NativeModSig table, not the generic
                                // dynamic-property fallback. Without this branch
                                // `program.name(...).version(...)` only the first
                                // call landed as a NativeMethodCall and the rest
                                // silently no-op'd at codegen — issue #187.
                                let is_commander = module.as_str() == "commander";
                                let is_commander_method = matches!(method_name.as_str(),
                                    "name" | "version" | "description" |
                                    "option" | "requiredOption" |
                                    "action" | "command" | "parse" | "opts"
                                );
                                if (is_math_lib && is_math_method)
                                    || (is_commander && is_commander_method)
                                {
                                    return Ok(Expr::NativeMethodCall {
                                        module: module.clone(),
                                        class_name: class_name.clone(),
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    // Check for fs.methodName() calls (including require('fs') aliases)
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            // Check if this is 'fs' directly or an alias from require('fs')
                            let obj_name = obj_ident.sym.as_ref();
                            let is_fs_module = obj_name == "fs" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("fs");
                            if is_fs_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "readFileSync" => {
                                            if args.len() >= 2 {
                                                // readFileSync(path, encoding) — returns string
                                                return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() == 1 {
                                                // readFileSync(path) without encoding — returns Buffer (Node parity)
                                                return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "writeFileSync" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path = iter.next().unwrap();
                                                let content = iter.next().unwrap();
                                                return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                            }
                                        }
                                        "existsSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "mkdirSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "unlinkSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "readFileBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "rmRecursive" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for path.methodName() calls (including require('path') aliases)
                            let is_path_module = obj_name == "path" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("path");
                            if is_path_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "join" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let mut result = iter.next().unwrap();
                                                for next_arg in iter {
                                                    result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                                }
                                                return Ok(result);
                                            }
                                        }
                                        "dirname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "basename" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path_arg = iter.next().unwrap();
                                                let ext_arg = iter.next().unwrap();
                                                return Ok(Expr::PathBasenameExt(Box::new(path_arg), Box::new(ext_arg)));
                                            }
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "extname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "resolve" => {
                                            if args.len() >= 1 {
                                                // path.resolve(a, b, c) => resolve(join(a, b, c))
                                                // For single arg, just resolve directly
                                                let mut iter = args.into_iter();
                                                let first = iter.next().unwrap();
                                                let mut joined = first;
                                                for next_arg in iter {
                                                    joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                                }
                                                return Ok(Expr::PathResolve(Box::new(joined)));
                                            }
                                        }
                                        "isAbsolute" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "relative" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let from = iter.next().unwrap();
                                                let to = iter.next().unwrap();
                                                return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
                                            }
                                        }
                                        "normalize" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathNormalize(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parse" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathParse(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "format" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathFormat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for JSON.methodName() calls
                            if obj_ident.sym.as_ref() == "JSON" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "parse" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let text = iter.next().unwrap();
                                                let reviver = iter.next().unwrap();
                                                return Ok(Expr::JsonParseWithReviver(Box::new(text), Box::new(reviver)));
                                            } else if args.len() >= 1 {
                                                let text = args.into_iter().next().unwrap();
                                                // Issue #179 typed-parse plan: if the call site
                                                // provides a TypeScript type argument (e.g.
                                                // `JSON.parse<Item[]>(blob)`), carry it into HIR
                                                // so codegen can emit a specialized parse path.
                                                // Semantically identical to JsonParse at runtime
                                                // (the `<T>` erases — Node-compatible).
                                                if let Some(type_args) = call.type_args.as_ref() {
                                                    if let Some(ts_type) = type_args.params.first() {
                                                        let ty = extract_ts_type_with_ctx(ts_type, Some(ctx));
                                                        // Resolve Named → structural (interface)
                                                        // aliases so codegen sees the full
                                                        // ObjectType without re-walking the alias
                                                        // table. Array<Named> inner element
                                                        // also gets resolved.
                                                        let resolved = resolve_typed_parse_ty(ctx, ty);
                                                        if !matches!(resolved, Type::Any | Type::Unknown) {
                                                            // Source-order field list for the
                                                            // inner Object type, if we can
                                                            // extract it from the AST. Codegen
                                                            // uses this for the fast-path
                                                            // per-field comparison.
                                                            let ordered_keys =
                                                                extract_typed_parse_source_order(
                                                                    ts_type, ctx,
                                                                );
                                                            return Ok(Expr::JsonParseTyped {
                                                                text: Box::new(text),
                                                                ty: resolved,
                                                                ordered_keys,
                                                            });
                                                        }
                                                    }
                                                }
                                                return Ok(Expr::JsonParse(Box::new(text)));
                                            }
                                        }
                                        "stringify" => {
                                            if args.len() >= 2 {
                                                let mut it = args.into_iter();
                                                let value = it.next().unwrap();
                                                let replacer = it.next().unwrap();
                                                let spacer = it.next().unwrap_or(Expr::Null);
                                                return Ok(Expr::JsonStringifyFull(
                                                    Box::new(value),
                                                    Box::new(replacer),
                                                    Box::new(spacer),
                                                ));
                                            } else if args.len() == 1 {
                                                let value = args.into_iter().next().unwrap();
                                                // Route ALL single-arg stringify through JsonStringifyFull
                                                // so the runtime can return TAG_UNDEFINED for undefined input
                                                return Ok(Expr::JsonStringifyFull(
                                                    Box::new(value),
                                                    Box::new(Expr::Null),
                                                    Box::new(Expr::Null),
                                                ));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for performance.now()
                            if obj_ident.sym.as_ref() == "performance" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    if method_ident.sym.as_ref() == "now" {
                                        return Ok(Expr::PerformanceNow);
                                    }
                                }
                            }

                            // Check for Response.json(value) / Response.redirect(url, status?) static factories
                            if obj_ident.sym.as_ref() == "Response" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "json" | "redirect" => {
                                            ctx.uses_fetch = true;
                                            return Ok(Expr::NativeMethodCall {
                                                module: "fetch".to_string(),
                                                class_name: Some("Response".to_string()),
                                                object: None,
                                                method: format!("static_{}", method_name),
                                                args,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            // Check for Math.methodName() calls
                            if obj_ident.sym.as_ref() == "Math" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "floor" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathFloor(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "ceil" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathCeil(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "round" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathRound(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "trunc" => {
                                            // Math.trunc(x) = x >= 0 ? floor(x) : ceil(x)
                                            if args.len() >= 1 {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Conditional {
                                                    condition: Box::new(Expr::Compare {
                                                        op: crate::CompareOp::Ge,
                                                        left: Box::new(arg.clone()),
                                                        right: Box::new(Expr::Number(0.0)),
                                                    }),
                                                    then_expr: Box::new(Expr::MathFloor(Box::new(arg.clone()))),
                                                    else_expr: Box::new(Expr::MathCeil(Box::new(arg))),
                                                });
                                            }
                                        }
                                        "sign" => {
                                            // Math.sign(x) = x > 0 ? 1 : x < 0 ? -1 : 0 (or x for NaN)
                                            if args.len() >= 1 {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Conditional {
                                                    condition: Box::new(Expr::Compare {
                                                        op: crate::CompareOp::Gt,
                                                        left: Box::new(arg.clone()),
                                                        right: Box::new(Expr::Number(0.0)),
                                                    }),
                                                    then_expr: Box::new(Expr::Number(1.0)),
                                                    else_expr: Box::new(Expr::Conditional {
                                                        condition: Box::new(Expr::Compare {
                                                            op: crate::CompareOp::Lt,
                                                            left: Box::new(arg.clone()),
                                                            right: Box::new(Expr::Number(0.0)),
                                                        }),
                                                        then_expr: Box::new(Expr::Number(-1.0)),
                                                        else_expr: Box::new(arg),
                                                    }),
                                                });
                                            }
                                        }
                                        "abs" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAbs(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "sqrt" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathSqrt(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log2" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog2(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log10" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog10(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "pow" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let base = args_iter.next().unwrap();
                                                let exp = args_iter.next().unwrap();
                                                return Ok(Expr::MathPow(Box::new(base), Box::new(exp)));
                                            }
                                        }
                                        "min" => {
                                            if has_spread && args.len() == 1 {
                                                return Ok(Expr::MathMinSpread(Box::new(args.into_iter().next().unwrap())));
                                            }
                                            return Ok(Expr::MathMin(args));
                                        }
                                        "max" => {
                                            if has_spread && args.len() == 1 {
                                                return Ok(Expr::MathMaxSpread(Box::new(args.into_iter().next().unwrap())));
                                            }
                                            return Ok(Expr::MathMax(args));
                                        }
                                        "random" => {
                                            return Ok(Expr::MathRandom);
                                        }
                                        "imul" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let a = args_iter.next().unwrap();
                                                let b = args_iter.next().unwrap();
                                                return Ok(Expr::MathImul(Box::new(a), Box::new(b)));
                                            }
                                        }
                                        "sin" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathSin(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "cos" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathCos(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "tan" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathTan(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "asin" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAsin(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "acos" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAcos(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "atan" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAtan(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "atan2" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let y = args_iter.next().unwrap();
                                                let x = args_iter.next().unwrap();
                                                return Ok(Expr::MathAtan2(Box::new(y), Box::new(x)));
                                            }
                                        }
                                        "cbrt" => { if args.len() >= 1 { return Ok(Expr::MathCbrt(Box::new(args.into_iter().next().unwrap()))); } }
                                        "hypot" => { return Ok(Expr::MathHypot(args)); }
                                        "fround" => { if args.len() >= 1 { return Ok(Expr::MathFround(Box::new(args.into_iter().next().unwrap()))); } }
                                        "clz32" => { if args.len() >= 1 { return Ok(Expr::MathClz32(Box::new(args.into_iter().next().unwrap()))); } }
                                        "expm1" => { if args.len() >= 1 { return Ok(Expr::MathExpm1(Box::new(args.into_iter().next().unwrap()))); } }
                                        "log1p" => { if args.len() >= 1 { return Ok(Expr::MathLog1p(Box::new(args.into_iter().next().unwrap()))); } }
                                        "sinh" => { if args.len() >= 1 { return Ok(Expr::MathSinh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "cosh" => { if args.len() >= 1 { return Ok(Expr::MathCosh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "tanh" => { if args.len() >= 1 { return Ok(Expr::MathTanh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "asinh" => { if args.len() >= 1 { return Ok(Expr::MathAsinh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "acosh" => { if args.len() >= 1 { return Ok(Expr::MathAcosh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "atanh" => { if args.len() >= 1 { return Ok(Expr::MathAtanh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "exp" => { if args.len() >= 1 { return Ok(Expr::MathExp(Box::new(args.into_iter().next().unwrap()))); } }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Number.methodName() static calls
                            if obj_ident.sym.as_ref() == "Number" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "isNaN" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsNaN(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isFinite" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsFinite(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isInteger" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsInteger(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isSafeInteger" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsSafeInteger(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parseFloat" => {
                                            // Number.parseFloat is the same as global parseFloat
                                            if args.len() >= 1 {
                                                return Ok(Expr::ParseFloat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parseInt" => {
                                            // Number.parseInt is the same as global parseInt
                                            let mut iter = args.into_iter();
                                            let string_arg = if let Some(s) = iter.next() { Box::new(s) } else { return Err(anyhow!("Number.parseInt requires at least one argument")); };
                                            let radix_arg = iter.next().map(Box::new);
                                            return Ok(Expr::ParseInt { string: string_arg, radix: radix_arg });
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for String.methodName() static calls
                            if obj_ident.sym.as_ref() == "String" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "fromCharCode" => {
                                            if args.len() == 1 {
                                                return Ok(Expr::StringFromCharCode(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() > 1 {
                                                // Multi-arg: concat each char as a separate fromCharCode call
                                                let mut iter = args.into_iter();
                                                let mut acc = Expr::StringFromCharCode(Box::new(iter.next().unwrap()));
                                                for arg in iter {
                                                    acc = Expr::Binary {
                                                        op: crate::ir::BinaryOp::Add,
                                                        left: Box::new(acc),
                                                        right: Box::new(Expr::StringFromCharCode(Box::new(arg))),
                                                    };
                                                }
                                                return Ok(acc);
                                            }
                                        }
                                        "fromCodePoint" => {
                                            if args.len() == 1 {
                                                return Ok(Expr::StringFromCodePoint(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() > 1 {
                                                let mut iter = args.into_iter();
                                                let mut acc = Expr::StringFromCodePoint(Box::new(iter.next().unwrap()));
                                                for arg in iter {
                                                    acc = Expr::Binary {
                                                        op: crate::ir::BinaryOp::Add,
                                                        left: Box::new(acc),
                                                        right: Box::new(Expr::StringFromCodePoint(Box::new(arg))),
                                                    };
                                                }
                                                return Ok(acc);
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for crypto.methodName() calls (including require('crypto') aliases)
                            let is_crypto_module = obj_name == "crypto" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("crypto");
                            if is_crypto_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "randomBytes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoRandomBytes(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "randomUUID" => {
                                            return Ok(Expr::CryptoRandomUUID);
                                        }
                                        "sha256" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoSha256(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "md5" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoMd5(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        // `crypto.getRandomValues(buf)` fills the buffer
                                        // in-place with random bytes and returns it.
                                        // Lower as a synthetic instance method call so
                                        // the runtime buffer dispatcher (added in
                                        // perry-runtime/src/object.rs) handles it via
                                        // `js_buffer_fill_random`.
                                        "getRandomValues" => {
                                            if args.len() >= 1 {
                                                let buf_arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(buf_arg),
                                                        property: "$$cryptoFillRandom".to_string(),
                                                    }),
                                                    args: vec![],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os.methodName() calls (including require('os') aliases)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => {
                                            return Ok(Expr::OsPlatform);
                                        }
                                        "arch" => {
                                            return Ok(Expr::OsArch);
                                        }
                                        "hostname" => {
                                            return Ok(Expr::OsHostname);
                                        }
                                        "homedir" => {
                                            return Ok(Expr::OsHomedir);
                                        }
                                        "tmpdir" => {
                                            return Ok(Expr::OsTmpdir);
                                        }
                                        "totalmem" => {
                                            return Ok(Expr::OsTotalmem);
                                        }
                                        "freemem" => {
                                            return Ok(Expr::OsFreemem);
                                        }
                                        "uptime" => {
                                            return Ok(Expr::OsUptime);
                                        }
                                        "type" => {
                                            return Ok(Expr::OsType);
                                        }
                                        "release" => {
                                            return Ok(Expr::OsRelease);
                                        }
                                        "cpus" => {
                                            return Ok(Expr::OsCpus);
                                        }
                                        "networkInterfaces" => {
                                            return Ok(Expr::OsNetworkInterfaces);
                                        }
                                        "userInfo" => {
                                            return Ok(Expr::OsUserInfo);
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer.methodName() static calls
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding,
                                            });
                                        }
                                        "alloc" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let size = args_iter.next().unwrap();
                                                let fill = args_iter.next().map(Box::new);
                                                return Ok(Expr::BufferAlloc {
                                                    size: Box::new(size),
                                                    fill,
                                                });
                                            }
                                        }
                                        "allocUnsafe" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferAllocUnsafe(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "concat" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferConcat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferIsBuffer(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "byteLength" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferByteLength(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        // `Buffer.compare(a, b)` returns -1/0/1. The runtime
                                        // dispatch already handles `a.compare(b)` as an
                                        // instance method routing through `js_buffer_compare`.
                                        // Synthesize that form so we don't need a dedicated
                                        // HIR variant or runtime entry point.
                                        "compare" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let a = iter.next().unwrap();
                                                let b = iter.next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(a),
                                                        property: "compare".to_string(),
                                                    }),
                                                    args: vec![b],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for child_process named imports (execSync, spawnSync, spawn, exec)
                            let is_child_process_module = ctx.lookup_builtin_module_alias(obj_name) == Some("child_process");
                            if is_child_process_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "execSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExecSync {
                                                    command: Box::new(command),
                                                    options,
                                                });
                                            }
                                        }
                                        "spawnSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnSync {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "spawn" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawn {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "exec" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                let callback = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExec {
                                                    command: Box::new(command),
                                                    options,
                                                    callback,
                                                });
                                            }
                                        }
                                        "spawnBackground" => {
                                            if args.len() >= 3 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let log_file = args_iter.next().unwrap();
                                                let env_json = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnBackground {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    log_file: Box::new(log_file),
                                                    env_json,
                                                });
                                            }
                                        }
                                        "getProcessStatus" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessGetProcessStatus(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "killProcess" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessKillProcess(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net.methodName() calls
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let mut args_iter = args.into_iter();
                                            let options = args_iter.next().map(Box::new);
                                            let connection_listener = args_iter.next().map(Box::new);
                                            return Ok(Expr::NetCreateServer {
                                                options,
                                                connection_listener,
                                            });
                                        }
                                        // createConnection/connect: see sibling site above —
                                        // falls through to generic NativeMethodCall so the LLVM
                                        // backend's NATIVE_MODULE_TABLE dispatch can handle it.
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for AbortSignal.timeout(ms) static method call
                            if obj_ident.sym.as_ref() == "AbortSignal" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "timeout" {
                                        return Ok(Expr::StaticMethodCall {
                                            class_name: "AbortSignal".to_string(),
                                            method_name: "timeout".to_string(),
                                            args,
                                        });
                                    }
                                }
                            }

                            // Check for Date.now() / Date.parse() / Date.UTC() static method calls
                            if obj_ident.sym.as_ref() == "Date" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "now" {
                                        return Ok(Expr::DateNow);
                                    }
                                    if method_name == "parse" {
                                        if args.len() >= 1 {
                                            return Ok(Expr::DateParse(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    if method_name == "UTC" {
                                        return Ok(Expr::DateUtc(args));
                                    }
                                }
                            }
                        }

                        // Check for Date instance method calls (date.getTime(), etc.)
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "getTime" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetTime(Box::new(date_expr)));
                                }
                                "toISOString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToISOString(Box::new(date_expr)));
                                }
                                "getFullYear" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetFullYear(Box::new(date_expr)));
                                }
                                "getMonth" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMonth(Box::new(date_expr)));
                                }
                                "getDate" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetDate(Box::new(date_expr)));
                                }
                                "getHours" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetHours(Box::new(date_expr)));
                                }
                                "getMinutes" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMinutes(Box::new(date_expr)));
                                }
                                "getSeconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetSeconds(Box::new(date_expr)));
                                }
                                "getMilliseconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMilliseconds(Box::new(date_expr)));
                                }
                                // UTC getters
                                "getUTCDay" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcDay(Box::new(date_expr)));
                                }
                                "getUTCFullYear" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcFullYear(Box::new(date_expr)));
                                }
                                "getUTCMonth" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMonth(Box::new(date_expr)));
                                }
                                "getUTCDate" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcDate(Box::new(date_expr)));
                                }
                                "getUTCHours" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcHours(Box::new(date_expr)));
                                }
                                "getUTCMinutes" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMinutes(Box::new(date_expr)));
                                }
                                "getUTCSeconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcSeconds(Box::new(date_expr)));
                                }
                                "getUTCMilliseconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMilliseconds(Box::new(date_expr)));
                                }
                                // Other getters/methods
                                "valueOf" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateValueOf(Box::new(date_expr)));
                                }
                                "toDateString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToDateString(Box::new(date_expr)));
                                }
                                "toTimeString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToTimeString(Box::new(date_expr)));
                                }
                                "toLocaleDateString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleDateString(Box::new(date_expr)));
                                }
                                "toLocaleTimeString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleTimeString(Box::new(date_expr)));
                                }
                                "toLocaleString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleString(Box::new(date_expr)));
                                }
                                "getTimezoneOffset" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetTimezoneOffset(Box::new(date_expr)));
                                }
                                "toJSON" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToJSON(Box::new(date_expr)));
                                }
                                // UTC setters — mutate the local variable in place
                                "setUTCFullYear" | "setUTCMonth" | "setUTCDate" |
                                "setUTCHours" | "setUTCMinutes" | "setUTCSeconds" |
                                "setUTCMilliseconds" => {
                                    if args.len() >= 1 {
                                        let value_expr = args.into_iter().next().unwrap();
                                        let date_expr = lower_expr(ctx, &member.obj)?;
                                        let setter_call = match method_name {
                                            "setUTCFullYear" => Expr::DateSetUtcFullYear { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMonth" => Expr::DateSetUtcMonth { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCDate" => Expr::DateSetUtcDate { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCHours" => Expr::DateSetUtcHours { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMinutes" => Expr::DateSetUtcMinutes { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCSeconds" => Expr::DateSetUtcSeconds { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMilliseconds" => Expr::DateSetUtcMilliseconds { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            _ => unreachable!(),
                                        };
                                        // If receiver is a local variable, mutate it in place by wrapping
                                        // the setter result in a LocalSet so the new timestamp is stored back.
                                        if let Expr::LocalGet(local_id) = &date_expr {
                                            return Ok(Expr::LocalSet(*local_id, Box::new(setter_call)));
                                        }
                                        return Ok(setter_call);
                                    }
                                }
                                _ => {} // Fall through to other handling
                            }
                        }

                        // Check for WeakRef.deref() / FinalizationRegistry.register() / .unregister()
                        // dispatch BEFORE the generic array method dispatch — these receivers were
                        // tracked in the pre-scan pass.
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if let ast::Expr::Ident(recv_ident) = member.obj.as_ref() {
                                let recv_name = recv_ident.sym.to_string();
                                if ctx.weakref_locals.contains(&recv_name) && method_name == "deref" {
                                    return Ok(Expr::WeakRefDeref(Box::new(Expr::LocalGet(
                                        ctx.lookup_local(&recv_name).unwrap_or(0),
                                    ))));
                                }
                                if ctx.finreg_locals.contains(&recv_name) {
                                    let registry_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    match method_name {
                                        "register" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let target = iter.next().unwrap();
                                                let held = iter.next().unwrap();
                                                let token = iter.next().map(Box::new);
                                                return Ok(Expr::FinalizationRegistryRegister {
                                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                                    target: Box::new(target),
                                                    held: Box::new(held),
                                                    token,
                                                });
                                            }
                                        }
                                        "unregister" => {
                                            if args.len() >= 1 {
                                                let token = args.into_iter().next().unwrap();
                                                return Ok(Expr::FinalizationRegistryUnregister {
                                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                                    token: Box::new(token),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                // WeakMap/WeakSet — route to dedicated runtime functions
                                // (NOT the regular Map/Set HIR variants) so reference-equality
                                // works for object keys. Primitive keys/values throw via
                                // js_weak_throw_primitive when the AST shows a bare literal.
                                let make_extern_call = |name: &str, args: Vec<Expr>| -> Expr {
                                    Expr::Call {
                                        callee: Box::new(Expr::ExternFuncRef {
                                            name: name.to_string(),
                                            param_types: Vec::new(),
                                            return_type: Type::Any,
                                        }),
                                        args,
                                        type_args: Vec::new(),
                                    }
                                };
                                let throw_primitive_expr = || -> Expr {
                                    Expr::Call {
                                        callee: Box::new(Expr::ExternFuncRef {
                                            name: "js_weak_throw_primitive".to_string(),
                                            param_types: Vec::new(),
                                            return_type: Type::Any,
                                        }),
                                        args: Vec::new(),
                                        type_args: Vec::new(),
                                    }
                                };
                                if ctx.weakmap_locals.contains(&recv_name) {
                                    let map_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    let recv = Expr::LocalGet(map_id);
                                    match method_name {
                                        "set" if args.len() >= 2 => {
                                            let key_is_primitive_lit = matches!(
                                                call.args.get(0).map(|a| a.expr.as_ref()),
                                                Some(ast::Expr::Lit(_))
                                            );
                                            if key_is_primitive_lit {
                                                return Ok(throw_primitive_expr());
                                            }
                                            let mut iter = args.into_iter();
                                            let key = iter.next().unwrap();
                                            let value = iter.next().unwrap();
                                            return Ok(make_extern_call("js_weakmap_set", vec![recv, key, value]));
                                        }
                                        "get" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_get", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "has" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_has", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "delete" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_delete", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        _ => {}
                                    }
                                }
                                if ctx.weakset_locals.contains(&recv_name) {
                                    let set_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    let recv = Expr::LocalGet(set_id);
                                    match method_name {
                                        "add" if args.len() >= 1 => {
                                            let value_is_primitive_lit = matches!(
                                                call.args.get(0).map(|a| a.expr.as_ref()),
                                                Some(ast::Expr::Lit(_))
                                            );
                                            if value_is_primitive_lit {
                                                return Ok(throw_primitive_expr());
                                            }
                                            return Ok(make_extern_call("js_weakset_add", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "has" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakset_has", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "delete" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakset_delete", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        // Check for array method calls (arr.push, arr.pop, etc.)
                        // These are called on local variables, not global modules
                        // IMPORTANT: Only apply to actual Array types, not String types
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                let arr_name = arr_ident.sym.to_string();
                                // Check that this is NOT a String type (Array, Set, Map are all OK)
                                // When type is unknown, only enter array block for array-only methods
                                // (push, pop, etc.), NOT for methods shared with strings (indexOf,
                                // includes, split) — those are handled by the general dispatch which
                                // checks is_string at codegen time.
                                let type_info = ctx.lookup_local_type(&arr_name);
                                // `Union<String, Void>` (e.g. `JSON.stringify` return type) is
                                // a possible-string — must NOT be treated as definitely not-a-
                                // string, otherwise `.indexOf`/`.includes` get routed through
                                // ArrayIndexOf/ArrayIncludes and return -1/false on a real
                                // string value.
                                let is_union_with_string = matches!(
                                    type_info,
                                    Some(Type::Union(variants)) if variants.iter().any(|v| matches!(v, Type::String))
                                );
                                let is_known_string = type_info.map(|ty| matches!(ty, Type::String)).unwrap_or(false)
                                    || is_union_with_string;
                                // A user-defined class instance is NOT an array — must skip the array
                                // fast path so user-defined methods like Stack<T>.push() are dispatched
                                // to the class method, not runtime js_array_push. Map/Set/Promise are
                                // handled by explicit checks within the array block below.
                                let builtin_generic_bases = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                                let is_user_class_instance = match type_info {
                                    Some(Type::Named(name)) => ctx.lookup_class(name).is_some(),
                                    Some(Type::Generic { base, .. }) => {
                                        !builtin_generic_bases.contains(&base.as_str())
                                            && ctx.lookup_class(base).is_some()
                                    }
                                    _ => false,
                                };
                                let is_known_not_string = type_info
                                    .map(|ty| !matches!(ty, Type::String | Type::Any | Type::Unknown))
                                    .unwrap_or(false)
                                    && !is_union_with_string;
                                // Object type literals (e.g., { push: (v: number) => void; ... })
                                // are NOT arrays — they are plain objects with closure-valued
                                // properties and must NOT enter the array fast path.
                                let is_object_type = matches!(type_info, Some(Type::Object(_)));
                                // `Uint8Array`/`Buffer` instances must NOT enter the generic
                                // array fast path. They have a distinct runtime representation
                                // (raw `BufferHeader`, no f64 elements) and a different method
                                // family (`readUInt8`, `swap16`, byte-level `indexOf` matching
                                // string/buffer needles, etc.). The runtime's
                                // `dispatch_buffer_method` handles all of these via the
                                // universal `js_native_call_method` fallback path.
                                let is_buffer_type = matches!(
                                    type_info,
                                    Some(Type::Named(n))
                                        if n == "Uint8Array" || n == "Buffer" || n == "Uint8ClampedArray"
                                );
                                let is_ambiguous_method = matches!(method_name,
                                    "indexOf" | "includes" | "slice"
                                );
                                let is_not_string = if is_known_string {
                                    false  // definitely a string, skip array block
                                } else if is_user_class_instance {
                                    false  // user class — must dispatch to class method, skip array fast-path
                                } else if is_object_type {
                                    false  // object type literal — dispatch via method call, not array ops
                                } else if is_buffer_type {
                                    false  // Buffer/Uint8Array — runtime dispatch handles byte-level methods
                                } else if is_known_not_string {
                                    true   // definitely not a string, enter array block
                                } else if is_ambiguous_method {
                                    false  // type unknown + ambiguous method, skip array block (fall through to general dispatch)
                                } else {
                                    true   // type unknown + array-only method (push, pop, etc.), enter array block
                                };
                                // Helper: if the callback arg is a bare Boolean/Number/String identifier,
                                // desugar to a synthetic closure: x => Boolean(x) / Number(x) / String(x).
                                // This is needed because .filter(Boolean) etc. expect a closure pointer at
                                // runtime but built-in constructors aren't first-class closure objects.
                                if is_not_string {
                                if let Some(array_id) = ctx.lookup_local(&arr_name) {
                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // Check if any argument has spread operator —
                                                // when present, route through the spread path.
                                                // Multi-arg push without spread is desugared to a
                                                // Sequence of ArrayPush statements (one per arg);
                                                // JS spec returns the final array length, which is
                                                // exactly what the last ArrayPush returns.
                                                let any_spread = call.args.iter().any(|a| a.spread.is_some());
                                                if any_spread {
                                                    if args.len() == 1 {
                                                        return Ok(Expr::ArrayPushSpread {
                                                            array_id,
                                                            source: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                    // Mixed regular + spread: bail to generic
                                                    // dispatch (no current single-IR-shape).
                                                } else {
                                                    if args.len() == 1 {
                                                        return Ok(Expr::ArrayPush {
                                                            array_id,
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                    let mut stmts: Vec<Expr> = Vec::with_capacity(args.len());
                                                    for a in args.into_iter() {
                                                        stmts.push(Expr::ArrayPush {
                                                            array_id,
                                                            value: Box::new(a),
                                                        });
                                                    }
                                                    return Ok(Expr::Sequence(stmts));
                                                }
                                            }
                                        }
                                        "pop" => {
                                            return Ok(Expr::ArrayPop(array_id));
                                        }
                                        "shift" => {
                                            return Ok(Expr::ArrayShift(array_id));
                                        }
                                        "unshift" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayUnshift {
                                                    array_id,
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            // arr.slice(start, end?) - returns new array
                                            // Only convert to ArraySlice if we KNOW it's an Array type
                                            // (Type::Any could be a string, which has its own .slice() method)
                                            let is_definitely_array = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if is_definitely_array && args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                            // Fall through to normal Call handling for strings or unknown types
                                        }
                                        "splice" => {
                                            // arr.splice(start, deleteCount?, ...items) - returns deleted elements
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArraySplice {
                                                    array_id,
                                                    start: Box::new(start),
                                                    delete_count: delete_count.map(Box::new),
                                                    items,
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if !is_map_or_set && args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "map" => {
                                            // Only use ArrayMap if receiver is not a class instance
                                            let is_class_instance = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Named(_) | Type::Generic { .. }) && !matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if !is_class_instance && args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "findIndex" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFindIndex {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "findLast" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFindLast {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "findLastIndex" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFindLastIndex {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "at" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayAt {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    index: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "some" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArraySome {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "every" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayEvery {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "flatMap" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFlatMap {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "join" => {
                                            // arr.join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                separator,
                                            });
                                        }
                                        "flat" => {
                                            // arr.flat() -> flattened array
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                        "reduceRight" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduceRight {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "toReversed" => {
                                            return Ok(Expr::ArrayToReversed {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                        "toSorted" => {
                                            let comparator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayToSorted {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                comparator,
                                            });
                                        }
                                        "toSpliced" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next().unwrap();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArrayToSpliced {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    start: Box::new(start),
                                                    delete_count: Box::new(delete_count),
                                                    items,
                                                });
                                            }
                                        }
                                        "with" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let index = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::ArrayWith {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    index: Box::new(index),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "copyWithin" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let target = args_iter.next().unwrap();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayCopyWithin {
                                                    array_id,
                                                    target: Box::new(target),
                                                    start: Box::new(start),
                                                    end,
                                                });
                                            }
                                        }
                                        "entries" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayEntries(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        "keys" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayKeys(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        "values" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set {
                                                return Ok(Expr::SetValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayValues(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        // Map methods (only apply to actual Map/Set types)
                                        "set" => {
                                            // Check if this is a Map or Set type before treating as Map.set()
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if is_map_or_set && args.len() >= 2 {
                                                // map.set(key, value) - returns the map for chaining
                                                let mut args_iter = args.into_iter();
                                                let key = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::MapSet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(key),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "get" => {
                                            // Check if this is a Map type before treating as Map.get()
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.len() >= 1 {
                                                // map.get(key) - returns value or undefined
                                                return Ok(Expr::MapGet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "has" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetHas {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapHas {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "delete" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetDelete {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapDelete {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "clear" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_set {
                                                return Ok(Expr::SetClear(Box::new(Expr::LocalGet(array_id))));
                                            } else if is_map {
                                                return Ok(Expr::MapClear(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            // Fall through if neither Set nor Map
                                        }
                                        // Map iterator methods: entries(), keys(), values()
                                        "entries" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "keys" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "values" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.is_empty() {
                                                return Ok(Expr::SetValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        // Set methods
                                        "add" => {
                                            // Check if this is a Set type before treating as Set.add()
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.len() >= 1 {
                                                // set.add(value) - returns the set for chaining
                                                let value = args.into_iter().next().unwrap();
                                                return Ok(Expr::SetAdd {
                                                    set_id: array_id,
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }

                                    // URLSearchParams methods
                                    let is_url_search_params = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "URLSearchParams"))
                                        .unwrap_or(false);
                                    if is_url_search_params {
                                        match method_name {
                                            "get" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "has" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsHas {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "set" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsSet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "append" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsAppend {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "delete" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsDelete {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "toString" => {
                                                return Ok(Expr::UrlSearchParamsToString(
                                                    Box::new(Expr::LocalGet(array_id))
                                                ));
                                            }
                                            "getAll" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGetAll {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // TextEncoder methods
                                    let is_text_encoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                        .unwrap_or(false);
                                    if is_text_encoder {
                                        match method_name {
                                            "encode" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::TextEncoderEncode(Box::new(args.into_iter().next().unwrap())));
                                                } else {
                                                    // encode() with no args encodes empty string
                                                    return Ok(Expr::TextEncoderEncode(Box::new(Expr::String(String::new()))));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // TextDecoder methods
                                    let is_text_decoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                        .unwrap_or(false);
                                    if is_text_decoder {
                                        match method_name {
                                            "decode" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                                } else {
                                                    // decode() with no args returns empty string
                                                    return Ok(Expr::String(String::new()));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                }  // close is_array_type check
                            }

                            // Check for array methods on property access (e.g., this.items.push(value))
                            // This handles cases where the array is a property of an object, not a local variable
                            if let ast::Expr::Member(obj_member) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(obj_prop_ident) = &obj_member.prop {
                                    let property_name = obj_prop_ident.sym.to_string();
                                    // Lower the object expression (e.g., 'this' or a local variable)
                                    let object_expr = lower_expr(ctx, &obj_member.obj)?;

                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // For now, fall through to generic Call handling
                                                // We'll compile this in codegen using inline property access
                                                // property-based push: object.{property}.push()
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on imported variables (e.g., import { CHAIN_NAMES } from './module')
                    // These don't have local IDs but are ExternFuncRef values
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                    let arr_name = arr_ident.sym.to_string();
                                    // Check if this is an imported variable (not a local)
                                    if ctx.lookup_local(&arr_name).is_none() {
                                        if let Some(orig_name) = ctx.lookup_imported_func(&arr_name) {
                                            // This is an imported variable - create ExternFuncRef for it
                                            let (param_types, return_type) = ctx.lookup_extern_func_types(orig_name)
                                                .map(|(p, r)| (p.clone(), r.clone()))
                                                .unwrap_or_else(|| (Vec::new(), Type::Any));
                                            let extern_ref = Expr::ExternFuncRef {
                                                name: orig_name.to_string(),
                                                param_types,
                                                return_type,
                                            };
                                            match method_name {
                                                "join" => {
                                                    // arr.join(separator?) -> string
                                                    let separator = args.into_iter().next().map(Box::new);
                                                    return Ok(Expr::ArrayJoin {
                                                        array: Box::new(extern_ref),
                                                        separator,
                                                    });
                                                }
                                                "map" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayMap {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "filter" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayFilter {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "forEach" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayForEach {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "find" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayFind {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "sort" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArraySort {
                                                            array: Box::new(extern_ref),
                                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "indexOf" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIndexOf {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "includes" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIncludes {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "slice" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let start = args_iter.next().unwrap();
                                                        let end = args_iter.next();
                                                        return Ok(Expr::ArraySlice {
                                                            array: Box::new(extern_ref),
                                                            start: Box::new(start),
                                                            end: end.map(Box::new),
                                                        });
                                                    }
                                                }
                                                "reduce" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let callback = args_iter.next().unwrap();
                                                        let initial = args_iter.next().map(Box::new);
                                                        return Ok(Expr::ArrayReduce {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(callback),
                                                            initial,
                                                        });
                                                    }
                                                }
                                                "flat" => {
                                                    return Ok(Expr::ArrayFlat {
                                                        array: Box::new(extern_ref),
                                                    });
                                                }
                                                "reduceRight" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let callback = args_iter.next().unwrap();
                                                        let initial = args_iter.next().map(Box::new);
                                                        return Ok(Expr::ArrayReduceRight {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(callback),
                                                            initial,
                                                        });
                                                    }
                                                }
                                                "toReversed" => {
                                                    return Ok(Expr::ArrayToReversed {
                                                        array: Box::new(extern_ref),
                                                    });
                                                }
                                                "toSorted" => {
                                                    let comparator = args.into_iter().next().map(Box::new);
                                                    return Ok(Expr::ArrayToSorted {
                                                        array: Box::new(extern_ref),
                                                        comparator,
                                                    });
                                                }
                                                "toSpliced" => {
                                                    if args.len() >= 2 {
                                                        let mut args_iter = args.into_iter();
                                                        let start = args_iter.next().unwrap();
                                                        let delete_count = args_iter.next().unwrap();
                                                        let items: Vec<Expr> = args_iter.collect();
                                                        return Ok(Expr::ArrayToSpliced {
                                                            array: Box::new(extern_ref),
                                                            start: Box::new(start),
                                                            delete_count: Box::new(delete_count),
                                                            items,
                                                        });
                                                    }
                                                }
                                                "with" => {
                                                    if args.len() >= 2 {
                                                        let mut args_iter = args.into_iter();
                                                        let index = args_iter.next().unwrap();
                                                        let value = args_iter.next().unwrap();
                                                        return Ok(Expr::ArrayWith {
                                                            array: Box::new(extern_ref),
                                                            index: Box::new(index),
                                                            value: Box::new(value),
                                                        });
                                                    }
                                                }
                                                "entries" => {
                                                    return Ok(Expr::ArrayEntries(Box::new(extern_ref)));
                                                }
                                                "keys" => {
                                                    return Ok(Expr::ArrayKeys(Box::new(extern_ref)));
                                                }
                                                "values" => {
                                                    return Ok(Expr::ArrayValues(Box::new(extern_ref)));
                                                }
                                                _ => {} // Fall through for other methods
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on inline array literals (e.g., ['a', 'b'].join('-'))
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Array(_arr_lit) = member.obj.as_ref() {
                                    // Lower the array literal
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    match method_name {
                                        "join" => {
                                            // ['a', 'b'].join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(array_expr),
                                                separator,
                                            });
                                        }
                                        "map" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(array_expr),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(array_expr),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "flat" => {
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(array_expr),
                                            });
                                        }
                                        "reduceRight" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduceRight {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "toReversed" => {
                                            return Ok(Expr::ArrayToReversed {
                                                array: Box::new(array_expr),
                                            });
                                        }
                                        "toSorted" => {
                                            let comparator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayToSorted {
                                                array: Box::new(array_expr),
                                                comparator,
                                            });
                                        }
                                        "toSpliced" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next().unwrap();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArrayToSpliced {
                                                    array: Box::new(array_expr),
                                                    start: Box::new(start),
                                                    delete_count: Box::new(delete_count),
                                                    items,
                                                });
                                            }
                                        }
                                        "with" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let index = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::ArrayWith {
                                                    array: Box::new(array_expr),
                                                    index: Box::new(index),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "entries" => {
                                            return Ok(Expr::ArrayEntries(Box::new(array_expr)));
                                        }
                                        "keys" => {
                                            return Ok(Expr::ArrayKeys(Box::new(array_expr)));
                                        }
                                        "values" => {
                                            return Ok(Expr::ArrayValues(Box::new(array_expr)));
                                        }
                                        _ => {} // Fall through for other methods
                                    }
                                }
                            }
                        }
                    }

                    // TextEncoder.encode() / TextDecoder.decode() on inline expressions
                    // e.g., new TextEncoder().encode("hello"), new TextDecoder().decode(buf)
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                // Check if the receiver is new TextEncoder() or new TextDecoder()
                                if let ast::Expr::New(new_expr) = member.obj.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                        let class_name = class_ident.sym.as_ref();
                                        if class_name == "TextEncoder" && method_name == "encode" {
                                            let str_arg = if args.len() >= 1 {
                                                args.into_iter().next().unwrap()
                                            } else {
                                                Expr::String(String::new())
                                            };
                                            return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                                        }
                                        if class_name == "TextDecoder" && method_name == "decode" {
                                            if args.len() >= 1 {
                                                return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                            } else {
                                                return Ok(Expr::String(String::new()));
                                            }
                                        }
                                    }
                                }
                                // Also check for local variable typed as TextEncoder/TextDecoder
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.to_string();
                                    let is_text_encoder = ctx.lookup_local_type(&obj_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                        .unwrap_or(false);
                                    if is_text_encoder && method_name == "encode" {
                                        let str_arg = if args.len() >= 1 {
                                            args.into_iter().next().unwrap()
                                        } else {
                                            Expr::String(String::new())
                                        };
                                        return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                                    }
                                    let is_text_decoder = ctx.lookup_local_type(&obj_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                        .unwrap_or(false);
                                    if is_text_decoder && method_name == "decode" {
                                        if args.len() >= 1 {
                                            return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                        } else {
                                            return Ok(Expr::String(String::new()));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for array-only methods on any expression (e.g., Object.entries(x).reduce(...))
                    // ONLY match methods that are unique to arrays (not shared with strings)
                    // "includes", "indexOf", "slice", "join" also exist on strings, so skip those
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                match method_name {
                                    "reduce" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduce {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                    "map" if args.len() >= 1 => {
                                        // Skip if receiver is a known class instance (e.g., Box.map())
                                        // Check both local variables with class types AND new expressions
                                        let is_class_instance = match member.obj.as_ref() {
                                            ast::Expr::Ident(ident) => {
                                                ctx.lookup_local_type(&ident.sym.to_string())
                                                    .map(|ty| matches!(ty, Type::Named(_) | Type::Generic { .. }) && !matches!(ty, Type::Array(_)))
                                                    .unwrap_or(false)
                                            }
                                            ast::Expr::New(_) => {
                                                // new ClassName(...).map() - always a class instance, not an array
                                                true
                                            }
                                            _ => false,
                                        };
                                        if !is_class_instance {
                                            let cb = args.into_iter().next().unwrap();
                                            let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            return Ok(Expr::ArrayMap {
                                                array: Box::new(array_expr),
                                                callback: Box::new(cb),
                                            });
                                        }
                                    }
                                    "filter" if args.len() >= 1 => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFilter {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "forEach" if args.len() >= 1 => {
                                        // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                        let is_map_or_set = if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false)
                                        } else {
                                            false
                                        };
                                        if !is_map_or_set {
                                            let cb = args.into_iter().next().unwrap();
                                            let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            return Ok(Expr::ArrayForEach {
                                                array: Box::new(array_expr),
                                                callback: Box::new(cb),
                                            });
                                        }
                                    }
                                    "find" if args.len() >= 1 => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFind {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "findIndex" if args.len() >= 1 => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFindIndex {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "sort" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArraySort {
                                            array: Box::new(array_expr),
                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    // .slice() exists on both Array and String, so we can only safely
                                    // lower to ArraySlice when the receiver is definitely an
                                    // array-producing expression (matches the indexOf/includes pattern
                                    // below). Without this, `arr.sort(cb).slice(0, 5)` falls through to
                                    // generic dynamic dispatch which corrupts the result — the inner
                                    // ArraySort returns a real array pointer but the outer .slice goes
                                    // through `js_native_call_method` which can't unwrap it properly,
                                    // producing an "object" with the right .length but Array.isArray
                                    // returns false and JSON.stringify segfaults.
                                    "slice" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) | Expr::ArraySpread(_) |
                                            Expr::ArrayFrom(_) | Expr::ArrayFromMapped { .. } |
                                            Expr::ArrayFlat { .. } | Expr::StringSplit(_, _) |
                                            Expr::ArrayToReversed { .. } | Expr::ArrayToSorted { .. } |
                                            Expr::ArrayToSpliced { .. } | Expr::ArrayWith { .. } |
                                            Expr::ArrayEntries(_) | Expr::ArrayKeys(_) | Expr::ArrayValues(_) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) | Expr::ObjectEntries(_) |
                                            // `process.argv` is a `string[]`. Without this arm the
                                            // fallthrough picked String.slice semantics — so
                                            // `process.argv.slice(2)` returned a "string" whose
                                            // length was the argv count and whose elements were
                                            // NaN-box bits of string pointers read as doubles
                                            // (closes #41).
                                            Expr::ProcessArgv
                                        ) {
                                            let mut args_iter = args.into_iter();
                                            let start = args_iter.next().unwrap();
                                            let end = args_iter.next();
                                            return Ok(Expr::ArraySlice {
                                                array: Box::new(array_expr),
                                                start: Box::new(start),
                                                end: end.map(Box::new),
                                            });
                                        }
                                        // Fall through to generic Call handling (could be a String.slice).
                                    }
                                    // .join() is exclusively an Array method (strings don't have it),
                                    // so we can always safely lower to ArrayJoin regardless of the
                                    // receiver expression type. Previously this only matched specific
                                    // array-returning expressions, which caused .split().join() chains
                                    // to fall through to generic dispatch and produce wrong results.
                                    "join" if args.len() <= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let separator = if args.is_empty() { None } else { Some(Box::new(args.into_iter().next().unwrap())) };
                                        return Ok(Expr::ArrayJoin {
                                            array: Box::new(array_expr),
                                            separator,
                                        });
                                    }
                                    "indexOf" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) |
                                            Expr::PropertyGet { .. }
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIndexOf {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "includes" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        // Don't treat error string properties as arrays
                                        let is_error_string_prop = matches!(&array_expr,
                                            Expr::PropertyGet { property, .. }
                                            if matches!(property.as_str(), "stack" | "message" | "name")
                                        );
                                        if !is_error_string_prop && matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) |
                                            Expr::PropertyGet { .. }
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIncludes {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "flat" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFlat {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                    "reduceRight" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduceRight {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                    "toReversed" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayToReversed {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                    "toSorted" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let comparator = args.into_iter().next().map(Box::new);
                                        return Ok(Expr::ArrayToSorted {
                                            array: Box::new(array_expr),
                                            comparator,
                                        });
                                    }
                                    "toSpliced" if args.len() >= 2 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let start = args_iter.next().unwrap();
                                        let delete_count = args_iter.next().unwrap();
                                        let items: Vec<Expr> = args_iter.collect();
                                        return Ok(Expr::ArrayToSpliced {
                                            array: Box::new(array_expr),
                                            start: Box::new(start),
                                            delete_count: Box::new(delete_count),
                                            items,
                                        });
                                    }
                                    "with" if args.len() >= 2 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let index = args_iter.next().unwrap();
                                        let value = args_iter.next().unwrap();
                                        return Ok(Expr::ArrayWith {
                                            array: Box::new(array_expr),
                                            index: Box::new(index),
                                            value: Box::new(value),
                                        });
                                    }
                                    "push" if args.len() >= 1 => {
                                        // Generic expr.push(value) or expr.push(...spread)
                                        // GUARD: Skip if the receiver is a user-defined class instance
                                        // (e.g. Stack<T>.push()), or an object type literal (e.g.
                                        // { push: (v) => void, ... }), so its method dispatches correctly.
                                        let is_user_class_receiver = match member.obj.as_ref() {
                                            ast::Expr::Ident(ident) => {
                                                ctx.lookup_local_type(&ident.sym.to_string()).map(|ty| {
                                                    match ty {
                                                        Type::Named(name) => ctx.lookup_class(name).is_some(),
                                                        Type::Generic { base, .. } => {
                                                            let builtin = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                                                            !builtin.contains(&base.as_str()) && ctx.lookup_class(base).is_some()
                                                        }
                                                        Type::Object(_) => true, // object type literal with push property
                                                        _ => false,
                                                    }
                                                }).unwrap_or(false)
                                            }
                                            ast::Expr::New(_) => true, // new ClassName().push()
                                            _ => false,
                                        };
                                        if !is_user_class_receiver {
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            if call.args.len() >= 1 && call.args[0].spread.is_some() {
                                                return Ok(Expr::NativeMethodCall {
                                                    module: "array".to_string(),
                                                    method: "push_spread".to_string(),
                                                    class_name: None,
                                                    object: Some(Box::new(array_expr)),
                                                    args: args,
                                                });
                                            } else {
                                                return Ok(Expr::NativeMethodCall {
                                                    module: "array".to_string(),
                                                    method: "push_single".to_string(),
                                                    class_name: None,
                                                    object: Some(Box::new(array_expr)),
                                                    args: args,
                                                });
                                            }
                                        }
                                    }
                                    _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                                }
                            }
                        }
                    }

                    // Check for regex .test() / .exec() method call on any expression
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let m = method_ident.sym.as_ref();
                                if (m == "test" || m == "exec") && args.len() == 1 {
                                    // Check if the object is a regex literal or a local assigned to a regex
                                    let is_regex_obj = match member.obj.as_ref() {
                                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                                        ast::Expr::Ident(ident) => {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Any | Type::Unknown) || matches!(ty, Type::Named(n) if n == "RegExp"))
                                                .unwrap_or(true)
                                        }
                                        _ => false,
                                    };
                                    if is_regex_obj {
                                        let regex_expr = lower_expr(ctx, &member.obj)?;
                                        // Only emit RegExp method calls if the object is actually a regex
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            let string_expr = args.into_iter().next().unwrap();
                                            if m == "test" {
                                                return Ok(Expr::RegExpTest {
                                                    regex: Box::new(regex_expr),
                                                    string: Box::new(string_expr),
                                                });
                                            } else {
                                                return Ok(Expr::RegExpExec {
                                                    regex: Box::new(regex_expr),
                                                    string: Box::new(string_expr),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for string .match(regex) method call
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                if (method_ident.sym.as_ref() == "match" || method_ident.sym.as_ref() == "matchAll") && args.len() == 1 {
                                    let is_match_all = method_ident.sym.as_ref() == "matchAll";
                                    // Check if the argument is a regex literal or a local holding a regex
                                    let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                                        Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                                        Some(ast::Expr::Ident(ident)) => {
                                            match ctx.lookup_local_type(&ident.sym.to_string()) {
                                                // Known regex local
                                                Some(Type::Named(n)) if n == "RegExp" => true,
                                                // Unknown type — assume could be regex
                                                Some(Type::Any) | Some(Type::Unknown) | None => true,
                                                _ => false,
                                            }
                                        }
                                        _ => false,
                                    };
                                    if arg_is_regex {
                                        let string_expr = lower_expr(ctx, &member.obj)?;
                                        let regex_expr = args.remove(0);
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            return Ok(if is_match_all {
                                                Expr::StringMatchAll {
                                                    string: Box::new(string_expr),
                                                    regex: Box::new(regex_expr),
                                                }
                                            } else {
                                                Expr::StringMatch {
                                                    string: Box::new(string_expr),
                                                    regex: Box::new(regex_expr),
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite)
                    if let ast::Expr::Ident(ident) = expr.as_ref() {
                        let func_name = ident.sym.as_ref();
                        match func_name {
                            "parseInt" => {
                                let string_arg = if args.len() >= 1 {
                                    Box::new(args.remove(0))
                                } else {
                                    return Err(anyhow!("parseInt requires at least one argument"));
                                };
                                let radix_arg = if !args.is_empty() {
                                    Some(Box::new(args.remove(0)))
                                } else {
                                    None
                                };
                                return Ok(Expr::ParseInt { string: string_arg, radix: radix_arg });
                            }
                            "parseFloat" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::ParseFloat(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("parseFloat requires one argument"));
                                }
                            }
                            "Number" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::NumberCoerce(Box::new(args.remove(0))));
                                } else {
                                    // Number() with no args returns 0
                                    return Ok(Expr::Number(0.0));
                                }
                            }
                            "BigInt" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::BigIntCoerce(Box::new(args.remove(0))));
                                } else {
                                    // BigInt() with no args returns 0n
                                    return Ok(Expr::BigInt("0".to_string()));
                                }
                            }
                            "String" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StringCoerce(Box::new(args.remove(0))));
                                } else {
                                    // String() with no args returns ""
                                    return Ok(Expr::String(String::new()));
                                }
                            }
                            "Boolean" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::BooleanCoerce(Box::new(args.remove(0))));
                                } else {
                                    // Boolean() with no args returns false
                                    return Ok(Expr::Bool(false));
                                }
                            }
                            "isNaN" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsNaN(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isNaN requires one argument"));
                                }
                            }
                            "isFinite" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsFinite(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isFinite requires one argument"));
                                }
                            }
                            "atob" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::Atob(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("atob requires one argument"));
                                }
                            }
                            "btoa" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::Btoa(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("btoa requires one argument"));
                                }
                            }
                            "encodeURI" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::EncodeURI(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("encodeURI requires one argument"));
                                }
                            }
                            "decodeURI" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::DecodeURI(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("decodeURI requires one argument"));
                                }
                            }
                            "encodeURIComponent" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::EncodeURIComponent(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("encodeURIComponent requires one argument"));
                                }
                            }
                            "decodeURIComponent" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::DecodeURIComponent(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("decodeURIComponent requires one argument"));
                                }
                            }
                            "structuredClone" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StructuredClone(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("structuredClone requires one argument"));
                                }
                            }
                            "queueMicrotask" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::QueueMicrotask(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("queueMicrotask requires one argument"));
                                }
                            }
                            "Symbol" => {
                                // Symbol() / Symbol(description)
                                if args.is_empty() {
                                    return Ok(Expr::SymbolNew(None));
                                } else {
                                    return Ok(Expr::SymbolNew(Some(Box::new(args.remove(0)))));
                                }
                            }
                            "perryResolveStaticPlugin" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StaticPluginResolve(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("perryResolveStaticPlugin requires one argument"));
                                }
                            }
                            "fetchWithAuth" => {
                                // fetchWithAuth(url, authHeader) -> Promise<Response>
                                // Calls js_fetch_get_with_auth(url, auth_header)
                                if args.len() >= 2 {
                                    let url = args.remove(0);
                                    let auth_header = args.remove(0);
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::FetchGetWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                    });
                                } else {
                                    return Err(anyhow!("fetchWithAuth requires url and authHeader arguments"));
                                }
                            }
                            "fetchPostWithAuth" => {
                                // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
                                // Calls js_fetch_post_with_auth(url, auth_header, body)
                                if args.len() >= 3 {
                                    let url = args.remove(0);
                                    let auth_header = args.remove(0);
                                    let body = args.remove(0);
                                    ctx.uses_fetch = true;
                                    return Ok(Expr::FetchPostWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                        body: Box::new(body),
                                    });
                                } else {
                                    return Err(anyhow!("fetchPostWithAuth requires url, authHeader, and body arguments"));
                                }
                            }
                            "fetch" => {
                                // Handle fetch(url) and fetch(url, options)
                                // Extract URL (first argument)
                                let url = if args.len() >= 1 {
                                    args.remove(0)
                                } else {
                                    return Err(anyhow!("fetch requires at least a URL argument"));
                                };

                                // Check if there's an options object (second argument)
                                if args.len() >= 1 {
                                    // Extract options from the object literal
                                    // We need to get the original AST to extract the object properties
                                    if let Some(options_arg) = call.args.get(1) {
                                        if let ast::Expr::Object(obj) = &*options_arg.expr {
                                            // Extract method, body, and headers from options
                                            let mut method = Expr::String("GET".to_string());
                                            let mut body = Expr::Undefined;
                                            let mut headers_obj: Vec<(String, Expr)> = Vec::new();

                                            for prop in &obj.props {
                                                if let ast::PropOrSpread::Prop(prop) = prop {
                                                    match prop.as_ref() {
                                                        ast::Prop::KeyValue(kv) => {
                                                            let key = match &kv.key {
                                                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                _ => continue,
                                                            };
                                                            match key.as_str() {
                                                                "method" => {
                                                                    method = lower_expr(ctx, &kv.value)?;
                                                                }
                                                                "body" => {
                                                                    body = lower_expr(ctx, &kv.value)?;
                                                                }
                                                                "headers" => {
                                                                    // Extract headers object
                                                                    if let ast::Expr::Object(headers_ast) = &*kv.value {
                                                                        for hprop in &headers_ast.props {
                                                                            if let ast::PropOrSpread::Prop(hprop) = hprop {
                                                                                if let ast::Prop::KeyValue(hkv) = hprop.as_ref() {
                                                                                    let hkey = match &hkv.key {
                                                                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                                        _ => continue,
                                                                                    };
                                                                                    let hval = lower_expr(ctx, &hkv.value)?;
                                                                                    headers_obj.push((hkey, hval));
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                        ast::Prop::Shorthand(ident) => {
                                                            // Handle shorthand properties like { body } which means { body: body }
                                                            let key = ident.sym.to_string();
                                                            let value = if let Some(local_id) = ctx.lookup_local(&key) {
                                                                Expr::LocalGet(local_id)
                                                            } else {
                                                                continue;
                                                            };
                                                            match key.as_str() {
                                                                "method" => method = value,
                                                                "body" => body = value,
                                                                _ => {}
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }

                                            // Create a FetchWithOptions expression
                                            ctx.uses_fetch = true;
                                            return Ok(Expr::FetchWithOptions {
                                                url: Box::new(url),
                                                method: Box::new(method),
                                                body: Box::new(body),
                                                headers: headers_obj,
                                            });
                                        }
                                    }
                                }

                                // Simple fetch(url) with no options - use GET
                                ctx.uses_fetch = true;
                                return Ok(Expr::FetchWithOptions {
                                    url: Box::new(url),
                                    method: Box::new(Expr::String("GET".to_string())),
                                    body: Box::new(Expr::Undefined),
                                    headers: Vec::new(),
                                });
                            }
                            _ => {} // Fall through to generic handling
                        }

                        // Check if this is a named import from child_process (e.g., execSync, spawnSync)
                        if let Some((module_name, _method)) = ctx.lookup_native_module(func_name) {
                            if module_name == "child_process" {
                                match func_name {
                                    "execSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExecSync {
                                                command: Box::new(command),
                                                options,
                                            });
                                        }
                                    }
                                    "spawnSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnSync {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "spawn" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawn {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "exec" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            let callback = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExec {
                                                command: Box::new(command),
                                                options,
                                                callback,
                                            });
                                        }
                                    }
                                    "spawnBackground" => {
                                        if args.len() >= 3 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let log_file = args_iter.next().unwrap();
                                            let env_json = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnBackground {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                log_file: Box::new(log_file),
                                                env_json,
                                            });
                                        }
                                    }
                                    "getProcessStatus" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessGetProcessStatus(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    "killProcess" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessKillProcess(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from path (e.g., join, dirname, basename)
                            if module_name == "path" {
                                match func_name {
                                    "join" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let mut result = iter.next().unwrap();
                                            for next_arg in iter {
                                                result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                            }
                                            return Ok(result);
                                        }
                                    }
                                    "dirname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "basename" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path_arg = iter.next().unwrap();
                                            let ext_arg = iter.next().unwrap();
                                            return Ok(Expr::PathBasenameExt(Box::new(path_arg), Box::new(ext_arg)));
                                        }
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "extname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "resolve" => {
                                        if args.len() >= 1 {
                                            let mut iter = args.into_iter();
                                            let first = iter.next().unwrap();
                                            let mut joined = first;
                                            for next_arg in iter {
                                                joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                            }
                                            return Ok(Expr::PathResolve(Box::new(joined)));
                                        }
                                    }
                                    "isAbsolute" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "relative" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let from = iter.next().unwrap();
                                            let to = iter.next().unwrap();
                                            return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
                                        }
                                    }
                                    "normalize" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathNormalize(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "parse" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathParse(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "format" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathFormat(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from url (e.g., fileURLToPath)
                            if module_name == "url" {
                                match func_name {
                                    "fileURLToPath" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FileURLToPath(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from fs (e.g., existsSync, mkdirSync, etc.)
                            if module_name == "fs" {
                                match func_name {
                                    "readFileSync" => {
                                        if args.len() >= 2 {
                                            // readFileSync(path, encoding) — returns string
                                            return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                        } else if args.len() == 1 {
                                            // readFileSync(path) without encoding — returns Buffer (Node parity)
                                            return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "writeFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "existsSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "mkdirSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "unlinkSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "appendFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsAppendFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "readFileBuffer" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "rmRecursive" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }
                        }

                        // Check if this is a direct call on an aliased named import
                        // e.g., uuid() where import { v4 as uuid } from 'uuid'
                        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            });
                        }

                        // Check if this is a call on a default import from a native module
                        // e.g., Fastify() where import Fastify from 'fastify'
                        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: "default".to_string(), // Use "default" for default export calls
                                args,
                            });
                        }
                    }

                    let callee_expr = lower_expr(ctx, expr)?;

                    // Fill in default arguments if callee is a known function
                    let mut args = args;
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some((defaults, param_ids)) = ctx.lookup_func_defaults(*func_id) {
                            let defaults = defaults.to_vec();
                            let param_ids = param_ids.to_vec();
                            let num_provided = args.len();
                            // Build substitution map: callee param LocalId -> actual arg expression
                            // For provided args, map to the caller's arg expression
                            // For defaulted args, map to the expanded default (built incrementally)
                            let mut param_map: Vec<(LocalId, Expr)> = Vec::new();
                            for i in 0..param_ids.len().min(num_provided) {
                                param_map.push((param_ids[i], args[i].clone()));
                            }
                            // Fill in missing arguments with their defaults, substituting
                            // any parameter references to use the caller's scope
                            for i in num_provided..defaults.len() {
                                if let Some(default_expr) = &defaults[i] {
                                    let substituted = LoweringContext::substitute_param_refs_in_default(
                                        default_expr, &param_map
                                    );
                                    // Add this expanded default to the map so later defaults
                                    // can reference it (e.g., c = b where b was also defaulted)
                                    if i < param_ids.len() {
                                        param_map.push((param_ids[i], substituted.clone()));
                                    }
                                    args.push(substituted);
                                }
                            }
                        }
                    }

                    // If inside a namespace, convert calls to namespace functions into StaticMethodCall
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some(ref ns_name) = ctx.current_namespace {
                            if let Some(func_name) = ctx.lookup_func_name(*func_id) {
                                if ctx.has_static_method(ns_name, func_name) {
                                    let method_name = func_name.to_string();
                                    let class_name = ns_name.clone();
                                    return Ok(Expr::StaticMethodCall {
                                        class_name,
                                        method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    let callee = Box::new(callee_expr);
                    // Extract explicit type arguments if present (e.g., identity<number>(x))
                    let type_args = call.type_args.as_ref()
                        .map(|ta| ta.params.iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect())
                        .unwrap_or_default();

                    // Use CallSpread if any argument has spread
                    if let Some(spread_args) = spread_args {
                        Ok(Expr::CallSpread { callee, args: spread_args, type_args })
                    } else {
                        Ok(Expr::Call { callee, args, type_args })
                    }
                }
                ast::Callee::Import(_) => {
                    // Dynamic import: import('module')
                    // Extract the module path from the first argument if available
                    let module_path = if let Some(first_arg) = args.first() {
                        if let Expr::String(s) = first_arg {
                            s.clone()
                        } else {
                            "<dynamic>".to_string()
                        }
                    } else {
                        "<unknown>".to_string()
                    };
                    eprintln!("Warning: Dynamic import('{}') not fully supported, returning undefined", module_path);
                    // Dynamic imports return a Promise that resolves to the module
                    // For now, return undefined as we'd need full runtime support
                    Ok(Expr::Undefined)
                }
            }
        }
        ast::Expr::Member(member) => {
            // Check if this is process.* property access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "process" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        match prop_ident.sym.as_ref() {
                            "argv" => return Ok(Expr::ProcessArgv),
                            "platform" => return Ok(Expr::OsPlatform),
                            "arch" => return Ok(Expr::OsArch),
                            "pid" => return Ok(Expr::ProcessPid),
                            "ppid" => return Ok(Expr::ProcessPpid),
                            "version" => return Ok(Expr::ProcessVersion),
                            "versions" => return Ok(Expr::ProcessVersions),
                            "stdin" => return Ok(Expr::ProcessStdin),
                            "stdout" => return Ok(Expr::ProcessStdout),
                            "stderr" => return Ok(Expr::ProcessStderr),
                            "env" => return Ok(Expr::ProcessEnv),
                            _ => {}
                        }
                    }
                }
                // `globalThis.process` returns an object whose `.env`/`.argv`/
                // etc. should resolve just like bare `process.*`. Without this
                // shim, `globalThis.process.env` walks through generic
                // PropertyGet dispatch and hits a 0.0 sentinel. Matches the
                // static `process.env` fast path above.
                if obj_ident.sym.as_ref() == "globalThis" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if prop_ident.sym.as_ref() == "process" {
                            // `globalThis.process` on its own — fall through
                            // to generic handling below (returns 0.0 sentinel,
                            // which is fine as the outer chain handles env/etc.).
                        }
                    }
                }
            }
            // Handle `globalThis.process.X` (and any PropertyGet whose object
            // resolves to `globalThis.process`): treat the outer `.X` as if
            // it were a bare `process.X` access. Unwraps transparent TS
            // wrappers (TsAs, TsNonNull, TsSatisfies, TsTypeAssertion, Paren)
            // so that `(globalThis as any).process.env` works too.
            fn unwrap_transparent(e: &ast::Expr) -> &ast::Expr {
                let mut cur = e;
                loop {
                    match cur {
                        ast::Expr::TsAs(x) => cur = &x.expr,
                        ast::Expr::TsNonNull(x) => cur = &x.expr,
                        ast::Expr::TsSatisfies(x) => cur = &x.expr,
                        ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
                        ast::Expr::TsConstAssertion(x) => cur = &x.expr,
                        ast::Expr::Paren(x) => cur = &x.expr,
                        _ => return cur,
                    }
                }
            }
            let member_obj_unwrapped = unwrap_transparent(member.obj.as_ref());
            if let ast::Expr::Member(inner) = member_obj_unwrapped {
                let inner_obj_unwrapped = unwrap_transparent(inner.obj.as_ref());
                let inner_is_global_process = matches!(
                    inner_obj_unwrapped,
                    ast::Expr::Ident(i) if i.sym.as_ref() == "globalThis"
                ) && matches!(
                    &inner.prop,
                    ast::MemberProp::Ident(p) if p.sym.as_ref() == "process"
                );
                if inner_is_global_process {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        match prop_ident.sym.as_ref() {
                            "argv" => return Ok(Expr::ProcessArgv),
                            "platform" => return Ok(Expr::OsPlatform),
                            "arch" => return Ok(Expr::OsArch),
                            "pid" => return Ok(Expr::ProcessPid),
                            "ppid" => return Ok(Expr::ProcessPpid),
                            "version" => return Ok(Expr::ProcessVersion),
                            "versions" => return Ok(Expr::ProcessVersions),
                            "env" => return Ok(Expr::ProcessEnv),
                            _ => {}
                        }
                    }
                }
            }

            // Check if this is Symbol.<well-known> — Symbol.toPrimitive,
            // Symbol.hasInstance, Symbol.toStringTag, Symbol.iterator,
            // Symbol.asyncIterator. Lowered to `SymbolFor(String("@@__perry_wk_<name>"))`
            // which the runtime's `js_symbol_for` sniffs via prefix and
            // resolves from the well-known cache (not the registry). This
            // gives each well-known symbol a stable pointer without needing
            // a new HIR variant.
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "Symbol" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let prop_name = prop_ident.sym.as_ref();
                        if matches!(
                            prop_name,
                            "toPrimitive"
                                | "hasInstance"
                                | "toStringTag"
                                | "iterator"
                                | "asyncIterator"
                        ) {
                            return Ok(Expr::SymbolFor(Box::new(Expr::String(
                                format!("@@__perry_wk_{}", prop_name),
                            ))));
                        }
                    }
                }
            }

            // Check if this is path.sep / path.delimiter constant access
            // (where `path` is an imported alias of the node:path module).
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                let is_path_module = obj_name == "path"
                    || ctx.lookup_builtin_module_alias(&obj_name) == Some("path");
                if is_path_module {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        match prop_ident.sym.as_ref() {
                            "sep" => return Ok(Expr::PathSep),
                            "delimiter" => return Ok(Expr::PathDelimiter),
                            _ => {}
                        }
                    }
                }
            }

            // Check if this is a process.env.VARNAME or process.env[expr] access
            if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                if let ast::Expr::Ident(obj_ident) = inner_member.obj.as_ref() {
                    if obj_ident.sym.as_ref() == "process" {
                        if let ast::MemberProp::Ident(prop_ident) = &inner_member.prop {
                            if prop_ident.sym.as_ref() == "env" {
                                // This is process.env access
                                match &member.prop {
                                    ast::MemberProp::Ident(var_ident) => {
                                        // process.env.VARNAME (static key)
                                        let var_name = var_ident.sym.to_string();
                                        return Ok(Expr::EnvGet(var_name));
                                    }
                                    ast::MemberProp::Computed(computed) => {
                                        // process.env[expr] (dynamic key)
                                        let key_expr = Box::new(lower_expr(ctx, &computed.expr)?);
                                        return Ok(Expr::EnvGetDynamic(key_expr));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Check for Math constants (e.g., Math.PI, Math.E)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "Math" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let val = match prop_ident.sym.as_ref() {
                            "PI" => Some(std::f64::consts::PI),
                            "E" => Some(std::f64::consts::E),
                            "LN2" => Some(std::f64::consts::LN_2),
                            "LN10" => Some(std::f64::consts::LN_10),
                            "LOG2E" => Some(std::f64::consts::LOG2_E),
                            "LOG10E" => Some(std::f64::consts::LOG10_E),
                            "SQRT2" => Some(std::f64::consts::SQRT_2),
                            "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
                            _ => None,
                        };
                        if let Some(v) = val {
                            return Ok(Expr::Number(v));
                        }
                    }
                }
            }

            // Check for Number constants (e.g., Number.MAX_SAFE_INTEGER)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == "Number" {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let val = match prop_ident.sym.as_ref() {
                            "MAX_SAFE_INTEGER" => Some(9007199254740991.0),
                            "MIN_SAFE_INTEGER" => Some(-9007199254740991.0),
                            "MAX_VALUE" => Some(f64::MAX),
                            "MIN_VALUE" => Some(f64::MIN_POSITIVE),
                            "EPSILON" => Some(f64::EPSILON),
                            "POSITIVE_INFINITY" => Some(f64::INFINITY),
                            "NEGATIVE_INFINITY" => Some(f64::NEG_INFINITY),
                            "NaN" => Some(f64::NAN),
                            _ => None,
                        };
                        if let Some(v) = val {
                            return Ok(Expr::Number(v));
                        }
                    }
                }
            }

            // Check if this is an enum member access (e.g., Color.Red)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_enum(&obj_name).is_some() {
                    // This is an enum access
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let member_name = prop_ident.sym.to_string();
                        return Ok(Expr::EnumMember {
                            enum_name: obj_name,
                            member_name,
                        });
                    }
                }
            }

            // Check if this is a static field access (e.g., Counter.count)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldGet {
                                class_name: obj_name,
                                field_name,
                            });
                        }
                    }
                }
            }

            // Check if this is a namespace variable access (e.g., Flag.OPENCODE_AUTO_SHARE)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let member_name = prop_ident.sym.to_string();
                    if let Some(local_id) = ctx.lookup_namespace_var(&obj_name, &member_name) {
                        return Ok(Expr::LocalGet(local_id));
                    }
                }
            }

            // Check if this is os.EOL property access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.as_ref();
                let is_os_module = obj_name == "os" ||
                    ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                if is_os_module {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if prop_ident.sym.as_ref() == "EOL" {
                            return Ok(Expr::OsEOL);
                        }
                    }
                }
            }

            // --- Proxy property get: `p.foo` / `p[k]` for known proxy locals ---
            {
                fn unwrap_member_obj<'a>(mut e: &'a ast::Expr) -> &'a ast::Expr {
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
                }
                let inner = unwrap_member_obj(member.obj.as_ref());
                if let ast::Expr::Ident(obj_ident) = inner {
                    let obj_name = obj_ident.sym.to_string();
                    if ctx.proxy_locals.contains(&obj_name) {
                        let proxy_expr = if let Some(id) = ctx.lookup_local(&obj_name) {
                            Expr::LocalGet(id)
                        } else {
                            lower_expr(ctx, &member.obj)?
                        };
                        let key_expr = match &member.prop {
                            ast::MemberProp::Ident(i) => Expr::String(i.sym.to_string()),
                            ast::MemberProp::Computed(c) => lower_expr(ctx, &c.expr)?,
                            ast::MemberProp::PrivateName(pn) => Expr::String(format!("#{}", pn.name.as_str())),
                        };
                        return Ok(Expr::ProxyGet { proxy: Box::new(proxy_expr), key: Box::new(key_expr) });
                    }
                }
            }

            // Check for native instance property access (e.g., response.status, response.ok)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                // Clone module_name early to avoid borrow issues
                let native_instance = ctx.lookup_native_instance(&obj_name)
                    .map(|(m, _c)| m.to_string());
                if let Some(module_name) = native_instance {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let property_name = prop_ident.sym.to_string();
                        // For properties that map to FFI functions, generate a NativeMethodCall
                        // with no args (property getter)
                        let object_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::NativeMethodCall {
                            module: module_name,
                            class_name: None,
                            object: Some(Box::new(object_expr)),
                            method: property_name,
                            args: Vec::new(),
                        });
                    }
                }
            }

            // TextEncoder / TextDecoder property access
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let prop_name = prop_ident.sym.as_ref();
                    let is_text_encoder = ctx.lookup_local_type(&obj_name)
                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                        .unwrap_or(false);
                    let is_text_decoder = ctx.lookup_local_type(&obj_name)
                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                        .unwrap_or(false);
                    if (is_text_encoder || is_text_decoder) && prop_name == "encoding" {
                        return Ok(Expr::String("utf-8".to_string()));
                    }
                }
            }

            // RegExp property access: regex.source / .flags / .lastIndex
            // Detect when receiver is a regex literal or local typed as RegExp.
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                if prop_name == "source" || prop_name == "flags" || prop_name == "lastIndex" {
                    let is_regex_obj = match member.obj.as_ref() {
                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                        ast::Expr::Ident(ident) => {
                            ctx.lookup_local_type(&ident.sym.to_string())
                                .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                                .unwrap_or(false)
                        }
                        _ => false,
                    };
                    if is_regex_obj {
                        let regex_expr = lower_expr(ctx, &member.obj)?;
                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                            return Ok(match prop_name {
                                "source" => Expr::RegExpSource(Box::new(regex_expr)),
                                "flags" => Expr::RegExpFlags(Box::new(regex_expr)),
                                "lastIndex" => Expr::RegExpLastIndex(Box::new(regex_expr)),
                                _ => unreachable!(),
                            });
                        }
                    }
                }
                // RegExpExecArray.index / .groups — receiver is a local that holds the result
                // of regex.exec(...). The runtime stores the most recent exec metadata in
                // thread-locals which RegExpExecIndex/Groups read.
                if prop_name == "index" || prop_name == "groups" {
                    // Strip non-null assertion (m1! → m1)
                    let inner = match member.obj.as_ref() {
                        ast::Expr::TsNonNull(nn) => nn.expr.as_ref(),
                        other => other,
                    };
                    if let ast::Expr::Ident(ident) = inner {
                        if ctx.regex_exec_locals.contains(&ident.sym.to_string()) {
                            return Ok(if prop_name == "index" {
                                Expr::RegExpExecIndex
                            } else {
                                Expr::RegExpExecGroups
                            });
                        }
                    }
                }
            }

            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    Ok(Expr::PropertyGet { object, property })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    // Specialize for Uint8Array/Buffer variables → byte-level access.
                    // Params declared `Buffer` (e.g. `function f(src: Buffer)`)
                    // reach here with `Type::Named("Buffer")` — treat it as a
                    // synonym for Uint8Array so `src[i]` uses the byte-read
                    // path instead of the generic f64-element IndexGet, which
                    // would return NaN-boxed pointer bits as a denormal f64.
                    if let Expr::LocalGet(id) = &*object {
                        if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                            if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
                                return Ok(Expr::Uint8ArrayGet { array: object, index });
                            }
                        }
                    }
                    Ok(Expr::IndexGet { object, index })
                }
                ast::MemberProp::PrivateName(private) => {
                    // Private field access: this.#field -> PropertyGet with "#field"
                    let property = format!("#{}", private.name.to_string());
                    Ok(Expr::PropertyGet { object, property })
                }
            }
        }
        ast::Expr::Paren(paren) => {
            lower_expr(ctx, &paren.expr)
        }
        ast::Expr::Assign(assign) => {
            // Detect assignments from native module calls and register for cross-function tracking.
            // e.g., `mongoClient = await MongoClient.connect(uri)` registers mongoClient as a mongodb instance.
            if assign.op == ast::AssignOp::Assign {
                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(target_ident)) = &assign.left {
                    let var_name = target_ident.id.sym.to_string();
                    // Unwrap await if present
                    let inner_rhs = if let ast::Expr::Await(await_expr) = assign.right.as_ref() {
                        await_expr.arg.as_ref()
                    } else {
                        assign.right.as_ref()
                    };
                    // Check for NativeModule.method() call (e.g., MongoClient.connect(uri))
                    if let ast::Expr::Call(call_expr) = inner_rhs {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.as_ref();
                                    if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                            let class_name = match (module_name, method_ident.sym.as_ref()) {
                                                ("mongodb", "connect") => Some("MongoClient"),
                                                ("pg", "connect") => Some("Client"),
                                                _ => Some("Instance"),
                                            };
                                            if let Some(class_name) = class_name {
                                                ctx.module_native_instances.push((var_name.clone(), module_name.to_string(), class_name.to_string()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Check for `new NativeClass(...)` assignment: `instance = new Database('mango.db')`
                    if let ast::Expr::New(new_expr) = inner_rhs {
                        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                            let class_name_str = class_ident.sym.as_ref();
                            let native_info = ctx.lookup_native_module(class_name_str)
                                .map(|(m, _)| m.to_string());
                            if let Some(module_name) = native_info {
                                ctx.register_native_instance(var_name.clone(), module_name.clone(), class_name_str.to_string());
                                ctx.module_native_instances.push((var_name.clone(), module_name, class_name_str.to_string()));
                            }
                        }
                    }
                    // Check for variable-to-variable assignment: `x = y` where y is a known native instance.
                    // e.g., `mongoClient = client` where client was tracked from MongoClient.connect().
                    if let ast::Expr::Ident(rhs_ident) = inner_rhs {
                        let rhs_name = rhs_ident.sym.as_ref();
                        if let Some((module, class)) = ctx.lookup_native_instance(rhs_name) {
                            ctx.module_native_instances.push((var_name, module.to_string(), class.to_string()));
                        }
                    }
                }
            }

            let rhs = lower_expr(ctx, &assign.right)?;

            // Handle compound assignment operators (+=, -=, *=, /=, etc.)
            let value = match assign.op {
                ast::AssignOp::Assign => Box::new(rhs),
                ast::AssignOp::AddAssign => {
                    // a += b becomes a = a + b
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::SubAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Sub,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::MulAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Mul,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::DivAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Div,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ModAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Mod,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitAndAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitAnd,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitOrAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitOr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::BitXorAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::LShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Shl,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::RShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Shr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ZeroFillRShiftAssign => {
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::UShr,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::ExpAssign => {
                    // a **= b becomes a = a ** b
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Binary {
                        op: BinaryOp::Pow,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::AndAssign => {
                    // a &&= b becomes a = a && b (short-circuit: only evaluates b if a is truthy)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::And,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::OrAssign => {
                    // a ||= b becomes a = a || b (short-circuit: only evaluates b if a is falsy)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::Or,
                        left,
                        right: Box::new(rhs),
                    })
                }
                ast::AssignOp::NullishAssign => {
                    // a ??= b becomes a = a ?? b (short-circuit: only evaluates b if a is null/undefined)
                    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
                    Box::new(Expr::Logical {
                        op: LogicalOp::Coalesce,
                        left,
                        right: Box::new(rhs),
                    })
                }
                _ => return Err(anyhow!("Unsupported assignment operator: {:?}", assign.op)),
            };

            match &assign.left {
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
                    let name = ident.id.sym.to_string();
                    if let Some(id) = ctx.lookup_local(&name) {
                        Ok(Expr::LocalSet(id, value))
                    } else {
                        // Variable not found in scope — likely a closure capture that wasn't
                        // properly tracked. Create an implicit local to avoid hard failure.
                        eprintln!("  Warning: Assignment to undeclared variable '{}', creating implicit local", name);
                        let id = ctx.define_local(name, Type::Any);
                        Ok(Expr::LocalSet(id, value))
                    }
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
                    // Proxy set: `proxy.foo = v` / `proxy[k] = v`
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        let obj_name = obj_ident.sym.to_string();
                        if ctx.proxy_locals.contains(&obj_name) {
                            let proxy = Box::new(if let Some(id) = ctx.lookup_local(&obj_name) {
                                Expr::LocalGet(id)
                            } else {
                                lower_expr(ctx, &member.obj)?
                            });
                            let key = Box::new(match &member.prop {
                                ast::MemberProp::Ident(i) => Expr::String(i.sym.to_string()),
                                ast::MemberProp::Computed(c) => lower_expr(ctx, &c.expr)?,
                                ast::MemberProp::PrivateName(p) => Expr::String(format!("#{}", p.name.as_str())),
                            });
                            return Ok(Expr::ProxySet { proxy, key, value });
                        }
                    }
                    // Check if this is a static field assignment (e.g., Counter.count = 5)
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

                    // regex.lastIndex = N → RegExpSetLastIndex
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if prop_ident.sym.as_ref() == "lastIndex" {
                            let is_regex_obj = match member.obj.as_ref() {
                                ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                                ast::Expr::Ident(ident) => ctx
                                    .lookup_local_type(&ident.sym.to_string())
                                    .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            if is_regex_obj {
                                let regex_expr = lower_expr(ctx, &member.obj)?;
                                if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                    return Ok(Expr::RegExpSetLastIndex {
                                        regex: Box::new(regex_expr),
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
                            Ok(Expr::PropertySet { object, property, value })
                        }
                        ast::MemberProp::Computed(computed) => {
                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                            // Specialize for Uint8Array/Buffer variables → byte-level access.
                            // See mirrored comment in IndexGet lowering: params
                            // typed `Buffer` must route through the byte-write path.
                            if let Expr::LocalGet(id) = &*object {
                                if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                                    if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
                                        return Ok(Expr::Uint8ArraySet { array: object, index, value });
                                    }
                                }
                            }
                            Ok(Expr::IndexSet { object, index, value })
                        }
                        ast::MemberProp::PrivateName(private) => {
                            // Private field assignment: this.#field = value
                            let property = format!("#{}", private.name.to_string());
                            Ok(Expr::PropertySet { object, property, value })
                        }
                    }
                }
                ast::AssignTarget::Pat(pat) => {
                    // Destructuring assignment: [a, b] = expr or { a, b } = expr
                    // We need to lower this to a sequence of assignments
                    lower_destructuring_assignment(ctx, pat, value)
                }
                // Unwrap TypeScript type annotations and parentheses for assignment
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
                    lower_expr_assignment(ctx, &paren.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
                    lower_expr_assignment(ctx, &ts_as.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
                    lower_expr_assignment(ctx, &ts_nn.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
                    lower_expr_assignment(ctx, &ts_ta.expr, value)
                }
                ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
                    lower_expr_assignment(ctx, &ts_sat.expr, value)
                }
                other => Err(anyhow!("Unsupported assignment target: {:?}", other)),
            }
        }
        ast::Expr::Cond(cond) => {
            let condition = Box::new(lower_expr(ctx, &cond.test)?);
            let then_expr = Box::new(lower_expr(ctx, &cond.cons)?);
            let else_expr = Box::new(lower_expr(ctx, &cond.alt)?);
            Ok(Expr::Conditional { condition, then_expr, else_expr })
        }
        ast::Expr::Array(array) => {
            // Check if any elements are spread elements
            let has_spread = array.elems.iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());

            if has_spread {
                // Use ArraySpread for arrays with spread elements.
                // If a spread source is a generator call, wrap it in IteratorToArray
                // so the codegen gets a real array to iterate.
                let elements = array.elems.iter()
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
                let elements = array.elems.iter()
                    .filter_map(|elem| elem.as_ref())
                    .map(|elem| lower_expr(ctx, &elem.expr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => {
            // Phase 3: closed-shape object literals lower to `new __AnonShape_N()`
            // so downstream field access hits the direct-GEP fast path. The
            // anon class is synthesized with `init: Some(value_expr)` on each
            // field, and `apply_field_initializers_recursive` at codegen time
            // emits `PropertySet { this, field, init }` — PropertySet's
            // direct-GEP arm at `crates/perry-codegen/src/expr.rs:2277-2293`
            // fires because `this` resolves to the anon class via class_stack.
            //
            // Runtime parity for Object.* introspection APIs on anon-shape
            // classes is handled runtime-side in perry-runtime's object module
            // — see that crate's handling of `class_id`-tagged objects on
            // getOwnPropertyDescriptor / Object.keys / JSON.stringify / etc.
            fn is_closed_shape(obj: &ast::ObjectLit) -> bool {
                if obj.props.is_empty() { return false; }
                for p in &obj.props {
                    match p {
                        ast::PropOrSpread::Spread(_) => return false,
                        ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                            ast::Prop::KeyValue(kv) => match &kv.key {
                                ast::PropName::Ident(_)
                                | ast::PropName::Str(_)
                                | ast::PropName::Num(_) => {}
                                _ => return false,
                            },
                            ast::Prop::Shorthand(_) => {}
                            _ => return false,
                        },
                    }
                }
                true
            }
            if is_closed_shape(obj) {
                let mut fields: Vec<(String, Type, Expr)> = Vec::new();
                let mut bail = false;
                let mut seen = std::collections::HashSet::new();
                for prop in &obj.props {
                    let ast::PropOrSpread::Prop(p) = prop else { unreachable!() };
                    match p.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            let key = match &kv.key {
                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                ast::PropName::Num(n) => n.value.to_string(),
                                _ => unreachable!(),
                            };
                            if !seen.insert(key.clone()) { bail = true; break; }
                            let ty = crate::lower_types::infer_type_from_expr(&kv.value, ctx);
                            let value = lower_expr(ctx, &kv.value)?;
                            fields.push((key, ty, value));
                        }
                        ast::Prop::Shorthand(ident) => {
                            let name = ident.sym.to_string();
                            if !seen.insert(name.clone()) { bail = true; break; }
                            let (value, ty) = if let Some(func_id) = ctx.lookup_func(&name) {
                                (Expr::FuncRef(func_id), Type::Any)
                            } else if let Some(local_id) = ctx.lookup_local(&name) {
                                let ty = ctx.lookup_local_type(&name).cloned().unwrap_or(Type::Any);
                                (Expr::LocalGet(local_id), ty)
                            } else if ctx.lookup_class(&name).is_some() {
                                (Expr::ClassRef(name.clone()), Type::Any)
                            } else {
                                bail = true; break;
                            };
                            fields.push((name, ty, value));
                        }
                        _ => unreachable!(),
                    }
                }
                if !bail {
                    // Split (name, ty, value) into parallel vecs before the
                    // synthesize call consumes ownership of the shape.
                    let args: Vec<Expr> = fields.iter().map(|(_, _, v)| v.clone()).collect();
                    let class_name = ctx.synthesize_anon_shape_class(&fields);
                    return Ok(Expr::New {
                        class_name,
                        args,
                        type_args: Vec::new(),
                    });
                }
            }
            // Legacy path — spread, methods/getters/setters, computed keys,
            // dup keys, or unresolvable shorthand.
            //
            // Check if any spread elements exist; if so, use ObjectSpread
            let has_spread = obj.props.iter().any(|p| matches!(p, ast::PropOrSpread::Spread(_)));
            if has_spread {
                let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                for prop in &obj.props {
                    match prop {
                        ast::PropOrSpread::Spread(spread) => {
                            let spread_expr = lower_expr(ctx, &spread.expr)?;
                            parts.push((None, spread_expr));
                        }
                        ast::PropOrSpread::Prop(prop) => {
                            match prop.as_ref() {
                                ast::Prop::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                        ast::PropName::Num(n) => n.value.to_string(),
                                        _ => continue,
                                    };
                                    let value = lower_expr(ctx, &kv.value)?;
                                    parts.push((Some(key), value));
                                }
                                ast::Prop::Shorthand(ident) => {
                                    let name = ident.sym.to_string();
                                    let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                        Expr::FuncRef(func_id)
                                    } else if let Some(local_id) = ctx.lookup_local(&name) {
                                        Expr::LocalGet(local_id)
                                    } else if ctx.lookup_class(&name).is_some() {
                                        Expr::ClassRef(name.clone())
                                    } else {
                                        continue;
                                    };
                                    parts.push((Some(name), value));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ok(Expr::ObjectSpread { parts });
            }
            let mut props = Vec::new();
            // Computed keys whose value can't be folded to a string at HIR time
            // (typically symbol-typed locals like `{ [symProp]: 42 }`). Deferred
            // and emitted as statements inside an IIFE wrapper after the
            // static-key Object literal is built.
            //
            // For `Prop::Method` with a computed key whose body uses `this`
            // (e.g. `{ [Symbol.toPrimitive](hint) { return this.value; } }`),
            // we emit a dedicated `js_object_set_symbol_method` runtime call
            // that BOTH stores the closure in the symbol side-table AND
            // patches the closure's reserved `this` slot with the object.
            enum PostInit {
                SetValue { key: Expr, value: Expr },
                SetMethodWithThis { key: Expr, closure: Expr },
            }
            let mut computed_post_init: Vec<PostInit> = Vec::new();
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Prop(prop) => {
                        match prop.as_ref() {
                            ast::Prop::KeyValue(kv) => {
                                enum KeyResolution {
                                    Static(String),
                                    Dynamic(Expr),
                                    Skip,
                                }
                                let key_resolution: KeyResolution = match &kv.key {
                                    ast::PropName::Ident(ident) => KeyResolution::Static(ident.sym.to_string()),
                                    ast::PropName::Str(s) => KeyResolution::Static(s.value.as_str().unwrap_or("").to_string()),
                                    ast::PropName::Num(n) => KeyResolution::Static(n.value.to_string()),
                                    ast::PropName::Computed(computed) => {
                                        // Handle computed property keys like [ChainName.ETHEREUM]
                                        // Try to resolve enum member access to string keys first.
                                        match computed.expr.as_ref() {
                                            ast::Expr::Member(member) => {
                                                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) = (member.obj.as_ref(), &member.prop) {
                                                    let enum_name = obj.sym.to_string();
                                                    let member_name = prop.sym.to_string();
                                                    if let Some(value) = ctx.lookup_enum_member(&enum_name, &member_name) {
                                                        match value {
                                                            EnumValue::String(s) => KeyResolution::Static(s.clone()),
                                                            EnumValue::Number(n) => KeyResolution::Static(n.to_string()),
                                                        }
                                                    } else {
                                                        // Non-enum member access: lower as a dynamic expression.
                                                        match lower_expr(ctx, computed.expr.as_ref()) {
                                                            Ok(e) => KeyResolution::Dynamic(e),
                                                            Err(_) => KeyResolution::Skip,
                                                        }
                                                    }
                                                } else {
                                                    match lower_expr(ctx, computed.expr.as_ref()) {
                                                        Ok(e) => KeyResolution::Dynamic(e),
                                                        Err(_) => KeyResolution::Skip,
                                                    }
                                                }
                                            }
                                            ast::Expr::Lit(ast::Lit::Str(s)) => KeyResolution::Static(s.value.as_str().unwrap_or("").to_string()),
                                            ast::Expr::Lit(ast::Lit::Num(n)) => KeyResolution::Static(n.value.to_string()),
                                            // Identifier or any other expression — lower it
                                            // and defer to post-init IndexSet so symbol-typed
                                            // locals like `[symProp]` flow through the
                                            // IndexSet symbol dispatch path.
                                            _ => match lower_expr(ctx, computed.expr.as_ref()) {
                                                Ok(e) => KeyResolution::Dynamic(e),
                                                Err(_) => KeyResolution::Skip,
                                            },
                                        }
                                    }
                                    _ => KeyResolution::Skip,
                                };
                                match key_resolution {
                                    KeyResolution::Skip => continue,
                                    KeyResolution::Static(key) => {
                                        let value = lower_expr(ctx, &kv.value)?;
                                        props.push((key, value));
                                    }
                                    KeyResolution::Dynamic(key_expr) => {
                                        let value = lower_expr(ctx, &kv.value)?;
                                        computed_post_init.push(PostInit::SetValue { key: key_expr, value });
                                    }
                                }
                            }
                            ast::Prop::Shorthand(ident) => {
                                // Shorthand property: { help } → { help: help }
                                let name = ident.sym.to_string();
                                let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                    Expr::FuncRef(func_id)
                                } else if let Some(local_id) = ctx.lookup_local(&name) {
                                    Expr::LocalGet(local_id)
                                } else if ctx.lookup_class(&name).is_some() {
                                    Expr::ClassRef(name.clone())
                                } else {
                                    continue;
                                };
                                props.push((name, value));
                            }
                            ast::Prop::Method(method) => {
                                // Inline method: { help(): string { ... } }
                                // Computed keys (e.g. `[Symbol.toPrimitive](hint) {}`)
                                // get routed through the IIFE wrapper's
                                // SetMethodWithThis post-init, which emits a
                                // `js_object_set_symbol_method` call that also
                                // patches the closure's reserved `this` slot.
                                enum MethodKey {
                                    Static(String),
                                    Computed(Expr),
                                }
                                let method_key = match &method.key {
                                    ast::PropName::Ident(ident) => {
                                        MethodKey::Static(ident.sym.to_string())
                                    }
                                    ast::PropName::Str(s) => {
                                        MethodKey::Static(s.value.as_str().unwrap_or("").to_string())
                                    }
                                    ast::PropName::Computed(computed) => {
                                        match lower_expr(ctx, computed.expr.as_ref()) {
                                            Ok(e) => MethodKey::Computed(e),
                                            Err(_) => continue,
                                        }
                                    }
                                    _ => continue,
                                };
                                let key_label: String = match &method_key {
                                    MethodKey::Static(s) => s.clone(),
                                    MethodKey::Computed(_) => format!("computed_{}", ctx.next_func_id),
                                };
                                let key: String = key_label.clone();
                                let func_id = ctx.fresh_func();
                                // Use a unique synthetic name to avoid collisions
                                let func_name = format!("__obj_method_{}_{}", key, func_id);

                                // Snapshot outer locals for capture analysis
                                let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                                    .map(|(name, id, _)| (name.clone(), *id))
                                    .collect();

                                let scope_mark = ctx.enter_scope();
                                let mut params = Vec::new();
                                for param in method.function.params.iter() {
                                    let param_name = get_pat_name(&param.pat)?;
                                    let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
                                    let param_default = get_param_default(ctx, &param.pat)?;
                                    let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                                    params.push(Param {
                                        id: param_id,
                                        name: param_name,
                                        ty: param_type,
                                        default: param_default,
                                        is_rest: is_rest_param(&param.pat),
                                    });
                                }
                                let return_type = method.function.return_type.as_ref()
                                    .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                let body = if let Some(ref block) = method.function.body {
                                    lower_block_stmt(ctx, block)?
                                } else {
                                    Vec::new()
                                };
                                ctx.exit_scope(scope_mark);

                                // Capture analysis (same pattern as arrow/function expressions)
                                let mut all_refs = Vec::new();
                                let mut visited_closures = std::collections::HashSet::new();
                                for stmt in &body {
                                    collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
                                }
                                let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                                    .map(|(_, id)| *id)
                                    .collect();
                                let method_param_ids: std::collections::HashSet<LocalId> = params.iter()
                                    .map(|p| p.id)
                                    .collect();
                                let mut captures: Vec<LocalId> = all_refs.into_iter()
                                    .filter(|id| outer_local_ids.contains(id) && !method_param_ids.contains(id))
                                    .collect();
                                captures.sort();
                                captures.dedup();
                                captures = ctx.filter_module_level_captures(captures);

                                // Check if the method body uses `this` — even with no
                                // outer-scope captures we must emit a Closure so the
                                // object-literal creation code can patch capture slot 0
                                // with the object pointer.
                                let uses_this = closure_uses_this(&body);

                                let value_expr: Expr = if captures.is_empty() && !uses_this {
                                    // No captures and no `this`: keep as standalone Function + FuncRef
                                    ctx.register_func(func_name.clone(), func_id);
                                    let defaults: Vec<Option<Expr>> = params.iter().map(|p| p.default.clone()).collect();
                                    let param_ids: Vec<LocalId> = params.iter().map(|p| p.id).collect();
                                    ctx.func_defaults.push((func_id, defaults, param_ids));
                                    ctx.pending_functions.push(Function {
                                        id: func_id,
                                        name: func_name,
                                        type_params: Vec::new(),
                                        params,
                                        return_type,
                                        body,
                                        is_async: method.function.is_async,
                                        is_generator: false,
                                        is_exported: false,
                                        captures: Vec::new(),
                                        decorators: Vec::new(),
                                    });
                                    Expr::FuncRef(func_id)
                                } else {
                                    // Has captures: emit as Closure
                                    let mut all_assigned = Vec::new();
                                    for stmt in &body {
                                        collect_assigned_locals_stmt(stmt, &mut all_assigned);
                                    }
                                    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
                                    let mutable_captures: Vec<LocalId> = captures.iter()
                                        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                                        .copied()
                                        .collect();
                                    let captures_this = uses_this;
                                    let enclosing_class = if captures_this {
                                        ctx.current_class.clone()
                                    } else {
                                        None
                                    };
                                    Expr::Closure {
                                        func_id,
                                        params,
                                        return_type,
                                        body,
                                        captures,
                                        mutable_captures,
                                        captures_this,
                                        enclosing_class,
                                        is_async: method.function.is_async,
                                    }
                                };
                                match method_key {
                                    MethodKey::Static(key_str) => {
                                        props.push((key_str, value_expr));
                                    }
                                    MethodKey::Computed(key_expr) => {
                                        if uses_this {
                                            computed_post_init.push(PostInit::SetMethodWithThis {
                                                key: key_expr,
                                                closure: value_expr,
                                            });
                                        } else {
                                            computed_post_init.push(PostInit::SetValue {
                                                key: key_expr,
                                                value: value_expr,
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            // No computed-key post-init: emit a plain object literal.
            if computed_post_init.is_empty() {
                return Ok(Expr::Object(props));
            }
            // Has computed keys: synthesize an IIFE wrapper that builds the
            // object with static props, then runs IndexSet for each computed
            // key, then returns the object. The IndexSet branch in the LLVM
            // backend already runtime-dispatches to
            // `js_object_set_symbol_property` when the key is a symbol — so
            // `{ [symProp]: 42, x: 1 }` flows through the symbol side-table
            // automatically.
            //
            // Lowered shape:
            //   ((__o) => {
            //       __o[k1] = v1;
            //       __o[k2] = v2;
            //       return __o;
            //   })({ static_props })
            let iife_func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();
            let param_id = ctx.define_local("__perry_obj_iife".to_string(), Type::Any);
            let param = Param {
                id: param_id,
                name: "__perry_obj_iife".to_string(),
                ty: Type::Any,
                default: None,
                is_rest: false,
            };
            let mut body: Vec<Stmt> = Vec::with_capacity(computed_post_init.len() + 1);
            for init in computed_post_init {
                match init {
                    PostInit::SetValue { key, value } => {
                        body.push(Stmt::Expr(Expr::IndexSet {
                            object: Box::new(Expr::LocalGet(param_id)),
                            index: Box::new(key),
                            value: Box::new(value),
                        }));
                    }
                    PostInit::SetMethodWithThis { key, closure } => {
                        // Emit a direct call to the runtime helper that
                        // stores the closure in the symbol side-table AND
                        // patches its reserved `this` slot with __o.
                        body.push(Stmt::Expr(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: "js_object_set_symbol_method".to_string(),
                                param_types: Vec::new(),
                                return_type: Type::Any,
                            }),
                            args: vec![
                                Expr::LocalGet(param_id),
                                key,
                                closure,
                            ],
                            type_args: Vec::new(),
                        }));
                    }
                }
            }
            body.push(Stmt::Return(Some(Expr::LocalGet(param_id))));
            ctx.exit_scope(scope_mark);
            // Capture analysis: any LocalIds referenced inside the body that
            // weren't defined here (i.e. the symbol locals from the outer scope).
            let mut all_refs = Vec::new();
            let mut visited_closures = std::collections::HashSet::new();
            for stmt in &body {
                collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
            }
            let mut captures: Vec<LocalId> = all_refs
                .into_iter()
                .filter(|id| *id != param_id)
                .collect();
            captures.sort();
            captures.dedup();
            captures = ctx.filter_module_level_captures(captures);
            let static_obj = Expr::Object(props);
            let closure = Expr::Closure {
                func_id: iife_func_id,
                params: vec![param],
                return_type: Type::Any,
                body,
                captures,
                mutable_captures: Vec::new(),
                captures_this: false,
                enclosing_class: None,
                is_async: false,
            };
            Ok(Expr::Call {
                callee: Box::new(closure),
                args: vec![static_obj],
                type_args: vec![],
            })
        }
        ast::Expr::This(_) => {
            // Always use Expr::This - the codegen will handle it with ThisContext
            Ok(Expr::This)
        }
        ast::Expr::New(new_expr) => {
            // Try to extract class name from callee
            match new_expr.callee.as_ref() {
                ast::Expr::Ident(ident) => {
                    let class_name = ident.sym.to_string();

                    // Handle built-in types
                    if class_name == "Map" {
                        // new Map() or new Map(entries)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::MapNew);
                        } else {
                            return Ok(Expr::MapNewFromArray(Box::new(args.into_iter().next().unwrap())));
                        }
                    }
                    if class_name == "Set" {
                        // new Set() or new Set(iterable)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::SetNew);
                        } else {
                            return Ok(Expr::SetNewFromArray(Box::new(args.into_iter().next().unwrap())));
                        }
                    }
                    if class_name == "Date" {
                        // new Date() or new Date(timestamp)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::DateNew(None));
                        } else {
                            return Ok(Expr::DateNew(Some(Box::new(args.into_iter().next().unwrap()))));
                        }
                    }
                    if class_name == "RegExp" {
                        // new RegExp(pattern[, flags]) — for string-literal args,
                        // route to the same `Expr::RegExp { pattern, flags }`
                        // variant the literal `/foo/g` syntax produces. The
                        // codegen interns both strings and calls
                        // `js_regexp_new(pattern_handle, flags_handle)`.
                        //
                        // Without this branch, the New expression falls through
                        // to generic class instantiation, which silently fails
                        // (no user class named RegExp), leaving an unusable
                        // ObjectHeader that makes regex.exec() return null and
                        // any subsequent indexing on that null crash.
                        let args_ast = new_expr.args.as_ref();
                        let pattern_lit = args_ast
                            .and_then(|args| args.first())
                            .and_then(|a| match a.expr.as_ref() {
                                ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.as_str().unwrap_or("").to_string()),
                                _ => None,
                            });
                        let flags_lit = args_ast
                            .and_then(|args| args.get(1))
                            .and_then(|a| match a.expr.as_ref() {
                                ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.as_str().unwrap_or("").to_string()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        if let Some(pattern) = pattern_lit {
                            return Ok(Expr::RegExp { pattern, flags: flags_lit });
                        }
                        // Fall through to generic class instantiation for
                        // non-literal args (e.g. `new RegExp(userInput)`).
                        // That path is currently broken too, but at least
                        // doesn't regress on the literal case which is far
                        // more common.
                    }
                    if class_name == "Proxy" {
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let mut it = args.into_iter();
                        let target = it.next().unwrap_or(Expr::Undefined);
                        let handler = it.next().unwrap_or(Expr::Object(vec![]));
                        return Ok(Expr::ProxyNew { target: Box::new(target), handler: Box::new(handler) });
                    }
                    if ctx.proxy_locals.contains(&class_name) {
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        // If the proxy's construction wrapped a known class,
                        // call the construct trap (for side effects) then
                        // instantiate the real class. This matches the
                        // test's expected behaviour.
                        if let Some(target_class) = ctx.proxy_target_classes.get(&class_name).cloned() {
                            if ctx.lookup_class(&target_class).is_some() {
                                if let Some(id) = ctx.lookup_local(&class_name) {
                                    let trap_call = Expr::ProxyConstruct {
                                        proxy: Box::new(Expr::LocalGet(id)),
                                        args: args.clone(),
                                    };
                                    return Ok(Expr::Sequence(vec![
                                        trap_call,
                                        Expr::New {
                                            class_name: target_class,
                                            args,
                                            type_args: vec![],
                                        },
                                    ]));
                                }
                            }
                        }
                        if let Some(id) = ctx.lookup_local(&class_name) {
                            return Ok(Expr::ProxyConstruct { proxy: Box::new(Expr::LocalGet(id)), args });
                        }
                    }
                    // Handle AggregateError separately (2-arg form: errors array, message)
                    if class_name == "AggregateError" {
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let mut iter = args.into_iter();
                        let errors = iter.next().unwrap_or(Expr::Array(vec![]));
                        let message = iter.next().unwrap_or(Expr::String("".to_string()));
                        return Ok(Expr::AggregateErrorNew {
                            errors: Box::new(errors),
                            message: Box::new(message),
                        });
                    }

                    // Handle Error and its subclasses
                    if class_name == "Error" || class_name == "TypeError" || class_name == "RangeError"
                        || class_name == "ReferenceError" || class_name == "SyntaxError"
                        || class_name == "BugIndicatingError" {
                        // new Error() / new Error(message) / new Error(message, { cause })
                        //
                        // 2-arg form detection runs at AST level (not HIR) because Phase 3
                        // synthesises anon classes for closed-shape object literals — the
                        // options `{ cause: e }` would become `Expr::New { __AnonShape_N }`
                        // after lower_expr, and the `Expr::Object(fields)` match below
                        // would miss it. Pull `cause` directly from the AST first, then
                        // fall through to the standard argument lowering for other shapes.
                        let ast_args = new_expr.args.as_deref().unwrap_or(&[]);
                        if ast_args.len() == 2 && class_name == "Error" {
                            let msg = lower_expr(ctx, &ast_args[0].expr)?;
                            // Peel `Expr::Paren(({ cause: e }))` — SWC preserves paren
                            // nodes, so without unwrapping the outer Object match below
                            // would miss `new Error(msg, ({ cause }))` and we'd silently
                            // drop the cause.
                            let mut opts_expr: &ast::Expr = &ast_args[1].expr;
                            while let ast::Expr::Paren(p) = opts_expr {
                                opts_expr = &p.expr;
                            }
                            // Look for `{ cause: <expr> }` or `{ cause }` at the AST level.
                            if let ast::Expr::Object(opts_obj) = opts_expr {
                                for prop in &opts_obj.props {
                                    if let ast::PropOrSpread::Prop(p) = prop {
                                        match p.as_ref() {
                                            ast::Prop::KeyValue(kv) => {
                                                let key = match &kv.key {
                                                    ast::PropName::Ident(i) => i.sym.to_string(),
                                                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                    _ => continue,
                                                };
                                                if key == "cause" {
                                                    let cause = lower_expr(ctx, &kv.value)?;
                                                    return Ok(Expr::ErrorNewWithCause {
                                                        message: Box::new(msg),
                                                        cause: Box::new(cause),
                                                    });
                                                }
                                            }
                                            // ES2022 shorthand `new Error(msg, { cause })`
                                            // — the canonical idiom inside a `catch (cause)`
                                            // block. Resolve the ident the same way the
                                            // HIR Object-literal lowering does: func /
                                            // local / class-ref precedence.
                                            ast::Prop::Shorthand(ident) => {
                                                let name = ident.sym.to_string();
                                                if name != "cause" { continue; }
                                                let cause = if let Some(func_id) = ctx.lookup_func(&name) {
                                                    Expr::FuncRef(func_id)
                                                } else if let Some(local_id) = ctx.lookup_local(&name) {
                                                    Expr::LocalGet(local_id)
                                                } else if ctx.lookup_class(&name).is_some() {
                                                    Expr::ClassRef(name.clone())
                                                } else {
                                                    // Unresolvable identifier — fall through
                                                    // to the no-cause path below.
                                                    continue;
                                                };
                                                return Ok(Expr::ErrorNewWithCause {
                                                    message: Box::new(msg),
                                                    cause: Box::new(cause),
                                                });
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            // No recognizable `cause` key — lower the opts for side effects,
                            // then emit a plain Error with just the message.
                            let _ = lower_expr(ctx, &ast_args[1].expr)?;
                            return Ok(Expr::ErrorNew(Some(Box::new(msg))));
                        }

                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();

                        if args.is_empty() {
                            return match class_name.as_str() {
                                "TypeError" => Ok(Expr::TypeErrorNew(Box::new(Expr::String("".to_string())))),
                                "RangeError" => Ok(Expr::RangeErrorNew(Box::new(Expr::String("".to_string())))),
                                "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(Expr::String("".to_string())))),
                                "SyntaxError" => Ok(Expr::SyntaxErrorNew(Box::new(Expr::String("".to_string())))),
                                _ => Ok(Expr::ErrorNew(None)),
                            };
                        } else {
                            let msg = args.into_iter().next().unwrap();
                            return match class_name.as_str() {
                                "TypeError" => Ok(Expr::TypeErrorNew(Box::new(msg))),
                                "RangeError" => Ok(Expr::RangeErrorNew(Box::new(msg))),
                                "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(msg))),
                                "SyntaxError" => Ok(Expr::SyntaxErrorNew(Box::new(msg))),
                                _ => Ok(Expr::ErrorNew(Some(Box::new(msg)))),
                            };
                        }
                    }

                    // Handle URL class
                    if class_name == "URL" {
                        // new URL(url) or new URL(url, base)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let mut args_iter = args.into_iter();
                        let url_arg = args_iter.next()
                            .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
                        let base_arg = args_iter.next();
                        return Ok(Expr::UrlNew {
                            url: Box::new(url_arg),
                            base: base_arg.map(Box::new),
                        });
                    }

                    // Handle URLSearchParams class
                    if class_name == "URLSearchParams" {
                        // new URLSearchParams() or new URLSearchParams(init)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let init_arg = args.into_iter().next();
                        return Ok(Expr::UrlSearchParamsNew(init_arg.map(Box::new)));
                    }

                    // Handle WeakRef class — wraps a value (object) in a weak reference object.
                    // Pragmatic implementation: stores a strong reference and `deref()` always returns it.
                    if class_name == "WeakRef" {
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let target = args.into_iter().next()
                            .ok_or_else(|| anyhow!("WeakRef constructor requires 1 argument"))?;
                        return Ok(Expr::WeakRefNew(Box::new(target)));
                    }

                    // Handle FinalizationRegistry class — registers cleanup callbacks invoked when
                    // tracked targets are GC'd. Pragmatic implementation: stores registrations but
                    // never fires the callback (Perry's GC doesn't track weak references yet).
                    if class_name == "FinalizationRegistry" {
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let cb = args.into_iter().next()
                            .ok_or_else(|| anyhow!("FinalizationRegistry constructor requires a callback argument"))?;
                        return Ok(Expr::FinalizationRegistryNew(Box::new(cb)));
                    }
                    // Handle TextEncoder constructor
                    if class_name == "TextEncoder" {
                        return Ok(Expr::TextEncoderNew);
                    }
                    // Handle TextDecoder constructor
                    if class_name == "TextDecoder" {
                        // new TextDecoder() or new TextDecoder("utf-8") — we only support UTF-8
                        return Ok(Expr::TextDecoderNew);
                    }

                    // Handle Uint8Array constructor
                    if class_name == "Uint8Array" {
                        // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        if args.is_empty() {
                            return Ok(Expr::Uint8ArrayNew(None));
                        } else if args.len() == 1 {
                            return Ok(Expr::Uint8ArrayNew(Some(Box::new(args.into_iter().next().unwrap()))));
                        }
                        // 2+ args: fall through to Expr::New to handle
                        // new Uint8Array(buffer, byteOffset, length) etc.
                    }

                    // Handle other typed-array constructors (Int8/16/32, Uint16/32, Float32/64,
                    // Uint8ClampedArray). Uint8Array stays on the Buffer path above.
                    if let Some(kind) = crate::ir::typed_array_kind_for_name(class_name.as_str()) {
                        if class_name != "Uint8Array" {
                            let args = new_expr.args.as_ref()
                                .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                                .transpose()?
                                .unwrap_or_default();
                            if args.is_empty() {
                                return Ok(Expr::TypedArrayNew { kind, arg: None });
                            } else if args.len() == 1 {
                                return Ok(Expr::TypedArrayNew {
                                    kind,
                                    arg: Some(Box::new(args.into_iter().next().unwrap())),
                                });
                            }
                            // Multi-arg form (buffer, byteOffset, length): fall through.
                        }
                    }

                    let args = new_expr.args.as_ref()
                        .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                        .transpose()?
                        .unwrap_or_default();
                    // Extract explicit type arguments if present (e.g., new Box<number>(42))
                    let type_args = new_expr.type_args.as_ref()
                        .map(|ta| ta.params.iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect())
                        .unwrap_or_default();
                    Ok(Expr::New { class_name, args, type_args })
                }
                // Non-identifier callee (e.g., new (condition ? A : B)() or new someVar())
                _ => {
                    // Check for class expressions: new (class extends X { ... })()
                    let class_expr_opt = match new_expr.callee.as_ref() {
                        ast::Expr::Class(ce) => Some(ce),
                        ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                            ast::Expr::Class(ce) => Some(ce),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(class_expr) = class_expr_opt {
                        let synthetic_name = format!("__anon_class_{}", ctx.fresh_class());
                        let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
                        ctx.pending_classes.push(class);
                        let args = new_expr.args.as_ref()
                            .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                            .transpose()?
                            .unwrap_or_default();
                        let type_args = new_expr.type_args.as_ref()
                            .map(|ta| ta.params.iter()
                                .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                                .collect())
                            .unwrap_or_default();
                        return Ok(Expr::New { class_name: synthetic_name, args, type_args });
                    }

                    let callee = Box::new(lower_expr(ctx, &new_expr.callee)?);
                    let args = new_expr.args.as_ref()
                        .map(|args| args.iter().map(|a| lower_expr(ctx, &a.expr)).collect::<Result<Vec<_>>>())
                        .transpose()?
                        .unwrap_or_default();
                    Ok(Expr::NewDynamic { callee, args })
                }
            }
        }
        ast::Expr::Arrow(arrow) => {
            // Lower arrow function to a closure
            let func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();

            // Track which locals exist before entering the closure scope
            let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                .map(|(name, id, _)| (name.clone(), *id))
                .collect();

            // Lower parameters and collect destructuring info
            let mut params = Vec::new();
            let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
            for param in &arrow.params {
                let param_name = get_pat_name(param)?;
                let param_default = get_param_default(ctx, param)?;
                let is_rest = is_rest_param(param);
                let param_ty = get_pat_type(param, ctx);
                let param_id = ctx.define_local(param_name.clone(), param_ty.clone());
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_ty,
                    default: param_default,
                    is_rest,
                });
                // Track destructuring patterns to generate extraction statements
                if is_destructuring_pattern(param) {
                    destructuring_params.push((param_id, param.clone()));
                }
            }

            // Register arrow function parameters with known native types as native instances
            for param in &params {
                if let Type::Named(type_name) = &param.ty {
                    let native_info = match type_name.as_str() {
                        "PluginApi" => Some(("perry/plugin", "PluginApi")),
                        "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                        "Redis" => Some(("ioredis", "Redis")),
                        "EventEmitter" => Some(("events", "EventEmitter")),
                        // Fastify types
                        "FastifyInstance" => Some(("fastify", "App")),
                        "FastifyRequest" => Some(("fastify", "Request")),
                        "FastifyReply" => Some(("fastify", "Reply")),
                        // HTTP/HTTPS types
                        "IncomingMessage" => Some(("http", "IncomingMessage")),
                        "ClientRequest" => Some(("http", "ClientRequest")),
                        "ServerResponse" => Some(("http", "ServerResponse")),
                        _ => None,
                    };
                    if let Some((module, class)) = native_info {
                        ctx.register_native_instance(param.name.clone(), module.to_string(), class.to_string());
                    }
                }
            }

            // Generate Let statements for destructuring patterns BEFORE lowering body
            // This ensures the destructured variable names are defined when the body references them
            let mut destructuring_stmts = Vec::new();
            for (param_id, pat) in &destructuring_params {
                let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                destructuring_stmts.extend(stmts);
            }

            // Hoist function declarations in block body (JS hoisting semantics)
            if let ast::BlockStmtOrExpr::BlockStmt(block) = &*arrow.body {
                for stmt in &block.stmts {
                    if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                        if fn_decl.function.body.is_some() {
                            let name = fn_decl.ident.sym.to_string();
                            if ctx.lookup_local(&name).is_none() {
                                ctx.define_local(name, Type::Any);
                            }
                        }
                    }
                }
            }

            // Lower body with JS function hoisting.
            // Only `var` declarations and function declarations are hoisted
            // to the top per JS semantics — `let`/`const` MUST remain at their
            // lexical position because they have block-scoped temporal dead
            // zone semantics and, critically, their init expressions are only
            // evaluated when control flow reaches them. Hoisting a `const x =
            // someCall()` above a conditional that should skip it would
            // eagerly invoke the call and break user code.
            let mut body = match &*arrow.body {
                ast::BlockStmtOrExpr::BlockStmt(block) => {
                    let mut var_hoisted = Vec::new();
                    let mut func_decls = Vec::new();
                    let mut exec_stmts = Vec::new();
                    for stmt in &block.stmts {
                        let lowered = crate::lower_decl::lower_body_stmt(ctx, stmt)?;
                        match stmt {
                            ast::Stmt::Decl(ast::Decl::Fn(_)) => func_decls.extend(lowered),
                            ast::Stmt::Decl(ast::Decl::Var(var_decl))
                                if var_decl.kind == ast::VarDeclKind::Var =>
                            {
                                var_hoisted.extend(lowered);
                            }
                            _ => exec_stmts.extend(lowered),
                        }
                    }
                    var_hoisted.extend(func_decls);
                    var_hoisted.extend(exec_stmts);
                    var_hoisted
                }
                ast::BlockStmtOrExpr::Expr(expr) => {
                    let return_expr = lower_expr(ctx, expr)?;
                    vec![Stmt::Return(Some(return_expr))]
                }
            };

            // Prepend destructuring statements to body
            if !destructuring_stmts.is_empty() {
                let mut new_body = destructuring_stmts;
                new_body.append(&mut body);
                body = new_body;
            }

            ctx.exit_scope(scope_mark);

            // Detect captured variables: locals referenced in the body that were defined in outer scope
            let mut all_refs = Vec::new();
                let mut visited_closures = std::collections::HashSet::new();
            for stmt in &body {
                collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
            }

            // Filter to only include outer locals (not parameters or locals defined within the closure)
            let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                .map(|(_, id)| *id)
                .collect();
            let param_ids: std::collections::HashSet<LocalId> = params.iter()
                .map(|p| p.id)
                .collect();

            // Find unique captures: refs that are in outer_locals but not params
            let mut captures: Vec<LocalId> = all_refs.into_iter()
                .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
                .collect();
            captures.sort();
            captures.dedup();
            captures = ctx.filter_module_level_captures(captures);

            // Detect which captures are assigned to inside the closure (need boxing)
            let mut all_assigned = Vec::new();
            for stmt in &body {
                collect_assigned_locals_stmt(stmt, &mut all_assigned);
            }
            let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
            let mutable_captures: Vec<LocalId> = captures.iter()
                .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                .copied()
                .collect();

            // Check if this arrow function uses `this` (needs to capture it from enclosing scope)
            let captures_this = closure_uses_this(&body);

            // Store enclosing class name for arrow functions that capture `this`
            let enclosing_class = if captures_this {
                ctx.current_class.clone()
            } else {
                None
            };

            Ok(Expr::Closure {
                func_id,
                params,
                return_type: Type::Any,
                body,
                captures,
                mutable_captures,
                captures_this,
                enclosing_class,
                is_async: arrow.is_async,
            })
        }
        ast::Expr::Fn(fn_expr) => {
            // Lower function expression to a closure (similar to arrow)
            let func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();

            // Track which locals exist before entering the closure scope
            let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                .map(|(name, id, _)| (name.clone(), *id))
                .collect();

            // Lower parameters and collect destructuring info
            let mut params = Vec::new();
            let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
            for param in &fn_expr.function.params {
                let param_name = get_pat_name(&param.pat)?;
                let param_default = get_param_default(ctx, &param.pat)?;
                let is_rest = is_rest_param(&param.pat);
                let param_id = ctx.define_local(param_name.clone(), Type::Any);
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: Type::Any,
                    default: param_default,
                    is_rest,
                });
                // Track destructuring patterns to generate extraction statements
                if is_destructuring_pattern(&param.pat) {
                    destructuring_params.push((param_id, param.pat.clone()));
                }
            }

            // Generate Let statements for destructuring patterns BEFORE lowering body
            let mut destructuring_stmts = Vec::new();
            for (param_id, pat) in &destructuring_params {
                let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                destructuring_stmts.extend(stmts);
            }

            // Hoist function declarations: pre-register all function declarations in the body
            // so they can be referenced before their lexical position (JS hoisting semantics).
            if let Some(ref block) = fn_expr.function.body {
                for stmt in &block.stmts {
                    if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
                        if fn_decl.function.body.is_some() {
                            let name = fn_decl.ident.sym.to_string();
                            if ctx.lookup_local(&name).is_none() {
                                ctx.define_local(name, Type::Any);
                            }
                        }
                    }
                }
            }

            // Lower body with JS hoisting: only `var` declarations and function
            // declarations are hoisted per JS semantics. `let`/`const` MUST remain
            // at their lexical position because their init expressions are only
            // evaluated when control flow reaches them — hoisting `const x = fn()`
            // out of a conditional branch would eagerly run the call.
            let mut body = if let Some(ref block) = fn_expr.function.body {
                let mut var_hoisted = Vec::new();
                let mut func_decls = Vec::new();
                let mut exec_stmts = Vec::new();
                for stmt in &block.stmts {
                    let lowered = crate::lower_decl::lower_body_stmt(ctx, stmt)?;
                    match stmt {
                        ast::Stmt::Decl(ast::Decl::Fn(_)) => func_decls.extend(lowered),
                        ast::Stmt::Decl(ast::Decl::Var(var_decl))
                            if var_decl.kind == ast::VarDeclKind::Var =>
                        {
                            var_hoisted.extend(lowered);
                        }
                        _ => exec_stmts.extend(lowered),
                    }
                }
                var_hoisted.extend(func_decls);
                var_hoisted.extend(exec_stmts);
                var_hoisted
            } else {
                Vec::new()
            };

            // Prepend destructuring statements to body
            if !destructuring_stmts.is_empty() {
                let mut new_body = destructuring_stmts;
                new_body.append(&mut body);
                body = new_body;
            }

            ctx.exit_scope(scope_mark);

            // Detect captured variables
            let mut all_refs = Vec::new();
                let mut visited_closures = std::collections::HashSet::new();
            for stmt in &body {
                collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
            }

            let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                .map(|(_, id)| *id)
                .collect();
            let param_ids: std::collections::HashSet<LocalId> = params.iter()
                .map(|p| p.id)
                .collect();

            let mut captures: Vec<LocalId> = all_refs.into_iter()
                .filter(|id| outer_local_ids.contains(id) && !param_ids.contains(id))
                .collect();
            captures.sort();
            captures.dedup();
            captures = ctx.filter_module_level_captures(captures);

            // Detect which captures are assigned to inside the closure (need boxing)
            let mut all_assigned = Vec::new();
            for stmt in &body {
                collect_assigned_locals_stmt(stmt, &mut all_assigned);
            }
            let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
            let mutable_captures: Vec<LocalId> = captures.iter()
                .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                .copied()
                .collect();

            // Regular function expressions do NOT capture `this` from enclosing scope
            // (they have their own `this` binding determined by how they're called)
            let captures_this = false;

            Ok(Expr::Closure {
                func_id,
                params,
                return_type: Type::Any,
                body,
                captures,
                mutable_captures,
                captures_this,
                enclosing_class: None,
                is_async: fn_expr.function.is_async,
            })
        }
        ast::Expr::Await(await_expr) => {
            let inner = Box::new(lower_expr(ctx, &await_expr.arg)?);
            Ok(Expr::Await(inner))
        }
        ast::Expr::SuperProp(super_prop) => {
            // super.property access - used in super.method() calls
            // When used as a call target, the Call expression will detect this
            // For now, we'll error on direct property access (super.prop without call)
            match &super_prop.prop {
                ast::SuperProp::Ident(_ident) => {
                    // This is typically used in Call expressions like super.method()
                    // We return a placeholder that will be handled specially
                    crate::lower_bail!(
                        super_prop.span,
                        "Direct super property access not yet supported, use super.method()"
                    );
                }
                ast::SuperProp::Computed(_) => {
                    crate::lower_bail!(
                        super_prop.span,
                        "Computed super property access not supported"
                    );
                }
            }
        }
        ast::Expr::Update(update) => {
            // Handle ++x, x++, --x, x--
            let binary_op = match update.op {
                ast::UpdateOp::PlusPlus => BinaryOp::Add,
                ast::UpdateOp::MinusMinus => BinaryOp::Sub,
            };

            match update.arg.as_ref() {
                // Simple identifier: x++ or ++x
                ast::Expr::Ident(ident) => {
                    let name = ident.sym.to_string();
                    let id = ctx.lookup_local(&name)
                        .ok_or_else(|| anyhow!("Undefined variable in update expression: {}", name))?;
                    let op = match update.op {
                        ast::UpdateOp::PlusPlus => UpdateOp::Increment,
                        ast::UpdateOp::MinusMinus => UpdateOp::Decrement,
                    };
                    Ok(Expr::Update {
                        id,
                        op,
                        prefix: update.prefix,
                    })
                }
                // Member expression: this.count++ or obj.prop++ or obj[key]++
                ast::Expr::Member(member) => {
                    let object = lower_expr(ctx, &member.obj)?;
                    match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let property = ident.sym.to_string();
                            // Desugar: this.count++ becomes (tmp = this.count, this.count = tmp + 1, tmp)
                            // For prefix ++this.count becomes (this.count = this.count + 1, this.count)
                            // We simplify to just: this.count = this.count + 1
                            // The return value semantics are handled at codegen
                            Ok(Expr::PropertyUpdate {
                                object: Box::new(object),
                                property,
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                        ast::MemberProp::PrivateName(priv_name) => {
                            let property = format!("#{}", priv_name.name);
                            Ok(Expr::PropertyUpdate {
                                object: Box::new(object),
                                property,
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                        ast::MemberProp::Computed(comp) => {
                            // Computed property: obj[key]++
                            let index = lower_expr(ctx, &comp.expr)?;
                            Ok(Expr::IndexUpdate {
                                object: Box::new(object),
                                index: Box::new(index),
                                op: binary_op,
                                prefix: update.prefix,
                            })
                        }
                    }
                }
                _ => Err(anyhow!("Update expression only supports identifiers and member expressions")),
            }
        }
        ast::Expr::Tpl(tpl) => {
            // Template literal: `Hello, ${name}!`
            // quasis = ["Hello, ", "!"], exprs = [name]
            // We desugar this to string concatenation

            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            // Start with the first quasi
            let first_raw = tpl.quasis.first()
                .map(|q| q.raw.as_ref())
                .unwrap_or("");
            let mut result = Expr::String(unescape_template(first_raw));

            // Interleave expressions and remaining quasis
            for (i, expr) in tpl.exprs.iter().enumerate() {
                let lowered = lower_expr(ctx, expr)?;
                // Concatenate: result + toString(expr)
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(lowered),
                };

                // Add the next quasi (if it's non-empty)
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
        ast::Expr::OptChain(opt_chain) => {
            // Optional chaining: obj?.prop or obj?.[index] or obj?.method()
            // Convert to: obj == null ? undefined : obj.prop
            match &*opt_chain.base {
                ast::OptChainBase::Member(member) => {
                    // obj?.prop -> obj == null ? undefined : obj.prop
                    let obj_expr = lower_expr(ctx, &member.obj)?;

                    // Get the property access
                    let prop_expr = match &member.prop {
                        ast::MemberProp::Ident(ident) => {
                            let prop_name = ident.sym.to_string();
                            Expr::PropertyGet {
                                object: Box::new(obj_expr.clone()),
                                property: prop_name,
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

                    // Generate: obj == null ? undefined : prop_expr
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::Eq,
                            left: Box::new(obj_expr),
                            right: Box::new(Expr::Null),
                        }),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(prop_expr),
                    })
                }
                ast::OptChainBase::Call(call) => {
                    // obj?.method() -> obj == null ? undefined : obj.method()
                    let callee = &call.callee;

                    // Check for spread arguments
                    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

                    let args = call.args.iter()
                        .map(|arg| lower_expr(ctx, &arg.expr))
                        .collect::<Result<Vec<_>>>()?;

                    // Lower callee as plain MemberExpr, unwrapping inner OptChain.
                    // SWC may wrap the callee member access in an OptChain too.
                    // We must NOT re-lower via lower_expr which would nest Conditionals.
                    let (check_expr, callee_expr) = {
                        let mut lower_member_flat = |member: &ast::MemberExpr| -> Result<(Expr, Expr)> {
                            let obj = lower_expr(ctx, &member.obj)?;
                            let prop = match &member.prop {
                                ast::MemberProp::Ident(id) => Expr::PropertyGet { object: Box::new(obj.clone()), property: id.sym.to_string() },
                                ast::MemberProp::Computed(c) => { let idx = lower_expr(ctx, &c.expr)?; Expr::IndexGet { object: Box::new(obj.clone()), index: Box::new(idx) } },
                                _ => return Err(anyhow!("Unsupported optional chain member")),
                            };
                            Ok((obj, prop))
                        };
                        match &**callee {
                            ast::Expr::Member(m) => lower_member_flat(m)?,
                            ast::Expr::OptChain(inner) => match &*inner.base {
                                ast::OptChainBase::Member(m) => lower_member_flat(m)?,
                                _ => { let ce = lower_expr(ctx, callee)?; (ce.clone(), ce) }
                            },
                            _ => { let ce = lower_expr(ctx, callee)?; (ce.clone(), ce) }
                        }
                    };

                    // If check_expr is already a Conditional from an inner optional chain,
                    // nest the outer call inside its else branch instead of creating another Conditional.
                    // This avoids duplicating side-effecting expressions (like ArrayShift/ArrayPop).
                    if let Expr::Conditional { condition: inner_cond, then_expr: inner_then, else_expr: inner_else } = check_expr {
                        // Build the callee with inner_else as the object (not the full Conditional)
                        let fixed_callee = match callee_expr {
                            Expr::PropertyGet { property, .. } => Expr::PropertyGet { object: inner_else, property },
                            Expr::IndexGet { index, .. } => Expr::IndexGet { object: inner_else, index },
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
                        let spread_args: Vec<CallArg> = call.args.iter().zip(args.iter())
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

                    // Wrap in conditional: check_expr == null ? undefined : call_expr
                    Ok(Expr::Conditional {
                        condition: Box::new(Expr::Compare {
                            op: CompareOp::Eq,
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
        ast::Expr::Seq(seq) => {
            // Comma operator: evaluate all expressions left-to-right, return the last value
            // e.g., (a++, b++, c) evaluates a++, then b++, then returns c
            // All expressions must be evaluated for side effects (e.g., for-loop updates: it3--, i++)
            let mut exprs = Vec::new();
            for expr in &seq.exprs {
                exprs.push(lower_expr(ctx, expr)?);
            }
            if exprs.len() == 1 {
                Ok(exprs.pop().unwrap())
            } else {
                Ok(Expr::Sequence(exprs))
            }
        }
        ast::Expr::MetaProp(meta_prop) => {
            // import.meta expression
            // We only support import.meta itself - property access (.url) is handled in Member expr
            // For now, return a placeholder object that will be handled in property access
            match meta_prop.kind {
                ast::MetaPropKind::ImportMeta => {
                    // Return the file:// URL directly for import.meta.url
                    // Since import.meta is typically accessed via .url, we generate the URL here
                    let file_url = format!("file://{}", ctx.source_file_path);
                    Ok(Expr::Object(vec![
                        ("url".to_string(), Expr::String(file_url)),
                    ]))
                }
                ast::MetaPropKind::NewTarget => {
                    // Inside a class constructor, `new.target` evaluates to the
                    // class itself. We approximate this with a small object
                    // literal `{ name: <class_name> }` so:
                    //   - `new.target ? a : b` is truthy → takes the `a` branch
                    //   - `new.target.name` returns the class name string
                    // Outside a constructor (e.g., a regular function called
                    // without `new`), `new.target` is `undefined`.
                    if let Some(class_name) = ctx.in_constructor_class.clone() {
                        Ok(Expr::Object(vec![
                            ("name".to_string(), Expr::String(class_name)),
                        ]))
                    } else {
                        Ok(Expr::Undefined)
                    }
                }
            }
        }
        ast::Expr::Yield(y) => {
            let value = match &y.arg {
                Some(arg) => Some(Box::new(lower_expr(ctx, arg)?)),
                None => None,
            };
            Ok(Expr::Yield { value, delegate: y.delegate })
        }
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
                let first_raw = tpl.quasis.first()
                    .map(|q| q.raw.as_ref())
                    .unwrap_or("");
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

            // General case: desugar to `tag(stringsArray, ...exprs)`
            // The strings array uses each quasi's COOKED value (with escapes
            // processed). Per spec it should also have a `.raw` property, but
            // most user code doesn't read it; if a test exercises that we can
            // upgrade to a wrapper object later.
            let cooked_strings: Vec<Expr> = tpl
                .quasis
                .iter()
                .map(|q| {
                    // Each quasi has both `raw` and an optional `cooked` form;
                    // prefer `cooked` so escapes like `\n` are processed.
                    // `cooked` is a `Wtf8Atom` whose `as_str()` returns `Option<&str>`
                    // (None when the original source had non-UTF8 bytes — falls back to raw).
                    let cooked_owned: Option<String> = q
                        .cooked
                        .as_ref()
                        .and_then(|c| c.as_str().map(|s| s.to_string()));
                    let s = cooked_owned.unwrap_or_else(|| q.raw.as_ref().to_string());
                    Expr::String(s)
                })
                .collect();
            let strings_array = Expr::Array(cooked_strings);

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
        // Class expression used as a value (not in `new` context)
        ast::Expr::Class(class_expr) => {
            let ident_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
            let synthetic_name = ident_name.unwrap_or_else(|| format!("__anon_class_{}", ctx.fresh_class()));
            let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
            ctx.pending_classes.push(class);
            // Return as a New expression with no args (creates the class object reference)
            Ok(Expr::New { class_name: synthetic_name, args: vec![], type_args: vec![] })
        }
        ast::Expr::JSXElement(jsx) => {
            lower_jsx_element(ctx, jsx)
        }
        ast::Expr::JSXFragment(jsx) => {
            lower_jsx_fragment(ctx, jsx)
        }
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
fn try_desugar_reactive_text(
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
        let ast::Expr::Member(member) = expr.as_ref() else { continue };
        let ast::MemberProp::Ident(prop) = &member.prop else { continue };
        if prop.sym.as_ref() != "value" { continue }
        let ast::Expr::Ident(obj_ident) = member.obj.as_ref() else { continue };
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
        let state_local = ctx.lookup_local(state_name)
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
            is_rest: false,
        };
        let fresh_concat = lower_tpl_to_concat(ctx, tpl)?;
        let set_text_call = Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "textSetString".to_string(),
            object: None,
            args: vec![
                Expr::LocalGet(widget_id),
                fresh_concat,
            ],
            class_name: None,
        };
        let inner_body = vec![Stmt::Expr(set_text_call)];
        ctx.exit_scope(inner_scope);

        let mut inner_refs = Vec::new();
        let mut inner_visited = std::collections::HashSet::new();
        for stmt in &inner_body {
            collect_local_refs_stmt(stmt, &mut inner_refs, &mut inner_visited);
        }
        let mut inner_captures: Vec<LocalId> = inner_refs.into_iter()
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
    let mut outer_captures: Vec<LocalId> = outer_refs.into_iter()
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
fn try_desugar_reactive_animate(
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
        let state_local = ctx.lookup_local(state_name)
            .ok_or_else(|| anyhow!("reactive animate: state '{}' not in scope", state_name))?;

        let inner_func_id = ctx.fresh_func();
        let inner_scope = ctx.enter_scope();
        let v_param_id = ctx.define_local("__v".to_string(), Type::Any);
        let v_param = Param {
            id: v_param_id,
            name: "__v".to_string(),
            ty: Type::Any,
            default: None,
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
        let mut inner_captures: Vec<LocalId> = inner_refs.into_iter()
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
    let mut outer_captures: Vec<LocalId> = outer_refs.into_iter()
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
fn try_lower_widget_decl(
    ctx: &LoweringContext,
    call_expr: &ast::CallExpr,
) -> Option<WidgetDecl> {
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
    let mut reload_after_seconds: Option<u32> = None;

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
                    for elem in &arr.elems {
                        if let Some(ast::ExprOrSpread { expr, .. }) = elem {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                                supported_families.push(s.value.as_str().unwrap_or("").to_string());
                            }
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
                                if let Some(param) = parse_widget_config_param(&param_name, &field_kv.value) {
                                    config_params.push(param);
                                }
                            }
                        }
                    }
                }
            }
            "provider" => {
                // Arrow function provider: provider: async (config) => { ... }
                match kv.value.as_ref() {
                    ast::Expr::Arrow(_arrow) => {
                        let func_name = if kind.is_empty() {
                            "__widget_provider_widget".to_string()
                        } else {
                            let safe = kind.rsplit('.').next().unwrap_or(&kind);
                            format!("__widget_provider_{}", safe)
                        };
                        provider_func_name = Some(func_name);
                    }
                    _ => {}
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
                match kv.value.as_ref() {
                    ast::Expr::Arrow(arrow) => {
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
                    _ => {}
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
        ast::TsType::TsUnionOrIntersectionType(ast::TsUnionOrIntersectionType::TsUnionType(union)) => {
            // Check for T | null or T | undefined → Optional(T)
            let mut non_null_types: Vec<&ast::TsType> = Vec::new();
            let mut has_null = false;
            for member in &union.types {
                match member.as_ref() {
                    ast::TsType::TsKeywordType(kw) if matches!(kw.kind,
                        ast::TsKeywordTypeKind::TsNullKeyword | ast::TsKeywordTypeKind::TsUndefinedKeyword
                    ) => {
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
                    let content = call.args.first()
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
                "Image" => {
                    parse_image_node(&call.args)
                }
                "Spacer" => {
                    Some(WidgetNode::Spacer)
                }
                "Divider" => {
                    Some(WidgetNode::Divider)
                }
                "ForEach" => {
                    parse_foreach_node(&call.args)
                }
                "Label" => {
                    parse_label_node(&call.args)
                }
                "Gauge" => {
                    parse_gauge_node(&call.args)
                }
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
            for elem in &arr.elems {
                if let Some(ast::ExprOrSpread { expr, .. }) = elem {
                    if let Some(node) = parse_widget_node(expr) {
                        children.push(node);
                    }
                }
            }
        }
    }

    // Parse modifiers from remaining args
    let modifier_start = children_arg_idx + 1;
    modifiers = parse_modifiers_from_args(args, modifier_start);

    Some(WidgetNode::Stack { kind, spacing, children, modifiers })
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
    Some(WidgetNode::Image { system_name, modifiers })
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
fn is_widget_modifier_name(name: &str) -> bool {
    matches!(name,
        "font" | "fontWeight" | "weight" | "foregroundColor" | "color" | "foreground" |
        "padding" | "cornerRadius" | "background" | "backgroundColor" |
        "opacity" | "lineLimit" | "frame" | "minimumScaleFactor" |
        "containerBackground" | "maxWidth" | "url" |
        "bold" | "italic" | "underline" | "fontSize" |
        "strikethrough" | "multilineTextAlignment" | "lineSpacing"
    )
}

/// Parse a single modifier from key/value
fn parse_single_modifier(key: &str, value: &ast::Expr) -> Option<WidgetModifier> {
    match key {
        "font" => {
            match value {
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
            }
        }
        "fontWeight" | "weight" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::FontWeight(s.value.as_str().unwrap_or("").to_string()))
            } else {
                None
            }
        }
        "foregroundColor" | "color" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ForegroundColor(s.value.as_str().unwrap_or("").to_string()))
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
                Some(WidgetModifier::Background(s.value.as_str().unwrap_or("").to_string()))
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
                Some(WidgetModifier::ContainerBackground(s.value.as_str().unwrap_or("").to_string()))
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
                Some(WidgetModifier::WidgetURL(s.value.as_str().unwrap_or("").to_string()))
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
    let text = args.first()
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

    Some(WidgetNode::Label { text, system_image, modifiers })
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
                    } else { return None; }
                }
                _ => return None,
            };
            let right = match bin.right.as_ref() {
                ast::Expr::Member(m) => {
                    if let ast::MemberProp::Ident(p) = &m.prop {
                        p.sym.to_string()
                    } else { return None; }
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

    Some(WidgetNode::Gauge { value_expr, label, style, modifiers })
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
                if let Some((family_value, node)) = try_parse_family_case(&if_stmt.test, &if_stmt.cons, family_name) {
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
                            if let Some((family_value, node)) = try_parse_family_case(&nested_if.test, &nested_if.cons, family_name) {
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

    Some(WidgetNode::FamilySwitch { cases, default: default_node })
}

/// Try to parse a single if (family === "value") { return node } case
fn try_parse_family_case(test: &ast::Expr, cons: &ast::Stmt, family_name: &str) -> Option<(String, WidgetNode)> {
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
                        "default" => {
                            match kv.value.as_ref() {
                                ast::Expr::Lit(ast::Lit::Str(s)) => {
                                    default_str = s.value.as_str().unwrap_or("").to_string();
                                }
                                ast::Expr::Lit(ast::Lit::Bool(b)) => {
                                    default_bool = b.value;
                                }
                                _ => {}
                            }
                        }
                        "values" => {
                            if let ast::Expr::Array(arr) = kv.value.as_ref() {
                                for elem in &arr.elems {
                                    if let Some(ast::ExprOrSpread { expr, .. }) = elem {
                                        if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                                            values.push(s.value.as_str().unwrap_or("").to_string());
                                        }
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
                default: if default_str.is_empty() { "".to_string() } else { default_str },
            },
            "bool" | "boolean" => WidgetConfigParamType::Bool { default: default_bool },
            "string" => WidgetConfigParamType::String { default: default_str },
            _ => WidgetConfigParamType::String { default: default_str },
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
        ast::Expr::Lit(ast::Lit::Num(n)) => {
            WidgetPlaceholderValue::Number(n.value)
        }
        ast::Expr::Lit(ast::Lit::Bool(b)) => {
            WidgetPlaceholderValue::Bool(b.value)
        }
        ast::Expr::Lit(ast::Lit::Null(_)) => {
            WidgetPlaceholderValue::Null
        }
        ast::Expr::Array(arr) => {
            let items: Vec<WidgetPlaceholderValue> = arr.elems.iter()
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
fn is_generator_call_expr(ctx: &LoweringContext, expr: &Expr) -> bool {
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
                        if matches!(prop,
                            "charAt" | "slice" | "substring" | "substr" | "trim" |
                            "trimStart" | "trimEnd" | "toLowerCase" | "toUpperCase" |
                            "replace" | "replaceAll" | "padStart" | "padEnd" |
                            "repeat" | "normalize" | "concat" | "toString" |
                            "toLocaleLowerCase" | "toLocaleUpperCase"
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
                    if method.sym.as_ref() == "exec" {
                        return match member.obj.as_ref() {
                            ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                            ast::Expr::Ident(ident) => ctx
                                .lookup_local_type(&ident.sym.to_string())
                                .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                                .unwrap_or(false),
                            _ => false,
                        };
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

        assert!(matches!(ctx.lookup_enum_member("Color", "Red"), Some(EnumValue::Number(0))));
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
        ctx.register_native_module("uuid".to_string(), "uuid".to_string(), Some("v4".to_string()));

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

        ctx.enter_type_param_scope(&[TypeParam { name: "T".to_string(), constraint: None, default: None }]);
        assert!(ctx.is_type_param("T"));
        assert!(!ctx.is_type_param("U"));

        // Nested scope
        ctx.enter_type_param_scope(&[TypeParam { name: "U".to_string(), constraint: None, default: None }]);
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
        ctx.namespace_vars.push(("Utils".to_string(), "helper".to_string(), local_id));

        assert_eq!(ctx.lookup_namespace_var("Utils", "helper"), Some(local_id));
        assert_eq!(ctx.lookup_namespace_var("Utils", "missing"), None);
        assert_eq!(ctx.lookup_namespace_var("Other", "helper"), None);
    }
}
