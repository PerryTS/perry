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
            ui_widget_type_aliases: HashMap::new(),
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
            suppress_stdlib_dispatch_guard_once: false,
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
        self.native_instances
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, module, class)| (module.as_str(), class.as_str()))
            .or_else(|| {
                // Check module-level instances (survive scope exits).
                // Same last-match-wins rule for consistency.
                self.module_native_instances
                    .iter()
                    .rev()
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
