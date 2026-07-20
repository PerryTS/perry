//! Best-effort HIR value-type inference.
//!
//! This is intentionally an analysis-side spine rather than an `Expr` layout
//! change. The current compiler already carries type facts in several places
//! (AST lowering, post-lowering widening, and codegen collectors). Providing a
//! reusable HIR-level API lets later work migrate those consumers gradually
//! before committing to storing result types on every expression node.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use perry_types::{FuncId, GlobalId, LocalId, ObjectType, PropertyInfo, Type};

use crate::ir::*;
use crate::walker::walk_expr_children;

/// Type facts known outside a single expression.
///
/// `HirTypeEnv` is deliberately small and conservative: it records declared
/// local/global types, function returns, and a few module-level class/enum
/// facts, then [`infer_expr_type`] combines those facts with expression-local
/// information. Missing entries always collapse to `Type::Any` rather than
/// inventing precision.
#[derive(Debug, Clone, Default)]
pub struct HirTypeEnv {
    locals: HashMap<LocalId, Type>,
    globals: HashMap<GlobalId, Type>,
    function_returns: HashMap<FuncId, Type>,
    enum_members: HashMap<(String, String), Type>,
    type_extends: HashMap<String, Vec<String>>,
    named_properties: HashMap<(String, String), Type>,
    static_fields: HashMap<(String, String), Type>,
    static_method_returns: HashMap<(String, String), Type>,
    /// Name of the class whose body is currently being inferred, if any. Drives
    /// `this`/`super` lookups so member reads inside class methods resolve to
    /// real types instead of `Any`.
    current_class: Option<String>,
}

/// Read-only type facts needed by [`infer_expr_type`].
///
/// Keeping inference generic over this trait lets compiler phases reuse the
/// HIR type-analysis spine with their existing context maps instead of copying
/// declarations into a temporary [`HirTypeEnv`].
pub trait HirTypeFacts {
    /// Return the declared/static type for a local, if known.
    fn local_type(&self, id: LocalId) -> Option<&Type>;

    /// Return the declared/static type for a global, if known.
    fn global_type(&self, id: GlobalId) -> Option<&Type>;

    /// Return the declared/static return type for a function, if known.
    fn function_return_type(&self, id: FuncId) -> Option<&Type>;

    /// Return the declared/static return type for an external function, if known.
    ///
    /// This hook lets cross-module consumers feed imported return facts into
    /// HIR inference without materializing a complete [`HirTypeEnv`]. `Any`
    /// and `Unknown` are treated as non-authoritative by external-function
    /// inference so sparse cross-module facts do not erase a more precise
    /// return type already embedded in the HIR node.
    fn extern_function_return_type(&self, _name: &str) -> Option<&Type> {
        None
    }

    /// Return the type of the current `this` binding, when the caller has
    /// enough lexical context to know it.
    fn this_type(&self) -> Option<Type> {
        None
    }

    /// Return the value type of a concrete enum member.
    fn enum_member_type(&self, _enum_name: &str, _member_name: &str) -> Option<Type> {
        None
    }

    /// Return the declared type for a static class field.
    fn static_field_type(&self, _class_name: &str, _field_name: &str) -> Option<&Type> {
        None
    }

    /// Return the declared return type for a static class method.
    fn static_method_return_type(&self, _class_name: &str, _method_name: &str) -> Option<&Type> {
        None
    }

    /// Return the value type exposed by a named class/interface property.
    ///
    /// This includes declared fields, getters, and method values. It is
    /// intentionally a value-level lookup rather than a layout lookup: callers
    /// use it to infer `obj.field` and `obj.method()` shapes when `obj` is
    /// already known as `Named("ClassOrInterface")`.
    fn named_property_type(&self, _type_name: &str, _property: &str) -> Option<Type> {
        None
    }

    /// Return the value type for `super.property` in the current class context.
    ///
    /// This is intentionally separate from [`HirTypeFacts::named_property_type`]:
    /// `super.x` reads from the parent prototype/accessor chain, not from
    /// instance fields declared on the parent class.
    fn super_property_type(&self, _property: &str) -> Option<Type> {
        None
    }

    /// Return the declared return type for `super.method(...)` in the current
    /// class context.
    fn super_method_return_type(&self, _method: &str) -> Option<Type> {
        None
    }
}

impl HirTypeFacts for HirTypeEnv {
    fn local_type(&self, id: LocalId) -> Option<&Type> {
        self.locals.get(&id)
    }

    fn global_type(&self, id: GlobalId) -> Option<&Type> {
        self.globals.get(&id)
    }

    fn function_return_type(&self, id: FuncId) -> Option<&Type> {
        self.function_returns.get(&id)
    }

    fn enum_member_type(&self, enum_name: &str, member_name: &str) -> Option<Type> {
        self.enum_members
            .get(&(enum_name.to_string(), member_name.to_string()))
            .cloned()
    }

    fn static_field_type(&self, class_name: &str, field_name: &str) -> Option<&Type> {
        self.lookup_member_type(
            &self.static_fields,
            class_name,
            field_name,
            &mut HashSet::new(),
        )
    }

    fn static_method_return_type(&self, class_name: &str, method_name: &str) -> Option<&Type> {
        self.lookup_member_type(
            &self.static_method_returns,
            class_name,
            method_name,
            &mut HashSet::new(),
        )
    }

    fn named_property_type(&self, type_name: &str, property: &str) -> Option<Type> {
        self.lookup_member_type(
            &self.named_properties,
            type_name,
            property,
            &mut HashSet::new(),
        )
        .cloned()
    }

    fn this_type(&self) -> Option<Type> {
        self.current_class.clone().map(Type::Named)
    }

    fn super_property_type(&self, property: &str) -> Option<Type> {
        // `super.x` reads from the parent chain, so resolve from the current
        // class's first parent (named_property_type itself walks further up).
        let parent = self
            .type_extends
            .get(self.current_class.as_deref()?)?
            .first()?;
        self.named_property_type(parent, property)
    }

    fn super_method_return_type(&self, method: &str) -> Option<Type> {
        match self.super_property_type(method)? {
            Type::Function(ft) => Some(*ft.return_type),
            _ => None,
        }
    }
}

impl HirTypeFacts for () {
    fn local_type(&self, _id: LocalId) -> Option<&Type> {
        None
    }

    fn global_type(&self, _id: GlobalId) -> Option<&Type> {
        None
    }

    fn function_return_type(&self, _id: FuncId) -> Option<&Type> {
        None
    }
}

impl<S: BuildHasher> HirTypeFacts for HashMap<LocalId, Type, S> {
    fn local_type(&self, id: LocalId) -> Option<&Type> {
        self.get(&id)
    }

    fn global_type(&self, _id: GlobalId) -> Option<&Type> {
        None
    }

    fn function_return_type(&self, _id: FuncId) -> Option<&Type> {
        None
    }
}

impl HirTypeEnv {
    /// Create an empty type environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a type environment from the declarations present in a HIR module.
    ///
    /// This walks function/class/closure bodies to pick up `Stmt::Let`
    /// declarations and closure return types. LocalIds are module-unique, so a
    /// single flat environment is enough for the best-effort consumers this API
    /// targets.
    pub fn from_module(module: &Module) -> Self {
        let mut env = Self::new();
        let class_names_by_id: HashMap<_, _> = module
            .classes
            .iter()
            .map(|class| (class.id, class.name.clone()))
            .collect();

        for global in &module.globals {
            env.globals.insert(global.id, global.ty.clone());
            if let Some(init) = &global.init {
                env.collect_expr_declarations(init);
            }
        }

        for enum_decl in &module.enums {
            for member in &enum_decl.members {
                env.enum_members.insert(
                    (enum_decl.name.clone(), member.name.clone()),
                    enum_member_value_type(&member.value),
                );
            }
        }

        for interface in &module.interfaces {
            let parents: Vec<_> = interface
                .extends
                .iter()
                .filter_map(named_type_base)
                .map(str::to_string)
                .collect();
            if !parents.is_empty() {
                env.type_extends.insert(interface.name.clone(), parents);
            }

            for property in &interface.properties {
                env.named_properties.insert(
                    (interface.name.clone(), property.name.clone()),
                    property.ty.clone(),
                );
            }
            for method in &interface.methods {
                env.named_properties.insert(
                    (interface.name.clone(), method.name.clone()),
                    interface_method_type(method),
                );
            }
        }

        for function in &module.functions {
            env.collect_function_declarations(function);
        }

        for class in &module.classes {
            let mut parents = Vec::new();
            if let Some(parent_name) = &class.extends_name {
                parents.push(parent_name.clone());
            } else if let Some(parent_id) = class.extends {
                if let Some(parent_name) = class_names_by_id.get(&parent_id) {
                    parents.push(parent_name.clone());
                }
            }
            if !parents.is_empty() {
                env.type_extends.insert(class.name.clone(), parents);
            }

            for field in &class.fields {
                env.named_properties
                    .insert((class.name.clone(), field.name.clone()), field.ty.clone());
                if let Some(key_expr) = &field.key_expr {
                    env.collect_expr_declarations(key_expr);
                }
                if let Some(init) = &field.init {
                    env.collect_expr_declarations(init);
                }
            }
            for field in &class.static_fields {
                env.static_fields
                    .insert((class.name.clone(), field.name.clone()), field.ty.clone());
                if let Some(key_expr) = &field.key_expr {
                    env.collect_expr_declarations(key_expr);
                }
                if let Some(init) = &field.init {
                    env.collect_expr_declarations(init);
                }
            }
            if let Some(ctor) = &class.constructor {
                env.collect_function_declarations(ctor);
            }
            for method in &class.methods {
                env.named_properties.insert(
                    (class.name.clone(), method.name.clone()),
                    function_type_from_decl(method),
                );
                env.collect_function_declarations(method);
            }
            for (property_name, getter) in &class.getters {
                if !class.static_accessor_fn_ids.contains(&getter.id) {
                    env.named_properties.insert(
                        (class.name.clone(), property_name.clone()),
                        getter.return_type.clone(),
                    );
                }
                env.collect_function_declarations(getter);
            }
            for (_, setter) in &class.setters {
                env.collect_function_declarations(setter);
            }
            for method in &class.static_methods {
                env.static_method_returns.insert(
                    (class.name.clone(), method.name.clone()),
                    method.return_type.clone(),
                );
                env.collect_function_declarations(method);
            }
            if let Some(extends_expr) = &class.extends_expr {
                env.collect_expr_declarations(extends_expr);
            }
        }

        env.collect_stmts_declarations(&module.init);
        env
    }

    /// Register or override a local type.
    pub fn insert_local(&mut self, id: LocalId, ty: Type) {
        self.locals.insert(id, ty);
    }

    /// Builder-style local registration for tests and staged callers.
    pub fn with_local(mut self, id: LocalId, ty: Type) -> Self {
        self.insert_local(id, ty);
        self
    }

    /// Set (or clear) the class context used for `this`/`super` lookups.
    /// Returns the previous value so callers can restore it after walking a
    /// class body.
    pub fn set_current_class(&mut self, name: Option<String>) -> Option<String> {
        std::mem::replace(&mut self.current_class, name)
    }

    /// Register or override a global type.
    pub fn insert_global(&mut self, id: GlobalId, ty: Type) {
        self.globals.insert(id, ty);
    }

    /// Builder-style global registration for tests and staged callers.
    pub fn with_global(mut self, id: GlobalId, ty: Type) -> Self {
        self.insert_global(id, ty);
        self
    }

    /// Register or override a function return type.
    pub fn insert_function_return(&mut self, id: FuncId, ty: Type) {
        self.function_returns.insert(id, ty);
    }

    /// Builder-style function return registration for tests and staged callers.
    pub fn with_function_return(mut self, id: FuncId, ty: Type) -> Self {
        self.insert_function_return(id, ty);
        self
    }

    pub fn local_type(&self, id: LocalId) -> Option<&Type> {
        self.locals.get(&id)
    }

    pub fn global_type(&self, id: GlobalId) -> Option<&Type> {
        self.globals.get(&id)
    }

    pub fn function_return_type(&self, id: FuncId) -> Option<&Type> {
        self.function_returns.get(&id)
    }

    fn lookup_member_type<'a>(
        &'a self,
        members: &'a HashMap<(String, String), Type>,
        type_name: &str,
        member_name: &str,
        visited: &mut HashSet<String>,
    ) -> Option<&'a Type> {
        let key = (type_name.to_string(), member_name.to_string());
        if let Some(ty) = members.get(&key) {
            return Some(ty);
        }
        if !visited.insert(type_name.to_string()) {
            return None;
        }
        for parent in self.type_extends.get(type_name).into_iter().flatten() {
            if let Some(ty) = self.lookup_member_type(members, parent, member_name, visited) {
                return Some(ty);
            }
        }
        None
    }

    fn collect_function_declarations(&mut self, function: &Function) {
        self.function_returns
            .insert(function.id, function.return_type.clone());
        for param in &function.params {
            self.locals.insert(param.id, param.ty.clone());
            if let Some(default) = &param.default {
                self.collect_expr_declarations(default);
            }
        }
        self.collect_stmts_declarations(&function.body);
    }

    fn collect_stmts_declarations(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_stmt_declarations(stmt);
        }
    }

    fn collect_stmt_declarations(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { id, ty, init, .. } => {
                self.locals.insert(*id, ty.clone());
                if let Some(init) = init {
                    self.collect_expr_declarations(init);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                self.collect_expr_declarations(expr);
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_expr_declarations(condition);
                self.collect_stmts_declarations(then_branch);
                if let Some(else_branch) = else_branch {
                    self.collect_stmts_declarations(else_branch);
                }
            }
            Stmt::While { condition, body } => {
                self.collect_expr_declarations(condition);
                self.collect_stmts_declarations(body);
            }
            Stmt::DoWhile { body, condition } => {
                self.collect_stmts_declarations(body);
                self.collect_expr_declarations(condition);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.collect_stmt_declarations(init);
                }
                if let Some(condition) = condition {
                    self.collect_expr_declarations(condition);
                }
                if let Some(update) = update {
                    self.collect_expr_declarations(update);
                }
                self.collect_stmts_declarations(body);
            }
            Stmt::Labeled { body, .. } => self.collect_stmt_declarations(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.collect_stmts_declarations(body);
                if let Some(catch) = catch {
                    self.collect_stmts_declarations(&catch.body);
                }
                if let Some(finally) = finally {
                    self.collect_stmts_declarations(finally);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.collect_expr_declarations(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.collect_expr_declarations(test);
                    }
                    self.collect_stmts_declarations(&case.body);
                }
            }
        }
    }

    fn collect_expr_declarations(&mut self, expr: &Expr) {
        if let Expr::Closure {
            func_id,
            params,
            return_type,
            body,
            ..
        } = expr
        {
            self.function_returns.insert(*func_id, return_type.clone());
            for param in params {
                self.locals.insert(param.id, param.ty.clone());
                if let Some(default) = &param.default {
                    self.collect_expr_declarations(default);
                }
            }
            self.collect_stmts_declarations(body);
            return;
        }

        walk_expr_children(expr, &mut |child| self.collect_expr_declarations(child));
    }
}

/// Infer the runtime value type produced by a HIR expression.
///
/// The result is conservative: when this cannot prove a narrower type, it
/// returns `Type::Any`. That keeps it safe for staged consumers such as
/// codegen fast-path selection, local widening, and future Expr annotations.
pub fn infer_expr_type<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    match expr {
        Expr::Undefined | Expr::Void(_) => Type::Void,
        Expr::Null => Type::Null,
        Expr::Bool(_) => Type::Boolean,
        Expr::Number(_) | Expr::Integer(_) => Type::Number,
        Expr::BigInt(_) => Type::BigInt,
        Expr::String(_) | Expr::WtfString(_) | Expr::I18nString { .. } => Type::String,
        Expr::SymbolNew(_) | Expr::SymbolFor(_) => Type::Symbol,

        Expr::LocalGet(id) => env.local_type(*id).cloned().unwrap_or(Type::Any),
        Expr::This => env.this_type().unwrap_or(Type::Any),
        Expr::EnumMember {
            enum_name,
            member_name,
        } => env
            .enum_member_type(enum_name, member_name)
            .unwrap_or(Type::Any),
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => env
            .static_field_type(class_name, field_name)
            .cloned()
            .unwrap_or(Type::Any),
        Expr::WithGet {
            object,
            property,
            fallback,
        } => same_type_or_any(
            infer_property_get_type(object, property, env),
            infer_expr_type(fallback, env),
        ),
        Expr::LocalSet(_, value)
        | Expr::GlobalSet(_, value)
        | Expr::WithSet { value, .. }
        | Expr::StaticFieldSet { value, .. }
        | Expr::ClassStaticSymbolSet { value, .. }
        | Expr::SuperPropertySet { value, .. }
        | Expr::ObjectSuperPropertySet { value, .. }
        | Expr::RegisterPrototypeMethod { value, .. }
        | Expr::RegisterFunctionPrototypeMethod { value, .. }
        | Expr::BufferIndexSet { value, .. }
        | Expr::Uint8ArraySet { value, .. }
        | Expr::ProxySet { value, .. }
        | Expr::PutValueSet { value, .. }
        | Expr::UrlSetPathname { value, .. }
        | Expr::UrlSetSearch { value, .. }
        | Expr::UrlSetHash { value, .. }
        | Expr::UrlSetProtocol { value, .. }
        | Expr::UrlSetHostname { value, .. }
        | Expr::UrlSetPort { value, .. }
        | Expr::UrlSetUsername { value, .. }
        | Expr::UrlSetPassword { value, .. }
        | Expr::UrlSetHref { value, .. } => infer_expr_type(value, env),
        Expr::GlobalGet(id) => env.global_type(*id).cloned().unwrap_or(Type::Any),

        Expr::Update { .. } => Type::Number,
        Expr::Unary { op, operand } => match op {
            UnaryOp::Not => Type::Boolean,
            UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => {
                let operand_ty = infer_expr_type(operand, env);
                if matches!(operand_ty, Type::BigInt) {
                    Type::BigInt
                } else {
                    Type::Number
                }
            }
        },
        Expr::Binary { op, left, right } => infer_binary_type(*op, left, right, env),
        Expr::Compare { .. }
        | Expr::InstanceOf { .. }
        | Expr::In { .. }
        | Expr::PrivateBrandCheck { .. } => Type::Boolean,
        Expr::PrivateGuard { object, .. } => infer_expr_type(object, env),
        Expr::Logical { op, left, right } => infer_logical_type(*op, left, right, env),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => same_type_or_any(
            infer_expr_type(then_expr, env),
            infer_expr_type(else_expr, env),
        ),
        Expr::TypeOf(_) => Type::String,
        Expr::Await(value) => match infer_expr_type(value, env) {
            Type::Promise(inner) => *inner,
            other => other,
        },
        Expr::Sequence(exprs) => exprs
            .last()
            .map(|expr| infer_expr_type(expr, env))
            .unwrap_or(Type::Any),
        Expr::StructuredClone { value, .. } => infer_expr_type(value, env),

        Expr::FuncRef(id) => function_type_for_return(env.function_return_type(*id)),
        Expr::ClassRef(_) | Expr::CurrentStepClosure => function_type_for_return(None),
        Expr::NewTarget => optional_type(function_type_for_return(None)),
        Expr::ExternFuncRef {
            name,
            param_types,
            return_type,
            ..
        } => {
            let effective_return_type = env
                .extern_function_return_type(name)
                .filter(|ty| !matches!(ty, Type::Any | Type::Unknown))
                .unwrap_or(return_type);
            Type::Function(perry_types::FunctionType {
                params: param_types
                    .iter()
                    .enumerate()
                    .map(|(idx, ty)| (format!("arg{idx}"), ty.clone(), false))
                    .collect(),
                return_type: Box::new(effective_return_type.clone()),
                is_async: false,
                is_generator: false,
            })
        }
        Expr::Closure {
            params,
            return_type,
            is_async,
            is_generator,
            ..
        } => Type::Function(perry_types::FunctionType {
            params: params
                .iter()
                .map(|param| (param.name.clone(), param.ty.clone(), false))
                .collect(),
            return_type: Box::new(return_type.clone()),
            is_async: *is_async,
            is_generator: *is_generator,
        }),
        Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } => {
            match infer_expr_type(callee, env) {
                Type::Function(function) => *function.return_type,
                _ => Type::Any,
            }
        }
        Expr::ReflectApply { func, .. } | Expr::ProxyApply { proxy: func, .. } => {
            function_return_type_from_expr(func, env)
        }
        Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } => env
            .static_method_return_type(class_name, method_name)
            .cloned()
            .unwrap_or(Type::Any),
        Expr::SuperCall(_) | Expr::SuperCallSpread(_) => Type::Void,
        Expr::SuperMethodCall { method, .. } => {
            env.super_method_return_type(method).unwrap_or(Type::Any)
        }
        Expr::SuperPropertyGet { property } => {
            env.super_property_type(property).unwrap_or(Type::Any)
        }

        Expr::PropertyGet {
            object, property, ..
        } => infer_property_get_type(object, property, env),
        Expr::PropertySet { value, .. } => infer_expr_type(value, env),
        Expr::PropertyUpdate { .. } => Type::Number,
        Expr::IndexGet { object, .. } => match infer_expr_type(object, env) {
            Type::Array(elem) => *elem,
            Type::Tuple(elems) => {
                if elems.len() == 1 {
                    elems.into_iter().next().unwrap_or(Type::Any)
                } else {
                    Type::Any
                }
            }
            Type::String => Type::String,
            _ => Type::Any,
        },
        Expr::IndexSet { value, .. } => infer_expr_type(value, env),
        Expr::IndexUpdate { .. } => Type::Number,

        Expr::Object(entries) => Type::Object(object_type_from_entries(entries, env)),
        Expr::ObjectSpread { .. } | Expr::ObjectAssign { .. } => Type::Object(ObjectType {
            properties: HashMap::new(),
            index_signature: Some(Box::new(Type::Any)),
            ..ObjectType::default()
        }),
        Expr::Array(items) => Type::Array(Box::new(unify_expr_types(items.iter(), env))),
        Expr::ArraySpread(items) => {
            let mut elem_tys = Vec::new();
            for item in items {
                match item {
                    ArrayElement::Expr(expr) => elem_tys.push(infer_expr_type(expr, env)),
                    ArrayElement::Hole | ArrayElement::Spread(_) => elem_tys.push(Type::Any),
                }
            }
            Type::Array(Box::new(unify_types(elem_tys)))
        }
        Expr::ArraySlice { array, .. }
        | Expr::ArrayFilter { array, .. }
        | Expr::ArraySort { array, .. }
        | Expr::ArrayToReversed { array }
        | Expr::ArrayToSorted { array, .. }
        | Expr::ArrayToSpliced { array, .. } => array_type_from_expr(array, env),
        Expr::ArraySplice { array_id, .. } | Expr::ArrayCopyWithin { array_id, .. } => {
            array_type_from_local(*array_id, env)
        }
        Expr::ArrayWith { array, value, .. } => Type::Array(Box::new(unify_types([
            array_element_type_from_expr(array, env),
            infer_expr_type(value, env),
        ]))),
        Expr::ArrayFrom(iterable) | Expr::ArrayFromArrayLikeHoley(iterable) => {
            array_type_from_iterable_expr(iterable, env)
        }
        Expr::ArrayMap { callback, .. }
        | Expr::ArrayFromMapped {
            map_fn: callback, ..
        } => Type::Array(Box::new(function_return_type_from_expr(callback, env))),
        Expr::ArrayFlatMap { callback, .. } => Type::Array(Box::new(flattened_array_element_type(
            function_return_type_from_expr(callback, env),
        ))),
        Expr::ArrayFlat { array } => Type::Array(Box::new(flattened_array_element_type(
            array_element_type_from_expr(array, env),
        ))),
        Expr::IteratorToArray(_) | Expr::ForOfToArray(_) | Expr::ForAwaitToArray(_) => {
            Type::Array(Box::new(Type::Any))
        }
        Expr::TaggedTemplateStrings { .. } => Type::Array(Box::new(Type::String)),
        Expr::TemplateRaw(_) => optional_type(Type::Array(Box::new(Type::String))),
        Expr::ArrayReverseValue { receiver } | Expr::ArrayCopyWithinValue { receiver, .. } => {
            infer_expr_type(receiver, env)
        }
        Expr::ArrayEntries(array) => Type::Array(Box::new(Type::Tuple(vec![
            Type::Number,
            array_element_type_from_expr(array, env),
        ]))),
        Expr::ArrayKeys(_) => Type::Array(Box::new(Type::Number)),
        Expr::ArrayValues(array) => array_type_from_expr(array, env),
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            optional_type(array_element_type_from_local(*array_id, env))
        }
        Expr::ArrayAt { array, .. }
        | Expr::ArrayFind { array, .. }
        | Expr::ArrayFindLast { array, .. } => {
            optional_type(array_element_type_from_expr(array, env))
        }
        Expr::ArrayReduce { callback, .. } | Expr::ArrayReduceRight { callback, .. } => {
            function_return_type_from_expr(callback, env)
        }
        Expr::ArrayJoin { .. } => Type::String,
        Expr::ArrayIsArray(_)
        | Expr::ArrayIncludes { .. }
        | Expr::ArraySome { .. }
        | Expr::ArrayEvery { .. } => Type::Boolean,
        Expr::ArrayPush { .. }
        | Expr::ArrayPushSpread { .. }
        | Expr::ArrayUnshift { .. }
        | Expr::ArrayIndexOf { .. }
        | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayFindIndex { .. }
        | Expr::ArrayFindLastIndex { .. } => Type::Number,
        Expr::ArrayForEach { .. } => Type::Void,
        Expr::ArrayLikeMethod {
            method,
            receiver,
            args,
        } => infer_arraylike_method_type(method, receiver, args, env),

        Expr::BigIntCoerce(_) => Type::BigInt,
        Expr::NumberCoerce(_) => Type::Number,
        Expr::StringCoerce(_) => Type::String,
        Expr::BooleanCoerce(_) => Type::Boolean,

        Expr::New { class_name, .. } if class_name == "Array" => Type::Array(Box::new(Type::Any)),
        Expr::New { class_name, .. } => Type::Named(class_name.clone()),
        Expr::NewDynamic { .. } | Expr::NewDynamicSpread { .. } => {
            Type::Object(ObjectType::default())
        }
        Expr::ClassExprFresh { .. } => Type::Object(ObjectType::default()),
        Expr::SetFunctionPrototype { proto, .. } => infer_expr_type(proto, env),
        Expr::GetFunctionPrototypeMethod { .. } => optional_type(function_type_for_return(None)),
        Expr::MapNew => generic_type("Map"),
        Expr::MapNewFromArray(entries) => map_type_from_entries_expr(entries, env),
        Expr::MapGroupBy { items, key_fn } => map_type(
            function_return_type_from_expr(key_fn, env),
            Type::Array(Box::new(array_element_type_from_expr(items, env))),
        ),
        Expr::MapSet { map, .. } => typed_collection_or_default(map, "Map", env),
        Expr::MapGet { map, .. } => generic_type_arg_from_expr(map, "Map", 1, env),
        Expr::MapHas { .. } | Expr::MapDelete { .. } => Type::Boolean,
        Expr::MapSize(_) => Type::Number,
        Expr::MapClear(_) => Type::Void,
        Expr::MapEntries(map) => Type::Array(Box::new(Type::Tuple(vec![
            generic_type_arg_from_expr(map, "Map", 0, env),
            generic_type_arg_from_expr(map, "Map", 1, env),
        ]))),
        Expr::MapKeys(map) => Type::Array(Box::new(generic_type_arg_from_expr(map, "Map", 0, env))),
        Expr::MapValues(map) => {
            Type::Array(Box::new(generic_type_arg_from_expr(map, "Map", 1, env)))
        }
        Expr::MapEntryKeyAt { map, .. } => generic_type_arg_from_expr(map, "Map", 0, env),
        Expr::MapEntryValueAt { map, .. } => generic_type_arg_from_expr(map, "Map", 1, env),
        Expr::SetNew => generic_type("Set"),
        Expr::SetNewFromArray(values) => set_type(array_element_type_from_expr(values, env)),
        Expr::SetAdd { set_id, .. } => env
            .local_type(*set_id)
            .cloned()
            .filter(|ty| !matches!(ty, Type::Any | Type::Unknown))
            .unwrap_or_else(|| generic_type("Set")),
        Expr::SetHas { .. } | Expr::SetDelete { .. } => Type::Boolean,
        Expr::SetSize(_) => Type::Number,
        Expr::SetClear(_) => Type::Void,
        Expr::SetValues(set) => {
            Type::Array(Box::new(generic_type_arg_from_expr(set, "Set", 0, env)))
        }
        Expr::SetValueAt { set, .. } => generic_type_arg_from_expr(set, "Set", 0, env),
        Expr::IteratorFrom(_) | Expr::GetIterator(_) | Expr::GetAsyncIterator(_) => {
            Type::Object(ObjectType::default())
        }
        Expr::RegExp { .. } => Type::Named("RegExp".to_string()),
        Expr::RegExpDynamic { .. } => Type::Named("RegExp".to_string()),
        Expr::DateNew(_) => Type::Named("Date".to_string()),
        Expr::ErrorNew(_) => Type::Named("Error".to_string()),
        Expr::ErrorNewWithCause { .. } | Expr::ErrorNewWithOptions { .. } => {
            Type::Named("Error".to_string())
        }
        Expr::TypeErrorNew(_) => Type::Named("TypeError".to_string()),
        Expr::RangeErrorNew(_) => Type::Named("RangeError".to_string()),
        Expr::ReferenceErrorNew(_) => Type::Named("ReferenceError".to_string()),
        Expr::SyntaxErrorNew(_) => Type::Named("SyntaxError".to_string()),
        Expr::AggregateErrorNew { .. } => Type::Named("AggregateError".to_string()),
        Expr::UrlNew { .. } => Type::Named("URL".to_string()),
        Expr::UrlPatternNew { .. } => Type::Named("URLPattern".to_string()),
        Expr::UrlSearchParamsNew(_) => Type::Named("URLSearchParams".to_string()),
        Expr::TextEncoderNew => Type::Named("TextEncoder".to_string()),
        Expr::TextDecoderNew { .. } => Type::Named("TextDecoder".to_string()),
        Expr::UrlGetSearchParams(_) => Type::Named("URLSearchParams".to_string()),
        Expr::WeakRefNew(_) => Type::Named("WeakRef".to_string()),
        Expr::WeakRefDeref(_) => optional_type(Type::Object(ObjectType::default())),
        Expr::FinalizationRegistryNew(_) => Type::Named("FinalizationRegistry".to_string()),
        Expr::BoxedPrimitiveNew { kind, .. } => Type::Named(
            match kind {
                BoxedPrimitiveKind::Number => "Number",
                BoxedPrimitiveKind::String => "String",
                BoxedPrimitiveKind::Boolean => "Boolean",
            }
            .to_string(),
        ),
        Expr::WebAssemblyModuleNew(_) => Type::Named("WebAssembly.Module".to_string()),
        Expr::ChildProcessSpawn { .. }
        | Expr::ChildProcessFork { .. }
        | Expr::ChildProcessExec { .. }
        | Expr::ChildProcessExecFile { .. } => Type::Named("ChildProcess".to_string()),
        Expr::NetCreateServer { .. } => Type::Named("Server".to_string()),
        Expr::NetCreateConnection { .. } | Expr::NetConnect { .. } => {
            Type::Named("Socket".to_string())
        }
        Expr::WorkerNew { .. } => Type::Named("Worker".to_string()),
        Expr::Uint8ArrayNew(_)
        | Expr::Uint8ArrayFrom(_)
        | Expr::BufferAlloc { .. }
        | Expr::BufferAllocUnsafe(_)
        | Expr::BufferFrom { .. }
        | Expr::BufferFromArrayBuffer { .. }
        | Expr::BufferConcat(_)
        | Expr::BufferConcatWithLength { .. }
        | Expr::BufferSlice { .. }
        | Expr::BufferFill { .. }
        | Expr::FsReadFileBinary(_)
        | Expr::TextEncoderEncode(_)
        | Expr::CryptoRandomBytes(_) => Type::Named("Uint8Array".to_string()),
        Expr::ChildProcessExecSync { .. } | Expr::ChildProcessExecFileSync { .. } => {
            Type::Union(vec![Type::Named("Uint8Array".to_string()), Type::String])
        }
        Expr::TypedArrayNew { kind, .. } => typed_array_kind_name(*kind)
            .map(|name| Type::Named(name.to_string()))
            .unwrap_or(Type::Any),
        Expr::NativeArenaAlloc(_) => Type::Named("NativeArenaOwner".to_string()),
        Expr::NativeArenaView { kind, .. } => typed_array_kind_name(*kind)
            .map(|name| Type::Named(name.to_string()))
            .unwrap_or(Type::Any),
        Expr::NativePodView { view_type, .. } => view_type.clone().unwrap_or(Type::Any),
        Expr::CryptoRandomFillSync { buffer, .. } => infer_expr_type(buffer, env),

        Expr::ProcessPid
        | Expr::ProcessPpid
        | Expr::ProcessAvailableMemory
        | Expr::ProcessConstrainedMemory
        | Expr::ProcessUptime
        | Expr::ProcessUmask(_)
        | Expr::ProcessPosixCredential(_)
        | Expr::ProcessStdoutColumns
        | Expr::ProcessStdoutRows
        | Expr::PerformanceNow
        | Expr::DateNow
        | Expr::DateGetTime(_)
        | Expr::DateGetFullYear(_)
        | Expr::DateGetMonth(_)
        | Expr::DateGetDate(_)
        | Expr::DateGetDay(_)
        | Expr::DateGetHours(_)
        | Expr::DateGetMinutes(_)
        | Expr::DateGetSeconds(_)
        | Expr::DateGetMilliseconds(_)
        | Expr::DateParse(_)
        | Expr::DateUtc(_)
        | Expr::DateGetUtcDay(_)
        | Expr::DateGetUtcFullYear(_)
        | Expr::DateGetUtcMonth(_)
        | Expr::DateGetUtcDate(_)
        | Expr::DateGetUtcHours(_)
        | Expr::DateGetUtcMinutes(_)
        | Expr::DateGetUtcSeconds(_)
        | Expr::DateGetUtcMilliseconds(_)
        | Expr::DateSetUtcFullYear { .. }
        | Expr::DateSetUtcMonth { .. }
        | Expr::DateSetUtcDate { .. }
        | Expr::DateSetUtcHours { .. }
        | Expr::DateSetUtcMinutes { .. }
        | Expr::DateSetUtcSeconds { .. }
        | Expr::DateSetUtcMilliseconds { .. }
        | Expr::DateSetFullYear { .. }
        | Expr::DateSetMonth { .. }
        | Expr::DateSetDate { .. }
        | Expr::DateSetHours { .. }
        | Expr::DateSetMinutes { .. }
        | Expr::DateSetSeconds { .. }
        | Expr::DateSetMilliseconds { .. }
        | Expr::DateSetTime { .. }
        | Expr::DateValueOf(_)
        | Expr::DateGetTimezoneOffset(_)
        | Expr::MathRandom
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathAbs(_)
        | Expr::MathSqrt(_)
        | Expr::MathLog(_)
        | Expr::MathLog2(_)
        | Expr::MathLog10(_)
        | Expr::MathLog1p(_)
        | Expr::MathClz32(_)
        | Expr::MathSin(_)
        | Expr::MathCos(_)
        | Expr::MathTan(_)
        | Expr::MathAsin(_)
        | Expr::MathAcos(_)
        | Expr::MathAtan(_)
        | Expr::MathAtan2(_, _)
        | Expr::MathCbrt(_)
        | Expr::MathFround(_)
        | Expr::MathF16round(_)
        | Expr::MathExpm1(_)
        | Expr::MathHypot(_)
        | Expr::MathSinh(_)
        | Expr::MathCosh(_)
        | Expr::MathTanh(_)
        | Expr::MathAsinh(_)
        | Expr::MathAcosh(_)
        | Expr::MathAtanh(_)
        | Expr::MathExp(_)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathPow(_, _)
        | Expr::MathImul(_, _)
        | Expr::MathMinSpread(_)
        | Expr::MathMaxSpread(_)
        | Expr::WebAssemblyCallExport { .. }
        | Expr::ParseInt { .. }
        | Expr::ParseFloat(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::BufferByteLength { .. }
        | Expr::BufferLength(_)
        | Expr::BufferCopy { .. }
        | Expr::BufferWrite { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::Uint8ArrayLength(_)
        | Expr::Uint8ArrayGet { .. }
        | Expr::RegExpLastIndex(_) => Type::Number,

        Expr::ProcessStdinIsTTY
        | Expr::ProcessStdoutIsTTY
        | Expr::ProcessStderrIsTTY
        | Expr::TtyIsAtty(_)
        | Expr::FsExistsSync(_)
        | Expr::FsRmRecursive(_)
        | Expr::PathIsAbsolute(_)
        | Expr::PathMatchesGlob(_, _)
        | Expr::UrlCanParse(_)
        | Expr::UrlCanParseWithBase { .. }
        | Expr::ObjectIsFrozen(_)
        | Expr::ObjectIsSealed(_)
        | Expr::ObjectIsExtensible(_)
        | Expr::ObjectIs(_, _)
        | Expr::ObjectHasOwn(_, _)
        | Expr::JsonIsRawJson(_)
        | Expr::WebAssemblyValidate(_)
        | Expr::TextDecoderFatal(_)
        | Expr::TextDecoderIgnoreBom(_)
        | Expr::BufferIsBuffer(_)
        | Expr::BufferIsEncoding(_)
        | Expr::BufferEquals { .. }
        | Expr::FinalizationRegistryUnregister { .. }
        | Expr::IterResultGetDone
        | Expr::RegExpTest { .. }
        | Expr::Delete(_)
        | Expr::ProxyHas { .. }
        | Expr::ProxyDelete { .. }
        | Expr::ReflectSet { .. }
        | Expr::ReflectHas { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectDefineProperty { .. }
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ReflectIsExtensible(_)
        | Expr::ReflectPreventExtensions(_)
        | Expr::ReflectHasMetadata { .. }
        | Expr::ReflectHasOwnMetadata { .. }
        | Expr::ReflectDeleteMetadata { .. }
        | Expr::UrlSearchParamsHas { .. }
        | Expr::IsNaN(_)
        | Expr::IsUndefinedOrBareNan(_)
        | Expr::IsFinite(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::NumberIsSafeInteger(_) => Type::Boolean,

        Expr::ProcessCwd
        | Expr::ProcessVersion
        | Expr::ProcessTitle
        | Expr::PathSep
        | Expr::PathDelimiter
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathBasenameExt(_, _)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathRelative(_, _)
        | Expr::PathJoin(..)
        | Expr::PathNormalize(_)
        | Expr::PathFormat(_)
        | Expr::PathResolveJoin(_, _)
        | Expr::PathWin32Join(_, _)
        | Expr::PathWin32 {
            method:
                PathWin32Method::Dirname
                | PathWin32Method::Basename
                | PathWin32Method::BasenameExt
                | PathWin32Method::Extname
                | PathWin32Method::Normalize
                | PathWin32Method::Format
                | PathWin32Method::Relative
                | PathWin32Method::Resolve
                | PathWin32Method::ResolveJoin,
            ..
        }
        | Expr::FileURLToPath(_)
        | Expr::OsPlatform
        | Expr::OsArch
        | Expr::OsHostname
        | Expr::OsHomedir
        | Expr::OsTmpdir
        | Expr::OsType
        | Expr::OsRelease
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion
        | Expr::FsReadFileSync(_)
        | Expr::ImportMetaUrl(_)
        | Expr::CryptoRandomUUID
        | Expr::CryptoRandomUUIDv7
        | Expr::CryptoSha256(_)
        | Expr::CryptoMd5(_)
        | Expr::RegExpEscape(_)
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        | Expr::RegExpExecIndex
        | Expr::RegExpReplaceFn { .. }
        | Expr::StringReplace { .. }
        | Expr::ErrorMessage(_)
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::EncodeURI(_)
        | Expr::DecodeURI(_)
        | Expr::EncodeURIComponent(_)
        | Expr::DecodeURIComponent(_)
        | Expr::Atob(_)
        | Expr::Btoa(_)
        | Expr::TextDecoderEncoding(_)
        | Expr::TextDecoderDecode { .. }
        | Expr::SymbolDescription(_)
        | Expr::SymbolToString(_)
        | Expr::UrlGetHref(_)
        | Expr::UrlGetPathname(_)
        | Expr::UrlGetProtocol(_)
        | Expr::UrlGetHost(_)
        | Expr::UrlGetHostname(_)
        | Expr::UrlGetPort(_)
        | Expr::UrlGetSearch(_)
        | Expr::UrlGetHash(_)
        | Expr::UrlGetOrigin(_)
        | Expr::UrlInstanceToString(_)
        | Expr::UrlInstanceToJSON(_)
        | Expr::UrlSearchParamsToString(_)
        | Expr::BufferToString { .. }
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringFromCodePoint(_)
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::DateToISOString(_)
        | Expr::DateToString(_)
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
        | Expr::DateToUTCString(_)
        | Expr::DateToLocaleString(_)
        | Expr::DateToLocaleDateString(_)
        | Expr::DateToLocaleTimeString(_)
        | Expr::DateToJSON(_) => Type::String,

        Expr::SymbolKeyFor(_) => Type::Union(vec![Type::String, Type::Void]),
        Expr::StringCodePointAt { .. } => Type::Union(vec![Type::Number, Type::Void]),
        Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => Type::Union(vec![Type::String, Type::Void]),
        Expr::UrlParse(_) | Expr::UrlParseWithBase { .. } => {
            Type::Union(vec![Type::Named("URL".to_string()), Type::Null])
        }
        Expr::UrlSearchParamsGet { .. } => Type::Union(vec![Type::String, Type::Null]),
        Expr::RegExpExecGroups => optional_type(Type::Object(ObjectType::default())),

        Expr::PathToNamespacedPath(path) => {
            if infer_expr_type(path, env).is_string_like() {
                Type::String
            } else {
                Type::Any
            }
        }
        Expr::PathWin32 {
            method: PathWin32Method::ToNamespacedPath,
            args,
        } => {
            if args
                .first()
                .is_some_and(|arg| infer_expr_type(arg, env).is_string_like())
            {
                Type::String
            } else {
                Type::Any
            }
        }
        Expr::PathWin32 {
            method: PathWin32Method::IsAbsolute | PathWin32Method::MatchesGlob,
            ..
        } => Type::Boolean,
        Expr::PathWin32 {
            method: PathWin32Method::Parse,
            ..
        } => Type::Object(ObjectType::default()),

        Expr::ProcessHrtimeBigint => Type::BigInt,
        Expr::ProcessHrtime(_) => Type::Tuple(vec![Type::Number, Type::Number]),

        Expr::OsTotalmem | Expr::OsFreemem | Expr::OsUptime | Expr::OsAvailableParallelism => {
            Type::Number
        }

        Expr::ProcessArgv
        | Expr::ProcessActiveResourcesInfo
        | Expr::ObjectGetOwnPropertyNames(_)
        | Expr::ObjectKeys(_)
        | Expr::ForInKeys(_)
        | Expr::StringSplit(_, _)
        | Expr::UrlSearchParamsGetAll { .. }
        | Expr::UrlSearchParamsKeys(_)
        | Expr::UrlSearchParamsValues(_) => Type::Array(Box::new(Type::String)),
        Expr::UrlSearchParamsEntries(_) => {
            Type::Array(Box::new(Type::Tuple(vec![Type::String, Type::String])))
        }
        Expr::WebAssemblyModuleExports(_) | Expr::WebAssemblyModuleImports(_) => {
            Type::Array(Box::new(Type::Object(ObjectType::default())))
        }
        Expr::WebAssemblyModuleCustomSections { .. } => {
            Type::Array(Box::new(Type::Named("ArrayBuffer".to_string())))
        }
        Expr::OsLoadavg => Type::Tuple(vec![Type::Number, Type::Number, Type::Number]),
        Expr::ObjectGetOwnPropertySymbols(_) => Type::Array(Box::new(Type::Symbol)),
        Expr::ObjectEntries(_) => Type::Array(Box::new(Type::Tuple(vec![Type::String, Type::Any]))),
        Expr::ReflectOwnKeys(_) => {
            Type::Array(Box::new(Type::Union(vec![Type::String, Type::Symbol])))
        }
        Expr::ReflectGetMetadataKeys { .. } | Expr::ReflectGetOwnMetadataKeys { .. } => {
            Type::Array(Box::new(Type::Any))
        }
        Expr::OsCpus | Expr::ObjectValues(_) | Expr::StringMatch { .. } => {
            Type::Array(Box::new(Type::Any))
        }

        Expr::GlobalThisExpr
        | Expr::NativeModuleRef(_)
        | Expr::ModuleTopThis
        | Expr::ProcessEnv
        | Expr::ProcessMemoryUsage
        | Expr::ProcessVersions
        | Expr::ProcessThreadCpuUsage(_)
        | Expr::ProcessCpuUsage(_)
        | Expr::ProcessResourceUsage
        | Expr::ProcessStdin
        | Expr::ProcessStdout
        | Expr::ProcessStderr
        | Expr::PathParse(_)
        | Expr::TextEncoderEncodeInto { .. }
        | Expr::ObjectCreate(_, _)
        | Expr::ObjectCoerce(_)
        | Expr::ObjectFromEntries(_)
        | Expr::ObjectGroupBy { .. }
        | Expr::ObjectRest { .. }
        | Expr::StringMatchAll { .. }
        | Expr::ProxyNew { .. }
        | Expr::ProxyConstruct { .. }
        | Expr::ProxyRevocable { .. }
        | Expr::ReflectConstruct { .. }
        | Expr::ChildProcessSpawnSync { .. }
        | Expr::ChildProcessSpawnBackground { .. }
        | Expr::ChildProcessGetProcessStatus(_)
        | Expr::ObjectGetOwnPropertyDescriptors(_)
        | Expr::JsonRawJson(_)
        | Expr::OsNetworkInterfaces
        | Expr::OsUserInfo
        | Expr::OsUserInfoBuffer => Type::Object(ObjectType::default()),

        Expr::ObjectDefineProperty(target, _, _)
        | Expr::ObjectFreeze(target)
        | Expr::ObjectSeal(target)
        | Expr::ObjectPreventExtensions(target)
        | Expr::ObjectDefineProperties(target, _)
        | Expr::ObjectSetPrototypeOf(target, _) => infer_expr_type(target, env),
        Expr::LinkGeneratorPrototype { obj, .. } => infer_expr_type(obj, env),
        Expr::ObjectGetPrototypeOf(_) | Expr::ReflectGetPrototypeOf(_) => {
            Type::Union(vec![Type::Object(ObjectType::default()), Type::Null])
        }
        Expr::ReflectGetOwnPropertyDescriptor { .. } => {
            optional_type(Type::Object(ObjectType::default()))
        }

        Expr::JsonParseTyped { ty, .. } => ty.clone(),
        Expr::JsSetProperty { value, .. } => infer_expr_type(value, env),

        Expr::ProcessSetTitle(value) => infer_expr_type(value, env),

        Expr::ProcessNextTick { .. }
        | Expr::ProcessOn { .. }
        | Expr::ProcessOnce { .. }
        | Expr::ProcessChdir(_)
        | Expr::ProcessKill { .. }
        | Expr::ProcessEmitWarning(_)
        | Expr::ProcessStdinSetRawMode(_)
        | Expr::ProcessStdinOn { .. }
        | Expr::ProcessStdinRemoveListener { .. }
        | Expr::ProcessStdinLifecycle(_)
        | Expr::ProcessStdoutOn { .. }
        | Expr::FsWriteFileSync(_, _)
        | Expr::FsMkdirSync(_)
        | Expr::FsUnlinkSync(_)
        | Expr::FsAppendFileSync(_, _)
        | Expr::RegExpSetLastIndex { .. }
        | Expr::FinalizationRegistryRegister { .. }
        | Expr::UrlSearchParamsSet { .. }
        | Expr::UrlSearchParamsAppend { .. }
        | Expr::UrlSearchParamsDelete { .. }
        | Expr::UrlSearchParamsSort(_)
        | Expr::UrlSearchParamsForEach { .. }
        | Expr::ProxyRevoke(_)
        | Expr::ReflectDefineMetadata { .. }
        | Expr::ChildProcessKillProcess(_)
        | Expr::NativeArenaDispose(_)
        | Expr::NativeMemoryFillU32 { .. }
        | Expr::NativeMemoryCopy { .. }
        | Expr::QueueMicrotask(_)
        | Expr::RegisterClassParentDynamic { .. }
        | Expr::RegisterClassCaptures { .. }
        | Expr::RegisterClassStaticSymbol { .. }
        | Expr::RegisterClassComputedMethod { .. }
        | Expr::RegisterClassComputedAccessor { .. }
        | Expr::IterResultSet(_, _) => Type::Void,
        Expr::ProcessExit(_) | Expr::ProcessAbort | Expr::UrlSearchParamsMissingArgs { .. } => {
            Type::Never
        }

        Expr::AsyncStepChain { value, .. } | Expr::AsyncStepDone { value, .. } => {
            Type::Promise(Box::new(infer_expr_type(value, env)))
        }
        Expr::FetchWithOptions { .. }
        | Expr::FetchGetWithAuth { .. }
        | Expr::FetchPostWithAuth { .. } => {
            Type::Promise(Box::new(Type::Named("Response".to_string())))
        }
        Expr::DynamicImport { .. } => Type::Promise(Box::new(Type::Object(ObjectType::default()))),
        Expr::WebAssemblyCompile(_) => {
            Type::Promise(Box::new(Type::Named("WebAssembly.Module".to_string())))
        }
        Expr::AsyncFirstCall { .. }
        | Expr::WebAssemblyInstantiate(_)
        | Expr::WebCryptoDigest { .. }
        | Expr::WebCryptoImportKey { .. }
        | Expr::WebCryptoExportKey { .. }
        | Expr::WebCryptoSign { .. }
        | Expr::WebCryptoVerify { .. }
        | Expr::WebCryptoEncrypt { .. }
        | Expr::WebCryptoDecrypt { .. }
        | Expr::WebCryptoGenerateKey { .. }
        | Expr::WebCryptoDeriveBits { .. }
        | Expr::WebCryptoDeriveKey { .. }
        | Expr::WebCryptoWrapKey { .. }
        | Expr::WebCryptoUnwrapKey { .. } => Type::Promise(Box::new(Type::Any)),

        _ => Type::Any,
    }
}

/// Infer a type only when it is precise enough to refine an existing
/// escape-hatch value type.
///
/// Consumers such as codegen local-type refinement want useful concrete facts
/// but should not overwrite a local with non-value, catch-all, or union
/// results. Unions remain available from [`infer_expr_type`] for static
/// lookups and widening, but local fast-path refinement should only promote a
/// single concrete family. Keep that policy next to the shared inference spine
/// so callers agree on the conservative filter.
pub fn infer_refinable_expr_type<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Option<Type> {
    match infer_expr_type(expr, env) {
        Type::Any
        | Type::Unknown
        | Type::Void
        | Type::Never
        | Type::Function(_)
        | Type::Union(_) => None,
        ty => Some(ty),
    }
}

fn function_type_for_return(return_type: Option<&Type>) -> Type {
    Type::Function(perry_types::FunctionType {
        params: Vec::new(),
        return_type: Box::new(return_type.cloned().unwrap_or(Type::Any)),
        is_async: false,
        is_generator: false,
    })
}

fn function_type_from_decl(function: &Function) -> Type {
    Type::Function(perry_types::FunctionType {
        params: function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone(), false))
            .collect(),
        return_type: Box::new(function.return_type.clone()),
        is_async: function.is_async || function.was_plain_async,
        is_generator: function.is_generator,
    })
}

fn interface_method_type(method: &InterfaceMethod) -> Type {
    Type::Function(perry_types::FunctionType {
        params: method.params.clone(),
        return_type: Box::new(method.return_type.clone()),
        is_async: false,
        is_generator: false,
    })
}

fn named_type_base(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) => Some(name),
        Type::Generic { base, .. } => Some(base),
        _ => None,
    }
}

fn enum_member_value_type(value: &EnumValue) -> Type {
    match value {
        EnumValue::Number(_) => Type::Number,
        EnumValue::String(_) => Type::String,
    }
}

fn generic_type(base: &str) -> Type {
    Type::Generic {
        base: base.to_string(),
        type_args: Vec::new(),
    }
}

fn map_type(key: Type, value: Type) -> Type {
    Type::Generic {
        base: "Map".to_string(),
        type_args: vec![key, value],
    }
}

fn set_type(value: Type) -> Type {
    Type::Generic {
        base: "Set".to_string(),
        type_args: vec![value],
    }
}

fn function_return_type_from_expr<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    match infer_expr_type(expr, env) {
        Type::Function(function) => *function.return_type,
        _ => Type::Any,
    }
}

fn typed_collection_or_default<F: HirTypeFacts + ?Sized>(expr: &Expr, base: &str, env: &F) -> Type {
    match infer_expr_type(expr, env) {
        Type::Any | Type::Unknown => generic_type(base),
        ty => ty,
    }
}

fn generic_type_arg_from_expr<F: HirTypeFacts + ?Sized>(
    expr: &Expr,
    base: &str,
    index: usize,
    env: &F,
) -> Type {
    match infer_expr_type(expr, env) {
        Type::Generic {
            base: ty_base,
            type_args,
        } if ty_base == base => type_args.get(index).cloned().unwrap_or(Type::Any),
        _ => Type::Any,
    }
}

fn optional_type(ty: Type) -> Type {
    match ty {
        Type::Any | Type::Unknown => ty,
        Type::Union(mut variants) => {
            if !variants.iter().any(|variant| matches!(variant, Type::Void)) {
                variants.push(Type::Void);
            }
            Type::Union(variants)
        }
        ty => Type::Union(vec![ty, Type::Void]),
    }
}

fn array_element_type_from_expr<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    array_element_type_from_type(infer_expr_type(expr, env))
}

fn array_element_type_from_local<F: HirTypeFacts + ?Sized>(id: LocalId, env: &F) -> Type {
    env.local_type(id)
        .cloned()
        .map(array_element_type_from_type)
        .unwrap_or(Type::Any)
}

fn array_element_type_from_type(ty: Type) -> Type {
    match ty {
        Type::Array(elem) => *elem,
        Type::Tuple(elems) => unify_types(elems),
        ty if ty.is_string_like() => Type::String,
        _ => Type::Any,
    }
}

fn flattened_array_element_type(ty: Type) -> Type {
    match ty {
        Type::Array(elem) => *elem,
        Type::Tuple(elems) => unify_types(elems.into_iter().map(flattened_array_element_type)),
        ty => ty,
    }
}

fn array_type_from_expr<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    array_type_from_type(infer_expr_type(expr, env))
}

fn array_type_from_iterable_expr<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    match infer_expr_type(expr, env) {
        Type::Generic { base, type_args } if base == "Map" => {
            let key = type_args.first().cloned().unwrap_or(Type::Any);
            let value = type_args.get(1).cloned().unwrap_or(Type::Any);
            Type::Array(Box::new(Type::Tuple(vec![key, value])))
        }
        Type::Generic { base, type_args } if base == "Set" => {
            Type::Array(Box::new(type_args.first().cloned().unwrap_or(Type::Any)))
        }
        ty => array_type_from_type(ty),
    }
}

fn map_type_from_entries_expr<F: HirTypeFacts + ?Sized>(expr: &Expr, env: &F) -> Type {
    if let Expr::Array(entries) = expr {
        let mut key_types = Vec::new();
        let mut value_types = Vec::new();
        for entry in entries {
            let Expr::Array(items) = entry else {
                return generic_type("Map");
            };
            let [key, value, ..] = items.as_slice() else {
                return generic_type("Map");
            };
            key_types.push(infer_expr_type(key, env));
            value_types.push(infer_expr_type(value, env));
        }
        if !key_types.is_empty() {
            return map_type(unify_types(key_types), unify_types(value_types));
        }
    }

    match array_element_type_from_expr(expr, env) {
        Type::Tuple(elems) if elems.len() >= 2 => map_type(elems[0].clone(), elems[1].clone()),
        Type::Array(elem) => {
            let elem = *elem;
            map_type(elem.clone(), elem)
        }
        _ => generic_type("Map"),
    }
}

fn infer_arraylike_method_type<F: HirTypeFacts + ?Sized>(
    method: &str,
    receiver: &Expr,
    args: &[Expr],
    env: &F,
) -> Type {
    match method {
        "forEach" => Type::Void,
        "map" => Type::Array(Box::new(
            args.first()
                .map(|callback| function_return_type_from_expr(callback, env))
                .unwrap_or(Type::Any),
        )),
        "filter" | "slice" | "splice" => {
            Type::Array(Box::new(array_element_type_from_expr(receiver, env)))
        }
        "concat" => Type::Array(Box::new(Type::Any)),
        "some" | "every" | "includes" => Type::Boolean,
        "find" | "findLast" | "at" => optional_type(array_element_type_from_expr(receiver, env)),
        "findIndex" | "findLastIndex" | "indexOf" | "lastIndexOf" => Type::Number,
        "reduce" | "reduceRight" => args
            .get(1)
            .map(|initial| infer_expr_type(initial, env))
            .unwrap_or_else(|| {
                args.first()
                    .map(|callback| function_return_type_from_expr(callback, env))
                    .unwrap_or(Type::Any)
            }),
        "join" => Type::String,
        "sort" => infer_expr_type(receiver, env),
        _ => Type::Any,
    }
}

fn array_type_from_local<F: HirTypeFacts + ?Sized>(id: LocalId, env: &F) -> Type {
    env.local_type(id)
        .cloned()
        .map(array_type_from_type)
        .unwrap_or_else(|| Type::Array(Box::new(Type::Any)))
}

fn array_type_from_type(ty: Type) -> Type {
    match ty {
        array @ Type::Array(_) => array,
        Type::Tuple(elems) => Type::Array(Box::new(unify_types(elems))),
        ty if ty.is_string_like() => Type::Array(Box::new(Type::String)),
        _ => Type::Array(Box::new(Type::Any)),
    }
}

fn typed_array_kind_name(kind: u8) -> Option<&'static str> {
    match kind {
        0 => Some("Int8Array"),
        1 => Some("Uint8Array"),
        2 => Some("Int16Array"),
        3 => Some("Uint16Array"),
        4 => Some("Int32Array"),
        5 => Some("Uint32Array"),
        6 => Some("Float32Array"),
        7 => Some("Float64Array"),
        8 => Some("Uint8ClampedArray"),
        9 => Some("BigInt64Array"),
        10 => Some("BigUint64Array"),
        11 => Some("Float16Array"),
        _ => None,
    }
}

fn infer_binary_type<F: HirTypeFacts + ?Sized>(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    env: &F,
) -> Type {
    match op {
        BinaryOp::Add => {
            let left_ty = infer_expr_type(left, env);
            let right_ty = infer_expr_type(right, env);
            if left_ty.is_string_like() || right_ty.is_string_like() {
                Type::String
            } else if matches!(left_ty, Type::BigInt) && matches!(right_ty, Type::BigInt) {
                // Mixed BigInt/Number arithmetic throws a TypeError at runtime,
                // so only both-BigInt operands yield a BigInt result.
                Type::BigInt
            } else if left_ty.is_number_like() && right_ty.is_number_like() {
                Type::Number
            } else {
                Type::Any
            }
        }
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
            let left_ty = infer_expr_type(left, env);
            let right_ty = infer_expr_type(right, env);
            if matches!(left_ty, Type::BigInt) && matches!(right_ty, Type::BigInt) {
                Type::BigInt
            } else if left_ty.is_number_like() && right_ty.is_number_like() {
                Type::Number
            } else {
                Type::Any
            }
        }
        BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::UShr => Type::Number,
    }
}

/// Union of two inferred types, collapsing to `Any` if either side is unknown
/// and deduping the trivially-equal case. Used where a value can take one of
/// two operand types (e.g. `&&`/`||`).
fn union_of(left: Type, right: Type) -> Type {
    if matches!(left, Type::Any) || matches!(right, Type::Any) {
        Type::Any
    } else if left == right {
        left
    } else {
        Type::Union(vec![left, right])
    }
}

fn infer_logical_type<F: HirTypeFacts + ?Sized>(
    op: LogicalOp,
    left: &Expr,
    right: &Expr,
    env: &F,
) -> Type {
    match op {
        LogicalOp::And | LogicalOp::Or => {
            // `a && b` / `a || b` evaluate to EITHER operand depending on `a`'s
            // truthiness (e.g. `0 && "x"` is `0`), so the result is their union.
            let left_ty = infer_expr_type(left, env);
            let right_ty = infer_expr_type(right, env);
            union_of(left_ty, right_ty)
        }
        LogicalOp::Coalesce => {
            let left_ty = infer_expr_type(left, env);
            if matches!(left_ty, Type::Any | Type::Null | Type::Void) {
                infer_expr_type(right, env)
            } else {
                left_ty
            }
        }
    }
}

fn infer_property_get_type<F: HirTypeFacts + ?Sized>(
    object: &Expr,
    property: &str,
    env: &F,
) -> Type {
    match object {
        Expr::Object(entries) => entries
            .iter()
            .rev()
            .find_map(|(key, value)| (key == property).then(|| infer_expr_type(value, env)))
            .unwrap_or(Type::Any),
        _ => match infer_expr_type(object, env) {
            Type::Array(_) | Type::Tuple(_) if property == "length" => Type::Number,
            ty if ty.is_string_like() && property == "length" => Type::Number,
            Type::Object(object) => object
                .properties
                .get(property)
                .map(|info| info.ty.clone())
                .or_else(|| object.index_signature.as_deref().cloned())
                .unwrap_or(Type::Any),
            Type::Named(name) => env
                .named_property_type(&name, property)
                .unwrap_or(Type::Any),
            Type::Generic { base, .. } => env
                .named_property_type(&base, property)
                .unwrap_or(Type::Any),
            Type::Function(_) if property == "length" => Type::Number,
            _ => Type::Any,
        },
    }
}

fn object_type_from_entries<F: HirTypeFacts + ?Sized>(
    entries: &[(String, Expr)],
    env: &F,
) -> ObjectType {
    let mut object = ObjectType::default();
    object.property_order = Some(entries.iter().map(|(name, _)| name.clone()).collect());
    for (name, value) in entries {
        object.properties.insert(
            name.clone(),
            PropertyInfo {
                ty: infer_expr_type(value, env),
                optional: false,
                readonly: false,
            },
        );
    }
    object
}

fn unify_expr_types<'a, F: HirTypeFacts + ?Sized>(
    exprs: impl Iterator<Item = &'a Expr>,
    env: &F,
) -> Type {
    unify_types(exprs.map(|expr| infer_expr_type(expr, env)))
}

fn unify_types(types: impl IntoIterator<Item = Type>) -> Type {
    let mut unified: Option<Type> = None;
    for ty in types {
        match &unified {
            None => unified = Some(ty),
            Some(current) if *current == ty => {}
            Some(_) => return Type::Any,
        }
    }
    unified.unwrap_or(Type::Any)
}

fn same_type_or_any(left: Type, right: Type) -> Type {
    if left == right {
        left
    } else {
        Type::Any
    }
}

#[cfg(test)]
#[path = "value_types_tests.rs"]
mod tests;
