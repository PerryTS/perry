//! Driver-time resolution of type-alias references that HIR lowering could not
//! settle by itself.
//!
//! Per-module lowering resolves a *non-generic, same-module* alias reference at
//! extraction time (`LoweringContext::resolve_type_alias`). Two shapes stay
//! opaque after that:
//!
//! * an alias imported from another module — `import type { EntityId } from
//!   "../entity"` leaves every `EntityId` annotation as `Type::Named("EntityId")`;
//! * a generic alias instantiation — `EntityId<T>` is `Type::Generic { base:
//!   "EntityId", .. }` even in the defining module, because the same-module
//!   resolver only accepts aliases without type parameters.
//!
//! Both erase what the alias actually spells. For the branded-primitive idiom
//! (`type EntityId<T> = number & { __tag?: T }`) that erasure turns every id in
//! a program into a dynamic value: comparisons take the generic relational and
//! equality helpers, Map keys lose their numeric proofs, and typed calling
//! conventions never fire — even though the same annotation written as plain
//! `number` would be guarded and lowered natively.
//!
//! This pass runs once all modules are lowered. Each module gets a table of the
//! aliases in *its own scope* (its declarations plus the aliases its imports
//! bind, keyed by the local binding name and looked up through the resolved
//! import path, exactly like the enum fix-up), so a name is never resolved
//! against an unrelated module's alias of the same name. Alias bodies are first
//! closed against their defining module's scope, then every type position in
//! the module is rewritten.
//!
//! Resolution is deliberately conservative: an alias reference is replaced only
//! when the instantiated body contains no remaining `TypeVar` and no alias
//! reference it could not resolve. Everything else keeps its original
//! `Named`/`Generic` spelling, so consumers that key on those shapes see exactly
//! what they saw before.

use std::collections::{BTreeMap, HashMap};

use crate::ir::*;
use crate::monomorph::substitute_type;
use crate::types::{ObjectType, PropertyInfo, Type, TypeParam};
use crate::walker::walk_expr_children_mut;

/// A type alias definition as seen from a consuming module.
#[derive(Clone, Debug, PartialEq)]
pub struct AliasDef {
    pub params: Vec<TypeParam>,
    pub ty: Type,
}

/// Alias definitions visible in one module, keyed by the local binding name.
pub type AliasTable = BTreeMap<String, AliasDef>;

/// Bound on alias-of-alias chasing. Real chains are two or three deep
/// (`ComponentId<T>` → `EntityId<T, "component">` → `number`); a malformed cycle
/// must terminate rather than hang the compiler.
const MAX_DEPTH: usize = 8;

/// Resolve every alias reference inside `ty` against `table`.
///
/// Returns `ty` unchanged (structurally) where nothing resolves. A reference
/// whose instantiated body still carries a `TypeVar` or an unresolved alias
/// reference is left as written.
pub fn resolve_type(ty: &Type, table: &AliasTable) -> Type {
    resolve_type_inner(ty, table, 0)
}

fn resolve_type_inner(ty: &Type, table: &AliasTable, depth: usize) -> Type {
    if depth > MAX_DEPTH {
        return ty.clone();
    }
    match ty {
        Type::Named(name) => match table.get(name) {
            Some(def) => instantiate(def, &[], table, depth).unwrap_or_else(|| ty.clone()),
            None => ty.clone(),
        },
        Type::Generic { base, type_args } => {
            let args: Vec<Type> = type_args
                .iter()
                .map(|t| resolve_type_inner(t, table, depth))
                .collect();
            match table.get(base) {
                Some(def) => {
                    instantiate(def, &args, table, depth).unwrap_or_else(|| Type::Generic {
                        base: base.clone(),
                        type_args: args,
                    })
                }
                None => Type::Generic {
                    base: base.clone(),
                    type_args: args,
                },
            }
        }
        Type::Array(elem) => Type::Array(Box::new(resolve_type_inner(elem, table, depth))),
        Type::Tuple(elems) => Type::Tuple(
            elems
                .iter()
                .map(|e| resolve_type_inner(e, table, depth))
                .collect(),
        ),
        Type::Promise(inner) => Type::Promise(Box::new(resolve_type_inner(inner, table, depth))),
        Type::Union(types) => {
            // Two branded aliases of one primitive (`EntityId | ComponentId`)
            // resolve to the same member; a union of identical members is
            // that member, so the binding gets the primitive's lowering.
            let mut resolved: Vec<Type> = Vec::with_capacity(types.len());
            for t in types {
                let t = resolve_type_inner(t, table, depth);
                if !resolved.contains(&t) {
                    resolved.push(t);
                }
            }
            if resolved.len() == 1 {
                resolved.pop().expect("one member")
            } else {
                Type::Union(resolved)
            }
        }
        Type::Function(f) => Type::Function(crate::types::FunctionType {
            params: f
                .params
                .iter()
                .map(|(n, t, opt)| (n.clone(), resolve_type_inner(t, table, depth), *opt))
                .collect(),
            return_type: Box::new(resolve_type_inner(&f.return_type, table, depth)),
            is_async: f.is_async,
            is_generator: f.is_generator,
        }),
        Type::Object(obj) => Type::Object(ObjectType {
            name: obj.name.clone(),
            properties: obj
                .properties
                .iter()
                .map(|(k, p)| {
                    (
                        k.clone(),
                        PropertyInfo {
                            ty: resolve_type_inner(&p.ty, table, depth),
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    )
                })
                .collect(),
            property_order: obj.property_order.clone(),
            index_signature: obj
                .index_signature
                .as_ref()
                .map(|t| Box::new(resolve_type_inner(t, table, depth))),
        }),
        _ => ty.clone(),
    }
}

/// Instantiate `def` with positional `args` (a missing argument takes the
/// parameter default, else `Any`) and resolve the body. `None` when the result
/// is not closed — it still mentions a type variable or an alias the table
/// cannot resolve — so the caller keeps the original reference.
fn instantiate(def: &AliasDef, args: &[Type], table: &AliasTable, depth: usize) -> Option<Type> {
    let body = if def.params.is_empty() {
        def.ty.clone()
    } else {
        let mut subs: HashMap<String, Type> = HashMap::new();
        for (i, p) in def.params.iter().enumerate() {
            let arg = args
                .get(i)
                .cloned()
                .or_else(|| p.default.as_deref().cloned())
                .unwrap_or(Type::Any);
            subs.insert(p.name.clone(), arg);
        }
        substitute_type_deep(&def.ty, &subs)
    };
    let resolved = resolve_type_inner(&body, table, depth + 1);
    if type_is_closed(&resolved, table) {
        Some(resolved)
    } else {
        None
    }
}

/// [`substitute_type`] with descent into object-literal types, which the
/// monomorphizer's substitution leaves opaque; an alias body such as
/// `type Box<T> = { value: T }` carries its parameter inside the object.
fn substitute_type_deep(ty: &Type, subs: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Object(obj) => Type::Object(ObjectType {
            name: obj.name.clone(),
            properties: obj
                .properties
                .iter()
                .map(|(k, p)| {
                    (
                        k.clone(),
                        PropertyInfo {
                            ty: substitute_type_deep(&p.ty, subs),
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    )
                })
                .collect(),
            property_order: obj.property_order.clone(),
            index_signature: obj
                .index_signature
                .as_ref()
                .map(|t| Box::new(substitute_type_deep(t, subs))),
        }),
        Type::Array(e) => Type::Array(Box::new(substitute_type_deep(e, subs))),
        Type::Promise(e) => Type::Promise(Box::new(substitute_type_deep(e, subs))),
        Type::Tuple(v) => Type::Tuple(v.iter().map(|t| substitute_type_deep(t, subs)).collect()),
        Type::Union(v) => Type::Union(v.iter().map(|t| substitute_type_deep(t, subs)).collect()),
        Type::Generic { base, type_args } => Type::Generic {
            base: base.clone(),
            type_args: type_args
                .iter()
                .map(|t| substitute_type_deep(t, subs))
                .collect(),
        },
        Type::Function(f) => Type::Function(crate::types::FunctionType {
            params: f
                .params
                .iter()
                .map(|(n, t, opt)| (n.clone(), substitute_type_deep(t, subs), *opt))
                .collect(),
            return_type: Box::new(substitute_type_deep(&f.return_type, subs)),
            is_async: f.is_async,
            is_generator: f.is_generator,
        }),
        _ => substitute_type(ty, subs),
    }
}

/// A resolved alias body may be substituted for its reference only when it
/// mentions no type variable and no alias the table knows but could not
/// resolve (a cycle or a non-closed instantiation).
fn type_is_closed(ty: &Type, table: &AliasTable) -> bool {
    match ty {
        Type::TypeVar(_) => false,
        Type::Named(name) => !table.contains_key(name),
        Type::Generic { base, type_args } => {
            !table.contains_key(base) && type_args.iter().all(|t| type_is_closed(t, table))
        }
        Type::Array(e) | Type::Promise(e) => type_is_closed(e, table),
        Type::Tuple(v) | Type::Union(v) => v.iter().all(|t| type_is_closed(t, table)),
        Type::Function(f) => {
            f.params.iter().all(|(_, t, _)| type_is_closed(t, table))
                && type_is_closed(&f.return_type, table)
        }
        Type::Object(obj) => {
            obj.properties
                .values()
                .all(|p| type_is_closed(&p.ty, table))
                && obj
                    .index_signature
                    .as_ref()
                    .is_none_or(|t| type_is_closed(t, table))
        }
        _ => true,
    }
}

/// Rewrite every type position in `module` through [`resolve_type`].
pub fn resolve_type_aliases_in_module(module: &mut Module, table: &AliasTable) {
    if table.is_empty() {
        return;
    }
    for func in &mut module.functions {
        fix_function(func, table);
    }
    for class in &mut module.classes {
        fix_class(class, table);
    }
    for global in &mut module.globals {
        global.ty = resolve_type(&global.ty, table);
    }
    for iface in &mut module.interfaces {
        for ext in &mut iface.extends {
            *ext = resolve_type(ext, table);
        }
        for prop in &mut iface.properties {
            prop.ty = resolve_type(&prop.ty, table);
        }
        for method in &mut iface.methods {
            for (_, ty, _) in &mut method.params {
                *ty = resolve_type(ty, table);
            }
            method.return_type = resolve_type(&method.return_type, table);
        }
    }
    for alias in &mut module.type_aliases {
        alias.ty = resolve_type(&alias.ty, table);
    }
    fix_stmts(&mut module.init, table);
}

fn fix_function(func: &mut Function, table: &AliasTable) {
    for param in &mut func.params {
        param.ty = resolve_type(&param.ty, table);
        if let Some(default) = param.default.as_mut() {
            fix_expr(default, table);
        }
    }
    func.return_type = resolve_type(&func.return_type, table);
    fix_stmts(&mut func.body, table);
}

fn fix_field(field: &mut ClassField, table: &AliasTable) {
    field.ty = resolve_type(&field.ty, table);
    if let Some(init) = field.init.as_mut() {
        fix_expr(init, table);
    }
    if let Some(key) = field.key_expr.as_mut() {
        fix_expr(key, table);
    }
}

fn fix_class(class: &mut Class, table: &AliasTable) {
    for field in &mut class.fields {
        fix_field(field, table);
    }
    for field in &mut class.static_fields {
        fix_field(field, table);
    }
    if let Some(ctor) = class.constructor.as_mut() {
        fix_function(ctor, table);
    }
    for method in &mut class.methods {
        fix_function(method, table);
    }
    for method in &mut class.static_methods {
        fix_function(method, table);
    }
    for (_, getter) in &mut class.getters {
        fix_function(getter, table);
    }
    for (_, setter) in &mut class.setters {
        fix_function(setter, table);
    }
    for member in &mut class.computed_members {
        fix_expr(&mut member.key_expr, table);
        fix_function(&mut member.function, table);
    }
    if let Some(extends) = class.extends_expr.as_mut() {
        fix_expr(extends, table);
    }
}

fn fix_stmts(stmts: &mut [Stmt], table: &AliasTable) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let { ty, init, .. } => {
                *ty = resolve_type(ty, table);
                if let Some(init) = init.as_mut() {
                    fix_expr(init, table);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                fix_expr(expr, table);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                fix_expr(condition, table);
                fix_stmts(then_branch, table);
                if let Some(else_branch) = else_branch.as_mut() {
                    fix_stmts(else_branch, table);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                fix_expr(condition, table);
                fix_stmts(body, table);
            }
            Stmt::Labeled { body, .. } => fix_stmts(std::slice::from_mut(&mut **body), table),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init.as_mut() {
                    fix_stmts(std::slice::from_mut(&mut **init), table);
                }
                if let Some(condition) = condition.as_mut() {
                    fix_expr(condition, table);
                }
                if let Some(update) = update.as_mut() {
                    fix_expr(update, table);
                }
                fix_stmts(body, table);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                fix_expr(discriminant, table);
                for case in cases {
                    if let Some(test) = case.test.as_mut() {
                        fix_expr(test, table);
                    }
                    fix_stmts(&mut case.body, table);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                fix_stmts(body, table);
                if let Some(catch) = catch.as_mut() {
                    fix_stmts(&mut catch.body, table);
                }
                if let Some(finally) = finally.as_mut() {
                    fix_stmts(finally, table);
                }
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
}

fn fix_expr(expr: &mut Expr, table: &AliasTable) {
    match expr {
        Expr::Closure {
            params,
            return_type,
            body,
            ..
        } => {
            for param in params.iter_mut() {
                param.ty = resolve_type(&param.ty, table);
            }
            *return_type = resolve_type(return_type, table);
            fix_stmts(body, table);
        }
        Expr::ExternFuncRef {
            param_types,
            return_type,
            ..
        } => {
            for ty in param_types.iter_mut() {
                *ty = resolve_type(ty, table);
            }
            *return_type = resolve_type(return_type, table);
        }
        Expr::JsonParseTyped { ty, .. }
        | Expr::PodLayoutSizeOf { ty }
        | Expr::PodLayoutAlignOf { ty }
        | Expr::PodLayoutOffsetOf { ty, .. } => {
            *ty = resolve_type(ty, table);
        }
        _ => {}
    }
    // Direct children, including closure parameter defaults; the closure body
    // (a statement list) was handled above.
    walk_expr_children_mut(expr, &mut |child| fix_expr(child, table));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &str, default: Option<Type>) -> TypeParam {
        TypeParam {
            name: name.to_string(),
            constraint: None,
            default: default.map(Box::new),
        }
    }

    fn branded_table() -> AliasTable {
        let mut table = AliasTable::new();
        // type EntityId<T = unknown, U = unknown> = number & { … }  →  Number
        table.insert(
            "EntityId".into(),
            AliasDef {
                params: vec![
                    param("T", Some(Type::Unknown)),
                    param("U", Some(Type::Unknown)),
                ],
                ty: Type::Number,
            },
        );
        // type ComponentId<T = void> = EntityId<T, "component">
        table.insert(
            "ComponentId".into(),
            AliasDef {
                params: vec![param("T", Some(Type::Void))],
                ty: Type::Generic {
                    base: "EntityId".into(),
                    type_args: vec![
                        Type::TypeVar("T".into()),
                        Type::StringLiteral("component".into()),
                    ],
                },
            },
        );
        // type Box<T> = { value: T }  — open body
        let mut props = HashMap::new();
        props.insert(
            "value".to_string(),
            PropertyInfo {
                ty: Type::TypeVar("T".into()),
                optional: false,
                readonly: false,
            },
        );
        table.insert(
            "Box".into(),
            AliasDef {
                params: vec![param("T", None)],
                ty: Type::Object(ObjectType {
                    name: None,
                    properties: props,
                    property_order: None,
                    index_signature: None,
                }),
            },
        );
        table
    }

    #[test]
    fn branded_generic_alias_resolves_to_its_primitive() {
        let table = branded_table();
        assert_eq!(
            resolve_type(&Type::Named("EntityId".into()), &table),
            Type::Number
        );
        assert_eq!(
            resolve_type(
                &Type::Generic {
                    base: "EntityId".into(),
                    type_args: vec![Type::Any],
                },
                &table
            ),
            Type::Number
        );
        // Alias of an alias, with the argument threaded through.
        assert_eq!(
            resolve_type(
                &Type::Generic {
                    base: "ComponentId".into(),
                    type_args: vec![Type::String],
                },
                &table
            ),
            Type::Number
        );
        // Nested positions.
        assert_eq!(
            resolve_type(
                &Type::Generic {
                    base: "Map".into(),
                    type_args: vec![
                        Type::Named("EntityId".into()),
                        Type::Array(Box::new(Type::Named("ComponentId".into()))),
                    ],
                },
                &table
            ),
            Type::Generic {
                base: "Map".into(),
                type_args: vec![Type::Number, Type::Array(Box::new(Type::Number))],
            }
        );
    }

    #[test]
    fn a_union_of_branded_aliases_of_one_primitive_is_that_primitive() {
        let table = branded_table();
        assert_eq!(
            resolve_type(
                &Type::Union(vec![
                    Type::Named("EntityId".into()),
                    Type::Named("ComponentId".into()),
                ]),
                &table
            ),
            Type::Number
        );
        assert_eq!(
            resolve_type(
                &Type::Union(vec![Type::Named("EntityId".into()), Type::Any]),
                &table
            ),
            Type::Union(vec![Type::Number, Type::Any])
        );
    }

    #[test]
    fn open_instantiations_and_unknown_names_keep_their_spelling() {
        let table = branded_table();
        // A consumer's own type variable stays a type variable, so the alias
        // reference is left alone.
        let open = Type::Generic {
            base: "Box".into(),
            type_args: vec![Type::TypeVar("Q".into())],
        };
        assert_eq!(resolve_type(&open, &table), open);
        // A closed instantiation of an object alias resolves.
        let closed = Type::Generic {
            base: "Box".into(),
            type_args: vec![Type::Number],
        };
        match resolve_type(&closed, &table) {
            Type::Object(obj) => assert_eq!(obj.properties["value"].ty, Type::Number),
            other => panic!("expected an object type, got {other:?}"),
        }
        // Names outside the table are untouched.
        assert_eq!(
            resolve_type(&Type::Named("Archetype".into()), &table),
            Type::Named("Archetype".into())
        );
    }

    #[test]
    fn cyclic_aliases_terminate_unresolved() {
        let mut table = AliasTable::new();
        table.insert(
            "A".into(),
            AliasDef {
                params: vec![],
                ty: Type::Named("B".into()),
            },
        );
        table.insert(
            "B".into(),
            AliasDef {
                params: vec![],
                ty: Type::Named("A".into()),
            },
        );
        assert_eq!(
            resolve_type(&Type::Named("A".into()), &table),
            Type::Named("A".into())
        );
    }

    #[test]
    fn module_pass_rewrites_params_locals_fields_and_closures() {
        let table = branded_table();
        let id_ty = Type::Generic {
            base: "EntityId".into(),
            type_args: vec![Type::Any],
        };
        let mut module = Module::new("m");
        module.functions.push(Function {
            id: 0,
            name: "f".into(),
            type_params: vec![],
            params: vec![Param {
                id: 0,
                name: "id".into(),
                ty: id_ty.clone(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Named("ComponentId".into()),
            body: vec![Stmt::Let {
                id: 1,
                name: "x".into(),
                ty: id_ty.clone(),
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 1,
                    params: vec![],
                    return_type: id_ty.clone(),
                    body: vec![Stmt::Let {
                        id: 2,
                        name: "y".into(),
                        ty: id_ty.clone(),
                        mutable: false,
                        init: None,
                    }],
                    captures: vec![],
                    mutable_captures: vec![],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: true,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            }],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        resolve_type_aliases_in_module(&mut module, &table);
        let f = &module.functions[0];
        assert_eq!(f.params[0].ty, Type::Number);
        assert_eq!(f.return_type, Type::Number);
        let Stmt::Let { ty, init, .. } = &f.body[0] else {
            panic!("expected let");
        };
        assert_eq!(*ty, Type::Number);
        let Some(Expr::Closure {
            return_type, body, ..
        }) = init
        else {
            panic!("expected closure");
        };
        assert_eq!(*return_type, Type::Number);
        let Stmt::Let { ty, .. } = &body[0] else {
            panic!("expected inner let");
        };
        assert_eq!(*ty, Type::Number);
    }
}
