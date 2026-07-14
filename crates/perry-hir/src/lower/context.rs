//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

fn stable_tagged_template_site_salt(source_file_path: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in source_file_path.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h & 0xFFFF_FFFF
}

impl LoweringContext {
    // #854: single-arg constructor (delegates to `with_class_id_start`).
    // Currently only exercised from the `#[cfg(test)]` lowering tests, so it
    // reads as dead in a non-test build. Kept as the canonical entry point.
    #[allow(dead_code)]
    pub fn new(source_file_path: impl Into<String>) -> Self {
        Self::with_class_id_start(source_file_path, 1)
    }

    pub fn with_class_id_start(
        source_file_path: impl Into<String>,
        start_class_id: ClassId,
    ) -> Self {
        let source_file_path = source_file_path.into();
        let tagged_template_site_salt = stable_tagged_template_site_salt(&source_file_path);
        Self {
            next_local_id: 0,
            next_global_id: 0,
            next_func_id: 0,
            next_class_id: start_class_id, // Start from the provided ID to avoid collisions across modules
            next_enum_id: 0,
            next_interface_id: 0,
            next_type_alias_id: 0,
            tagged_template_site_salt,
            next_tagged_template_site_id: 0,
            locals: crate::lower::Locals::new(),
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
            immutable_locals: HashSet::new(),
            interface_source_keys: std::collections::HashMap::new(),
            interface_object_types: std::collections::HashMap::new(),
            imported_functions: Vec::new(),
            builtin_named_imports: Vec::new(),
            native_modules: Vec::new(),
            builtin_module_aliases: Vec::new(),
            subns_path_aliases: HashMap::new(),
            type_param_scopes: Vec::new(),
            type_param_constraints: Vec::new(),
            native_instances: Vec::new(),
            param_native_hints: HashMap::new(),
            current_strict: false,
            ui_widget_type_aliases: HashMap::new(),
            deferred_unknown_native_imports: HashMap::new(),
            current_class: None,
            current_class_inner_name: None,
            pending_class_inner_name: None,
            current_class_member_is_static: false,
            private_scopes: Vec::new(),
            object_super_home_stack: Vec::new(),
            extern_func_types: Vec::new(),
            source_file_path,
            exportable_object_vars: HashSet::new(),
            pending_functions: Vec::new(),
            closure_display_names: HashMap::new(),
            class_display_names: HashMap::new(),
            gen_param_prologue_len: HashMap::new(),
            assignment_inferred_name: None,
            inferred_class_bindings: std::collections::HashSet::new(),
            closure_source_text: HashMap::new(),
            func_return_native_instances: Vec::new(),
            pending_classes: Vec::new(),
            func_return_types: Vec::new(),
            resolved_types: None,
            pre_registered_module_vars: HashSet::new(),
            pre_registered_module_var_decls: HashSet::new(),
            module_level_ids: HashSet::new(),
            sloppy_implicit_globals: Vec::new(),
            sloppy_implicit_global_ids: HashSet::new(),
            with_sloppy_implicit_ids: std::collections::HashMap::new(),
            pending_with_implicit_inits: Vec::new(),
            scope_depth: 0,
            scope_local_marks: Vec::new(),
            scope_module_shadow_marks: Vec::new(),
            inside_block_scope: 0,
            namespace_vars: Vec::new(),
            current_namespace: None,
            module_native_instances: Vec::new(),
            uses_fetch: false,
            uses_webassembly: false,
            react_default_import_local: None,
            suppress_stdlib_dispatch_guard_once: false,
            lowering_call_callee: false,
            unresolved_ident_as_global: false,
            with_env_stack: Vec::new(),
            var_hoisted_ids: HashSet::new(),
            tdz_forward_ids: HashSet::new(),
            forward_lexical_names: HashSet::new(),
            forward_lexical_saves: Vec::new(),
            catch_param_scopes: Vec::new(),
            annexb_block_fn_var_ids: HashMap::new(),
            annexb_block_fn_names_all: HashSet::new(),
            lexical_forward_decls: HashMap::new(),
            nested_forward_scope_ids: HashSet::new(),
            functions_index: HashMap::new(),
            classes_index: HashMap::new(),
            imported_functions_index: HashMap::new(),
            builtin_module_aliases_index: HashMap::new(),
            native_instances_index: HashMap::new(),
            module_native_instances_index: HashMap::new(),
            func_return_native_instances_index: HashMap::new(),
            prescan_protected_native_params: std::collections::HashMap::new(),
            native_modules_index: HashMap::new(),
            module_shadow_stack: Vec::new(),
            class_statics_index: HashMap::new(),
            weakref_locals: HashSet::new(),
            finreg_locals: HashSet::new(),
            weakmap_locals: HashSet::new(),
            weakset_locals: HashSet::new(),
            namespace_import_locals: HashSet::new(),
            fetch_call_response_locals: HashSet::new(),
            namespace_import_sources: std::collections::HashMap::new(),
            generator_func_names: HashSet::new(),
            async_generator_func_names: HashSet::new(),
            nested_generator_forward_referenced: HashSet::new(),
            iterator_func_for_class: std::collections::HashMap::new(),
            proxy_locals: HashSet::new(),
            builtin_proto_method_locals: HashMap::new(),
            wasm_instance_locals: HashSet::new(),
            plain_object_locals: HashSet::new(),
            proxy_revoke_locals: HashMap::new(),
            class_expr_aliases: HashMap::new(),
            in_constructor_class: None,
            current_class_is_derived: false,
            in_class_field_init: false,
            current_class_super_ident: None,
            mixin_funcs: HashMap::new(),
            anon_shape_classes: HashMap::new(),
            forward_class_names: std::collections::HashSet::new(),
            forward_class_decl_depth: std::collections::HashMap::new(),
            class_renames: std::collections::HashMap::new(),
            next_class_rename_id: 0,
            module_class_decl_names: std::collections::HashSet::new(),
            next_anon_shape_id: 0,
            class_method_return_types: Vec::new(),
            class_captures: Vec::new(),
            let_class_aliases: Vec::new(),
            global_this_aliases: HashSet::new(),
            prototype_aliases: HashMap::new(),
            prototype_function_aliases: HashMap::new(),
            function_valued_locals: HashSet::new(),
            prototype_function_locals: HashMap::new(),
            object_static_method_aliases: HashMap::new(),
            array_static_method_aliases: HashMap::new(),
            is_entry_module: false,
            saw_global_this_expr: false,
            reassigned_top_level_identifiers: HashSet::new(),
            module_strict: false,
            strict_mode_stack: Vec::new(),
            is_external_module: false,
            optional_require_try_depth: 0,
            fn_ctor_env: super::fn_ctor_env::FnCtorEnv::default(),
            expr_lower_depth: 0,
            prelowered_member_receiver: None,
            in_nonarrow_fn: false,
        }
    }

    pub(crate) fn fresh_tagged_template_site_id(&mut self) -> u64 {
        let local_id = self.next_tagged_template_site_id;
        self.next_tagged_template_site_id = self.next_tagged_template_site_id.wrapping_add(1);
        (self.tagged_template_site_salt << 32) | u64::from(local_id)
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
        // Constraint table mirrors the same scope so `is_type_param(name)`
        // and `resolve_type_param_constraint(name)` agree on visibility.
        // Only params with a declared upper bound contribute an entry.
        let constraints: HashMap<String, perry_types::Type> = type_params
            .iter()
            .filter_map(|p| {
                p.constraint
                    .as_ref()
                    .map(|c| (p.name.clone(), (**c).clone()))
            })
            .collect();
        self.type_param_constraints.push(constraints);
    }

    /// Exit the current type parameter scope
    pub(crate) fn exit_type_param_scope(&mut self) {
        self.type_param_scopes.pop();
        self.type_param_constraints.pop();
    }

    /// Check if a name is a type parameter in the current scope
    pub(crate) fn is_type_param(&self, name: &str) -> bool {
        self.type_param_scopes
            .iter()
            .any(|scope| scope.contains(name))
    }

    /// Resolve a type-parameter reference to its declared upper-bound
    /// constraint, when that constraint is a runtime-meaningful type
    /// (`String`, `Number`, `Boolean`, `BigInt`, `Array<T>`). Returns
    /// `None` for parameters with no constraint or with constraints that
    /// don't usefully narrow the runtime representation (`unknown`/`any`/
    /// `object`/named-class/union — those keep `TypeVar(T)` so
    /// monomorphization or downstream native-instance tagging still has
    /// the chance to substitute a concrete type).
    ///
    /// Innermost scope wins (shadowing).
    pub(crate) fn resolve_type_param_constraint(&self, name: &str) -> Option<perry_types::Type> {
        for scope in self.type_param_constraints.iter().rev() {
            if let Some(ty) = scope.get(name) {
                // Only return constraints whose runtime shape is
                // narrower than `Any`. `String`/`Number`/`Boolean`/
                // `BigInt` are primitives; `Array<elem>` lets
                // `<T extends string[]>` propagate to the array fast
                // path. Anything else (e.g., `T extends SomeClass`,
                // `T extends "literal"`, intersections) falls back to
                // the `TypeVar`/`Named` path so existing native-
                // instance tagging / class-id propagation still work.
                let useful = matches!(
                    ty,
                    perry_types::Type::String
                        | perry_types::Type::Number
                        | perry_types::Type::Boolean
                        | perry_types::Type::BigInt
                        | perry_types::Type::Array(_)
                );
                if useful {
                    return Some(ty.clone());
                } else {
                    return None;
                }
            }
        }
        None
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

impl LoweringContext {
    pub(crate) fn fresh_local(&mut self) -> LocalId {
        let id = self.next_local_id;
        self.next_local_id += 1;
        id
    }

    /// Push a private-name scope for a class body (innermost last). Built by
    /// pre-scanning the class members. See `private_scopes`.
    pub(crate) fn push_private_scope(&mut self, scope: PrivateScope) {
        self.private_scopes.push(scope);
    }

    pub(crate) fn pop_private_scope(&mut self) {
        self.private_scopes.pop();
    }

    /// Resolve a private name (`#name`, with the leading `#`) to its declaring
    /// class and member kind by walking the private-scope stack innermost
    /// outward — matching the lexical resolution rule for private names. A
    /// nested class that redeclares the same name shadows the outer one.
    pub(crate) fn resolve_private(&self, field_name: &str) -> Option<(String, u32, PrivMember)> {
        for scope in self.private_scopes.iter().rev() {
            if let Some(m) = scope.members.get(field_name) {
                return Some((scope.class_name.clone(), scope.class_id, *m));
            }
        }
        None
    }

    pub(crate) fn mark_local_immutable(&mut self, id: LocalId) {
        self.immutable_locals.insert(id);
    }

    pub(crate) fn is_local_immutable(&self, id: LocalId) -> bool {
        self.immutable_locals.contains(&id)
    }

    pub(crate) fn fresh_func(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id += 1;
        id
    }

    pub(crate) fn current_strict_mode(&self) -> bool {
        self.strict_mode_stack
            .last()
            .copied()
            .unwrap_or(self.module_strict)
    }

    pub(crate) fn enter_strict_mode(&mut self, strict: bool) {
        self.strict_mode_stack.push(strict);
    }

    pub(crate) fn exit_strict_mode(&mut self) {
        self.strict_mode_stack.pop();
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
                        arguments_object: None,
                    }],
                    return_type: Type::Any,
                    body: vec![Stmt::Return(Some(coerce_body))],
                    captures: vec![],
                    mutable_captures: vec![],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    is_strict: self.current_strict,
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

    /// Apply any active scope-local class-name alias (see `class_renames`).
    /// Identity for non-aliased names, so non-colliding classes are unaffected.
    pub(crate) fn resolve_class_name(&self, name: &str) -> String {
        self.class_renames
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// Register a scope-local rename for `class X` when an outer/prior `class X`
    /// is already registered (a distinct class that the name-keyed dedup would
    /// otherwise skip). Returns immediately if no collision or already aliased.
    /// Call from each body's Phase-1.5 class scan.
    pub(crate) fn maybe_rename_colliding_class(&mut self, name: &str) {
        if self.lookup_class(name).is_some() && !self.class_renames.contains_key(name) {
            let unique = format!("{}${}", name, self.next_class_rename_id);
            self.next_class_rename_id += 1;
            self.class_renames.insert(name.to_string(), unique);
        }
    }

    /// Is `name` a user-declared `interface`? Interfaces are not classes, so
    /// `lookup_class` returns `None` for them — but an interface-typed value is
    /// still the object's own type, whose methods must dispatch to its own
    /// members, never to an Array/builtin fast-path intrinsic. Used by the
    /// array-only-method fold to recognize interface receivers (follow-up to
    /// #5139, which covered only `any`-typed receivers).
    pub(crate) fn is_interface_type(&self, name: &str) -> bool {
        self.interfaces.iter().any(|(n, _)| n == name)
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

    /// Issue #665: register getter and setter property names for a class.
    /// Mirrors `register_class_field_names`; consumed by the ctor-body
    /// field-detection pass to skip names that are accessors. Stored as the
    /// own+inherited union so a child lookup sees the full chain in one hop.
    pub(crate) fn register_class_accessor_names(
        &mut self,
        class_name: String,
        accessor_names: crate::ClassAccessorNames,
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

    /// Look up the accessor property names registered for a
    /// class. The stored list includes inherited accessors (mirroring how
    /// `class_field_names` stores the own+inherited union), so callers do
    /// not need to walk the parent chain themselves.
    pub(crate) fn lookup_class_accessor_names(
        &self,
        class_name: &str,
    ) -> Option<&crate::ClassAccessorNames> {
        self.class_accessor_names
            .iter()
            .find(|(n, _)| n == class_name)
            .map(|(_, f)| f)
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

    /// Pre-seed class accessor names with cross-module class info collected
    /// from already-lowered dependencies. This lets constructor-field
    /// inference avoid creating data slots for inherited imported accessors.
    pub fn seed_imported_class_accessors(
        &mut self,
        seeds: &std::collections::HashMap<String, crate::ClassAccessorNames>,
    ) {
        for (name, accessors) in seeds {
            if !self.class_accessor_names.iter().any(|(n, _)| n == name) {
                self.class_accessor_names
                    .push((name.clone(), accessors.clone()));
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
        // Forward-scan (first-match-wins): index keeps the FIRST entry per name.
        let idx = self.class_statics.len();
        self.class_statics_index
            .entry(class_name.clone())
            .or_insert(idx);
        self.class_statics
            .push((class_name, static_fields, static_methods));
    }

    pub(crate) fn has_static_field(&self, class_name: &str, field_name: &str) -> bool {
        self.class_statics_index
            .get(class_name)
            .map(|&idx| self.class_statics[idx].1.iter().any(|f| f == field_name))
            .unwrap_or(false)
    }

    pub(crate) fn has_static_method(&self, class_name: &str, method_name: &str) -> bool {
        self.class_statics_index
            .get(class_name)
            .map(|&idx| self.class_statics[idx].2.iter().any(|m| m == method_name))
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

    pub(crate) fn define_sloppy_implicit_global(&mut self, name: String) -> LocalId {
        if let Some((_, id, _)) = self
            .locals
            .iter()
            .rev()
            .find(|(n, id, _)| n == &name && self.sloppy_implicit_global_ids.contains(id))
        {
            return *id;
        }
        let id = self.fresh_local();
        self.module_level_ids.insert(id);
        self.sloppy_implicit_global_ids.insert(id);
        self.sloppy_implicit_globals.push((name.clone(), id));
        self.locals.push((name, id, Type::Any));
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
        self.locals.lookup(name)
    }

    /// Like `lookup_local`, but only searches locals defined in the CURRENT
    /// function scope (at or after the most recent `enter_scope` mark). Used by
    /// function-declaration hoisting so a nested `function a` SHADOWS an
    /// outer-scope binding of the same name (fresh local + box) instead of
    /// reusing the outer local's box — which, when the outer binding is a
    /// closure-captured variable (a webpack chunk's `function a` require
    /// captured by an inner IIFE, with `function a` error-formatters in nested
    /// module factories), let the nested declaration overwrite the captured
    /// box at runtime.
    pub(crate) fn lookup_local_in_current_scope(&self, name: &str) -> Option<LocalId> {
        let scope_start = self.scope_local_marks.last().copied().unwrap_or(0);
        self.locals[scope_start..]
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, _)| *id)
    }

    fn lookup_local_index(&self, name: &str) -> Option<usize> {
        self.locals.lookup_index(name)
    }

    /// The function-scope depth at which the nearest local binding of `name`
    /// was declared, or `None` if there is no such binding. A binding's depth
    /// is the number of `enter_scope` (function/closure-boundary) marks that
    /// precede-or-equal its position in the locals stack — i.e. how deeply
    /// nested the function it lives in is. Used to resolve a bare-ident
    /// reference between a same-named `class` and a captured outer local by JS
    /// nearest-binding rules: the binding at the GREATER depth (nearer the
    /// reference) wins.
    pub(crate) fn local_decl_scope_depth(&self, name: &str) -> Option<usize> {
        let pos = self.lookup_local_index(name)?;
        let depth = self
            .scope_local_marks
            .iter()
            .filter(|&&mark| mark <= pos)
            .count();
        Some(depth)
    }

    /// #5216: drop the most-recently-bound local named `name` (if any), e.g. a
    /// module-var the top-level pre-scan registered for `const ns =
    /// require("<native>")`. After this, a bare read of `name` resolves to its
    /// native-module / builtin-alias registration instead of an
    /// always-`undefined` `LocalGet`, matching how `import * as ns` (which
    /// never creates a local) behaves. Returns the removed `LocalId`.
    pub(crate) fn remove_local_binding(&mut self, name: &str) -> Option<LocalId> {
        let idx = self.lookup_local_index(name)?;
        let (_, id, _) = self.locals.remove(idx);
        self.pre_registered_module_vars.remove(name);
        self.pre_registered_module_var_decls.remove(name);
        Some(id)
    }

    pub(crate) fn push_with_env(&mut self, local_id: LocalId) {
        let local_mark = self.locals.len();
        self.with_env_stack.push(WithEnvFrame {
            local_id,
            local_mark,
        });
    }

    pub(crate) fn pop_with_env(&mut self) {
        self.with_env_stack.pop();
    }

    pub(crate) fn active_with_envs_for_ident(&self, name: &str) -> Vec<LocalId> {
        let nearest_local_index = self.lookup_local_index(name);
        let mut envs = Vec::new();
        for frame in self.with_env_stack.iter().rev() {
            if nearest_local_index.is_some_and(|idx| idx >= frame.local_mark) {
                break;
            }
            envs.push(frame.local_id);
        }
        envs
    }

    pub(crate) fn shadows_unqualified_global(&self, name: &str) -> bool {
        self.lookup_local(name).is_some()
            || self.lookup_func(name).is_some()
            || self.lookup_imported_func(name).is_some()
            || self.lookup_class(name).is_some()
    }

    pub(crate) fn lookup_local_type(&self, name: &str) -> Option<&Type> {
        self.locals.lookup_type(name)
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
    /// literal. `field_shapes` is parallel to the literal's source-declared
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
        field_shapes: &[(String, Type)],
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
        for (name, ty) in field_shapes {
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
        let fields: Vec<ClassField> = field_shapes
            .iter()
            .map(|(name, ty)| ClassField {
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
        let mut ctor_params: Vec<Param> = Vec::with_capacity(field_shapes.len());
        let mut ctor_body: Vec<Stmt> = Vec::with_capacity(field_shapes.len());
        for (name, ty) in field_shapes {
            let param_id = self.fresh_local();
            ctor_params.push(Param {
                id: param_id,
                name: name.clone(),
                ty: ty.clone(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
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
            is_strict: true,
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
            heritage_lexically_shadowed: false,
            fields,
            constructor: Some(constructor),
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            // Synthetic anon-shape class; no static fields, so static-init
            // timing is irrelevant.
            is_nested: false,
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
    ) -> Option<(&[Option<Expr>], &[LocalId], Option<usize>, bool)> {
        self.func_defaults
            .iter()
            .find(|(id, _, _, _, _)| *id == func_id)
            .map(|(_, defaults, param_ids, rest_idx, has_synth_args)| {
                (
                    defaults.as_slice(),
                    param_ids.as_slice(),
                    *rest_idx,
                    *has_synth_args,
                )
            })
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

    pub(crate) fn register_builtin_named_import(
        &mut self,
        local_name: String,
        module_name: String,
        exported_name: String,
    ) {
        self.builtin_named_imports
            .push((local_name, module_name, exported_name));
    }

    pub(crate) fn lookup_builtin_named_import(&self, name: &str) -> Option<(&str, &str)> {
        self.builtin_named_imports
            .iter()
            .find(|(local, _, _)| local == name)
            .map(|(_, module, exported)| (module.as_str(), exported.as_str()))
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
        // Forward-scan (first-match-wins) semantics: only record the FIRST
        // index for a name so lookups match the old `.iter().find()`.
        let idx = self.native_modules.len();
        self.native_modules_index
            .entry(local_name.clone())
            .or_insert(idx);
        self.native_modules
            .push((local_name, module_name, method_name));
    }

    pub(crate) fn lookup_native_module(&self, name: &str) -> Option<(&str, Option<&str>)> {
        // #wall5: a local binding (function parameter / `const`) named the same
        // as a registered native module (`url`, `util`, `path`, …) SHADOWS that
        // module within its scope — `native_modules_index` is module-global and
        // first-match-wins, so without this a nested `function(url){ url.push() }`
        // (a local array) or undici's own `util` object would route `url.push` /
        // `util.isStream` through the node-module dispatch and the
        // unimplemented-API gate fires (Next.js app-page-turbo: 88× `url.push`,
        // 84× `util.destroy`, the `url.o` render throw). Mirrors the scope-aware
        // `native_instances` shadowing (shadow_native_instance / truncate).
        if self.module_shadow_stack.iter().any(|n| n == name) {
            return None;
        }
        self.native_modules_index.get(name).map(|&idx| {
            let (_, m, method) = &self.native_modules[idx];
            (m.as_str(), method.as_ref().map(|s| s.as_str()))
        })
    }

    /// #wall5: shadow a native-module name for the current scope IF it is a
    /// registered module (so a local/param of that name resolves as a value, not
    /// the module). No-op for non-module names. Restore with
    /// `truncate_module_shadow` at scope exit. Parallel to
    /// `shadow_native_instance_if_present`.
    pub(crate) fn shadow_native_module_if_present(&mut self, name: &str) {
        if self.native_modules_index.contains_key(name) {
            self.module_shadow_stack.push(name.to_string());
        }
    }

    /// Current depth of the module-shadow stack (a scope mark).
    pub(crate) fn module_shadow_mark(&self) -> usize {
        self.module_shadow_stack.len()
    }

    /// Restore the module-shadow stack to `mark`, re-exposing modules whose
    /// shadowing local bindings went out of scope.
    pub(crate) fn truncate_module_shadow(&mut self, mark: usize) {
        self.module_shadow_stack.truncate(mark);
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

    /// #1750: record `const w = <root>.win32` / `.posix` so that later
    /// `w.<method>(...)` calls dispatch like `path.<sub>.<method>(...)`. The
    /// root identifier is stored unresolved (imports aren't processed yet at
    /// pre-scan time); the `path` check happens when the call is lowered.
    pub(crate) fn register_subns_path_alias(&mut self, local: String, root: String, sub: String) {
        self.subns_path_aliases.insert(local, (root, sub));
    }

    /// Look up a local recorded by `register_subns_path_alias`, returning
    /// `(root_identifier_name, sub_namespace)`.
    pub(crate) fn lookup_subns_path_alias(&self, name: &str) -> Option<(&str, &str)> {
        self.subns_path_aliases
            .get(name)
            .map(|(root, sub)| (root.as_str(), sub.as_str()))
    }

    /// Register `local_name` as a native instance of `module_name`/`class_name`.
    /// Returns `false` (a no-op) when the package is a `perry.compilePackages`
    /// override (#5137) — callers that gate a side effect on successful
    /// registration (e.g. `protect_native_param`) must check this.
    pub(crate) fn register_native_instance(
        &mut self,
        local_name: String,
        module_name: String,
        class_name: String,
    ) -> bool {
        // #5137: if the user opted this package into `perry.compilePackages`,
        // its real npm source is being compiled and the binding resolves to
        // the compiled-from-source class. Registering a native instance here
        // would re-route the instance's fluent methods (`new Command()` →
        // `.name()`/`.option()`/`.parse()`) to the `js_commander_*` native
        // shim that was deliberately kept off the import path — so the call
        // emits an FFI reference the source-compile build never links (or, in
        // a shimless build, returns `undefined`). Back off so the source class
        // is used. `is_native_module` already makes the same back-off for the
        // import-resolution side (#665).
        if is_compile_package_override(&module_name) {
            return false;
        }
        // Push the new index onto this name's shadow stack (innermost last).
        let idx = self.native_instances.len();
        self.native_instances_index
            .entry(local_name.clone())
            .or_default()
            .push(idx);
        self.native_instances
            .push((local_name, module_name, class_name));
        true
    }

    /// Shadow any prior native-instance tag for `local_name` by pushing a
    /// tombstone (empty module). `native_instances` is module-global and
    /// last-match-wins, so without this a fresh binding of a name that an
    /// unrelated `new FormData()`/`new Response()`/etc. earlier registered
    /// (e.g. a minified bundle reusing the local `i`) would inherit the stale
    /// native tag — routing a plain `i.exports` read through FormData's native
    /// method dispatch (→ 0) instead of an ordinary property read. A real
    /// native binding re-registers AFTER this tombstone, so last-match-wins
    /// keeps the correct tag. (Next.js app-page-turbo `require` fix.)
    pub(crate) fn shadow_native_instance(&mut self, local_name: String) {
        self.native_instances
            .push((local_name, String::new(), String::new()));
    }

    /// Tombstone a stale native-instance tag for `name` ONLY if one is currently
    /// live, so a fresh binding (var-decl OR function parameter) of that name
    /// shadows it. A function PARAMETER named the same as a leaked native
    /// instance (e.g. a minified `function(e){…}` whose `e` collides with an
    /// earlier `e = new Response()` in another factory) must NOT route
    /// `e.<method>` through the stale native dispatch — that folds named reads to
    /// 0 (the same class as the Fragment `i.exports` wall, but for params: in
    /// the Next.js app-page bundle superstruct's `enums(e){ e.map(…).join() }`
    /// saw `e.map`/`e.length`/`e.constructor` all read 0 while `e[0]` and
    /// `Array.prototype.map.call(e)` worked → `(number).join is not a function`).
    pub(crate) fn shadow_native_instance_if_present(&mut self, name: &str) {
        // A pre-scan registered this exact param name as a native instance for
        // the callback now being lowered (e.g. `wsId` for `server.on('upgrade',
        // (req, wsId, head) => …)`). That tag is the FRESH, intended one — the
        // param IS the native instance — so its own param binding must NOT
        // tombstone it. One-shot: consume the protection so a later, unrelated
        // param of the same name still shadows a genuinely stale tag.
        // Consume only when this binding is at the depth the pre-scan anchored
        // the protection to (the callback's own param scope). A same-named
        // binding at any other depth must NOT consume it; `exit_scope` drops a
        // never-consumed entry when the callback scope unwinds.
        if self.prescan_protected_native_params.get(name).copied() == Some(self.scope_depth) {
            self.prescan_protected_native_params.remove(name);
            return;
        }
        if self.lookup_native_instance(name).is_some() {
            self.shadow_native_instance(name.to_string());
        }
    }

    /// Mark `name` as a pre-scan-designated native-instance param so the very
    /// next `shadow_native_instance_if_present(name)` (the callback's own param
    /// binding) skips tombstoning it. The pre-scan runs in the CALLER's scope,
    /// one level above the callback, so anchor to `scope_depth + 1` (the
    /// callback's param scope): the consume matches at exactly that depth, and a
    /// never-consumed entry is dropped when the callback scope exits — it can't
    /// linger into the caller scope and shadow a later, unrelated same-named
    /// binding. See `prescan_protected_native_params`.
    pub(crate) fn protect_native_param(&mut self, name: String) {
        self.prescan_protected_native_params
            .insert(name, self.scope_depth + 1);
    }

    /// Truncate `native_instances` back to `mark`, keeping the
    /// `native_instances_index` shadow stacks in sync: every recorded index
    /// `>= mark` is popped (these belong to bindings whose scope is exiting),
    /// re-exposing any earlier (outer-scope) binding of the same name. Empty
    /// stacks are removed to keep the map small. Use this everywhere
    /// `native_instances.truncate(..)` was previously called directly.
    pub(crate) fn truncate_native_instances(&mut self, mark: usize) {
        if self.native_instances.len() <= mark {
            return;
        }
        self.native_instances.truncate(mark);
        // Drop indices >= mark from each name's shadow stack, re-exposing any
        // earlier (outer-scope) binding. The map is keyed by distinct
        // native-instance names (bounded, not proportional to class count), so
        // this stays cheap.
        self.native_instances_index.retain(|_, stack| {
            while stack.last().is_some_and(|&i| i >= mark) {
                stack.pop();
            }
            !stack.is_empty()
        });
    }

    /// #1483: resolve a parameter's declared type name to a perry/ui widget
    /// class that uses handle-based instance dispatch (Canvas, State, ...).
    /// Returns the canonical widget name (e.g. "Canvas") when `type_name`
    /// refers to a perry/ui widget — whether via its value-import name
    /// (`canvas: Canvas`) or a type-only import alias (`type Canvas as
    /// CanvasType` → `canvas: CanvasType`). Returns `None` otherwise, so a
    /// user class that merely shares a name with a widget isn't mis-tagged
    /// (resolution requires an actual perry/ui import).
    pub(crate) fn resolve_perry_ui_widget_type(&self, type_name: &str) -> Option<String> {
        // Value import: `import { Canvas } from "perry/ui"`.
        if let Some(("perry/ui", Some(widget))) = self.lookup_native_module(type_name) {
            if perry_ui_handle_widget(widget) {
                return Some(widget.to_string());
            }
        }
        // Type-only import, possibly aliased: `import { type Canvas as CanvasType }`.
        self.ui_widget_type_aliases.get(type_name).cloned()
    }

    pub(crate) fn lookup_native_instance(&self, name: &str) -> Option<(&str, &str)> {
        fn exposes_plain_object_fields(module: &str, class: &str) -> bool {
            // `node:module`'s CommonJS Module constructor returns an ordinary
            // heap object with data fields (`id`, `path`, `exports`, ...).
            // Rewriting those reads as native receiver methods makes them
            // miss the object's actual properties.
            matches!((module, class), ("module", "Module") | ("repl", _))
        }

        // Tombstone shadowing (see `shadow_native_instance`): if the most
        // recent `native_instances` entry for `name` is a tombstone (empty
        // module), this binding deliberately shadows any older native tag of
        // the same name — resolve to no native instance so the read/call
        // lowers as an ordinary property access.
        if let Some((_, module, _)) = self
            .native_instances
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
        {
            if module.is_empty() {
                return None;
            }
        }

        // Issue #1132 — walk the scoped instances back-to-front so a
        // later (inner-scope) registration shadows an earlier
        // (outer-scope) one with the same name. `native_instances` is
        // a push-only Vec ordered by registration; an inner arrow
        // callback that re-binds a name already tagged by an outer
        // callback (the classic `createServer((req, res) => httpGet(…,
        // (res) => …))` shape) pushes its tag AFTER the outer one, so
        // last-match-wins is the correct lexical-shadowing direction.
        // (Pre-fix this was `.iter().find()` — first-match — so the
        // inner `res` always resolved to the outer `("http",
        // "ServerResponse")` tag and `res.on('data')` misrouted
        // through ServerResponse dispatch instead of IncomingMessage.)
        //
        // Indexed (#5271): `native_instances_index[name]` is the shadow stack
        // of indices for this name, innermost (last) on top — so the top index
        // is exactly the entry the old `.rev().find()` would have selected.
        // The `exposes_plain_object_fields` filter is then applied to THAT
        // entry only (matching `.find().filter()`, which never falls through to
        // an earlier match when the top one is filtered out).
        self.native_instances_index
            .get(name)
            .and_then(|stack| stack.last())
            .map(|&idx| &self.native_instances[idx])
            // `node:repl` constructors allocate real heap objects/errors with
            // bound methods; routing them through handle-dispatch native
            // getters turns ordinary fields like `Recoverable.err` into
            // zero-arg FFI calls.
            .filter(|(_, module, class)| !exposes_plain_object_fields(module, class))
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
            .or_else(|| {
                // Check module-level instances (survive scope exits).
                // Same last-match-wins rule for consistency — the index stores
                // the LAST pushed entry per name.
                self.module_native_instances_index
                    .get(name)
                    .map(|&idx| &self.module_native_instances[idx])
                    .filter(|(_, module, class)| !exposes_plain_object_fields(module, class))
                    .map(|(_, module, class)| (module.as_str(), class.as_str()))
            })
    }

    pub(crate) fn lookup_func_return_native_instance(
        &self,
        func_name: &str,
    ) -> Option<(&str, &str)> {
        // Forward-scan (first-match-wins): index keeps the FIRST pushed entry.
        self.func_return_native_instances_index
            .get(func_name)
            .map(|&idx| {
                let (_, module, class) = &self.func_return_native_instances[idx];
                (module.as_str(), class.as_str())
            })
    }

    /// Push a function-return native instance (push-only, never truncated) and
    /// update its perf index. `lookup_func_return_native_instance` scanned
    /// FORWARD (first-match-wins), so the index keeps the FIRST pushed entry.
    pub(crate) fn push_func_return_native_instance(&mut self, entry: (String, String, String)) {
        let idx = self.func_return_native_instances.len();
        self.func_return_native_instances_index
            .entry(entry.0.clone())
            .or_insert(idx);
        self.func_return_native_instances.push(entry);
    }

    /// Push a module-level native instance (module-scoped, never truncated)
    /// and update its perf index. `lookup_native_instance`'s fallback arm
    /// scans these in reverse (last-match-wins), so the index stores the LAST
    /// pushed entry per name (overwrite).
    pub(crate) fn push_module_native_instance(&mut self, entry: (String, String, String)) {
        let idx = self.module_native_instances.len();
        self.module_native_instances_index
            .insert(entry.0.clone(), idx);
        self.module_native_instances.push(entry);
    }
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
        // #6062: a `typeof <name>` inside a nested closure is runtime-timing-
        // dependent (the closure may run before OR after the outer lexical is
        // initialized), so the enclosing block's forward-lexical set must not
        // statically force a throw inside it. Save and clear across the
        // boundary; the nested body repopulates its own via `lower_stmts_using_aware`.
        self.forward_lexical_saves
            .push(std::mem::take(&mut self.forward_lexical_names));
        let local_mark = self.locals.len();
        self.scope_depth += 1;
        self.scope_local_marks.push(local_mark);
        // #wall5: parallel mark for the native-module shadow stack, restored in
        // exit_scope (kept off the returned tuple to avoid churning its callers).
        self.scope_module_shadow_marks
            .push(self.module_shadow_stack.len());
        (
            local_mark,
            self.native_instances.len(),
            self.functions.len(),
        )
    }

    pub(crate) fn exit_scope(&mut self, mark: (usize, usize, usize)) {
        debug_assert!(self.scope_depth > 0, "exit_scope called at module depth");
        // #6062: restore the enclosing block's forward-lexical set saved in the
        // matching `enter_scope`.
        self.forward_lexical_names = self.forward_lexical_saves.pop().unwrap_or_default();
        self.scope_depth = self.scope_depth.saturating_sub(1);
        // Drop any pre-scan param protections registered in a scope deeper than
        // the one we just returned to — their expected consumer (the callback's
        // param binding) never fired, so they must not linger and skip a later
        // same-named binding's tombstone.
        self.prescan_protected_native_params
            .retain(|_, depth| *depth <= self.scope_depth);
        self.scope_local_marks.pop();
        // #wall5: restore native-module shadowing for this scope.
        if let Some(m) = self.scope_module_shadow_marks.pop() {
            self.module_shadow_stack.truncate(m);
        }
        if self.locals.len() > mark.0 {
            let mut kept: Vec<(String, LocalId, Type)> = Vec::new();
            for entry in self.locals.drain_from(mark.0) {
                if self.sloppy_implicit_global_ids.contains(&entry.1) {
                    kept.push(entry);
                }
            }
            self.locals.extend(kept);
        }
        self.truncate_native_instances(mark.1);
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
        // Sloppy implicit globals (`undeclared = v` inside the block) are
        // module-scoped bindings too — keep them visible after the block.
        // Nested-scope forward-captured `let`/`const` pre-registrations are in
        // `var_hoisted_ids` only for box-at-entry allocation; they are lexical
        // bindings, so their block-entry rebinding must die with the block.
        if self.locals.len() > locals_mark {
            let mut kept: Vec<(String, LocalId, Type)> = Vec::new();
            for entry in self.locals.drain_from(locals_mark) {
                if (self.var_hoisted_ids.contains(&entry.1)
                    && !self.nested_forward_scope_ids.contains(&entry.1))
                    || self.sloppy_implicit_global_ids.contains(&entry.1)
                {
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

/// perry/ui named imports that return an opaque widget handle and dispatch
/// instance methods through `NativeMethodCall` (handle-based dispatch). The
/// set mirrors the local-init registration in `module_decl.rs`; keep the two
/// in sync. Used to tag widget-typed function parameters (#1483).
pub(crate) fn perry_ui_handle_widget(name: &str) -> bool {
    matches!(
        name,
        "Canvas"
            | "State"
            | "Sheet"
            | "Toolbar"
            | "Window"
            | "LazyVStack"
            | "NavigationStack"
            | "Picker"
            | "Table"
            | "TabBar"
    )
}
