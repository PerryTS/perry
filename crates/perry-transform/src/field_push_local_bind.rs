//! Bind `this.field.push(v)` to a local so the inline append lowering applies.
//!
//! `arr.push(v)` on a LOCAL array lowers to `Expr::ArrayPush { array_id }`,
//! which codegen turns into an inline bump append: a header test, one store,
//! and — when the value and the live header jointly prove it — none of the
//! per-store GC bookkeeping calls. The same push through a class field
//! (`this.commands.push(cmd)`) is a `NativeMethodCall { module: "array",
//! method: "push_single" }` and lowers to `js_array_push_guard` +
//! `js_array_push_f64` + `js_array_length`, with the runtime doing the layout
//! note and the barrier out of line: on an ECS command buffer that was 7% of
//! the frame, all in one statement.
//!
//! This pass rewrites the statement form
//!
//! ```text
//! this.f.push(v);
//! ```
//!
//! into
//!
//! ```text
//! let __push_recv_old = this.f;
//! let __push_recv = __push_recv_old;
//! __push_recv.push(v);                       // Expr::ArrayPush
//! if (__push_recv !== __push_recv_old) this.f = __push_recv;
//! ```
//!
//! which is, read for read and write for write, what the native lowering
//! already did: the field is read once before the value is evaluated, the
//! push targets that array, and the field is written back only when the
//! append re-allocated the head (`arr.push.wb` in
//! `lower_call/native/native_instance_branch.rs` compares the returned head
//! against the original for exactly this). A `let` local is what codegen
//! roots across the value's evaluation, so the head survives a collection
//! there; a synthetic codegen slot would not.
//!
//! Admission is deliberately narrow:
//!
//! * the receiver is `this.f` where `f` is an instance FIELD of the enclosing
//!   class (declared in `Class::fields`) whose declared type is an array,
//!   and no getter or setter of that name exists — a rewritten accessor
//!   would turn one call into a read plus a write;
//! * the call is a statement (its length result is unused);
//! * exactly one non-spread argument (`push_single`);
//! * instance methods, getters and setters only — never a constructor,
//!   where a field may not be initialised yet, and never a static method.
use perry_hir::types::LocalId;
use perry_hir::types::Type;
use perry_hir::{Class, CompareOp, Expr, Function, Module, Stmt};

use crate::closure_local_inline::nested_stmt_lists;

pub fn run(module: &mut Module) {
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);
    for c in &mut module.classes {
        let fields: Vec<(String, Type)> = admissible_fields(c);
        if fields.is_empty() {
            continue;
        }
        for m in &mut c.methods {
            run_function(m, &fields, &mut next_local_id);
        }
        for (_, g) in &mut c.getters {
            run_function(g, &fields, &mut next_local_id);
        }
        for (_, s) in &mut c.setters {
            run_function(s, &fields, &mut next_local_id);
        }
    }
}

/// Instance fields with a declared array type that no accessor shadows.
fn admissible_fields(c: &Class) -> Vec<(String, Type)> {
    c.fields
        .iter()
        .filter(|f| f.key_expr.is_none() && matches!(f.ty, Type::Array(_)))
        .filter(|f| {
            !c.getters.iter().any(|(name, _)| name == &f.name)
                && !c.setters.iter().any(|(name, _)| name == &f.name)
        })
        .map(|f| (f.name.clone(), f.ty.clone()))
        .collect()
}

fn run_function(f: &mut Function, fields: &[(String, Type)], next_local_id: &mut LocalId) {
    if f.is_async || f.is_generator {
        return;
    }
    process_stmts(&mut f.body, fields, next_local_id);
}

fn process_stmts(stmts: &mut Vec<Stmt>, fields: &[(String, Type)], next_local_id: &mut LocalId) {
    for s in stmts.iter_mut() {
        for inner in nested_stmt_lists(s) {
            process_stmts(inner, fields, next_local_id);
        }
    }
    let mut i = 0;
    while i < stmts.len() {
        let Some((field, ty)) = field_push_candidate(&stmts[i], fields) else {
            i += 1;
            continue;
        };
        let Stmt::Expr(Expr::NativeMethodCall { args, .. }) = stmts.remove(i) else {
            unreachable!("candidate shape was just matched");
        };
        let value = args
            .into_iter()
            .next()
            .expect("push_single carries one argument");
        let old_id = alloc_local(next_local_id);
        let recv_id = alloc_local(next_local_id);
        let field_get = || Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: field.clone(),
            byte_offset: 0,
        };
        let rewritten = [
            Stmt::Let {
                id: old_id,
                name: "__push_recv_old".to_string(),
                ty: ty.clone(),
                mutable: false,
                init: Some(field_get()),
            },
            Stmt::Let {
                id: recv_id,
                name: "__push_recv".to_string(),
                ty: ty.clone(),
                mutable: true,
                init: Some(Expr::LocalGet(old_id)),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: recv_id,
                value: Box::new(value),
            }),
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(Expr::LocalGet(recv_id)),
                    right: Box::new(Expr::LocalGet(old_id)),
                },
                then_branch: vec![Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: field.clone(),
                    value: Box::new(Expr::LocalGet(recv_id)),
                })],
                else_branch: None,
            },
        ];
        let n = rewritten.len();
        stmts.splice(i..i, rewritten);
        i += n;
    }
}

fn alloc_local(next_id: &mut LocalId) -> LocalId {
    let id = *next_id;
    *next_id += 1;
    id
}

/// `this.f.push(v)` as a statement, for an admissible field `f`.
fn field_push_candidate(stmt: &Stmt, fields: &[(String, Type)]) -> Option<(String, Type)> {
    let Stmt::Expr(Expr::NativeMethodCall {
        module,
        class_name: None,
        object: Some(object),
        method,
        args,
    }) = stmt
    else {
        return None;
    };
    if module != "array" || method != "push_single" || args.len() != 1 {
        return None;
    }
    let Expr::PropertyGet {
        object: recv,
        property,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    if !matches!(recv.as_ref(), Expr::This) {
        return None;
    }
    fields
        .iter()
        .find(|(name, _)| name == property)
        .map(|(name, ty)| (name.clone(), ty.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::ClassField;

    fn push_stmt(field: &str) -> Stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::PropertyGet {
                object: Box::new(Expr::This),
                property: field.to_string(),
                byte_offset: 0,
            })),
            method: "push_single".to_string(),
            args: vec![Expr::Number(1.0)],
        })
    }

    fn method(body: Vec<Stmt>) -> Function {
        Function {
            id: 1,
            name: "add".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn field(name: &str, ty: Type) -> ClassField {
        ClassField {
            name: name.to_string(),
            key_expr: None,
            ty,
            init: None,
            is_private: true,
            is_readonly: false,
            decorators: Vec::new(),
        }
    }

    fn class(fields: Vec<ClassField>, body: Vec<Stmt>) -> Class {
        Class {
            id: 1,
            name: "Buffer".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields,
            constructor: None,
            methods: vec![method(body)],
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        }
    }

    fn module_with(class: Class) -> Module {
        let mut m = Module::new("field_push_local_bind_test");
        m.classes.push(class);
        m
    }

    #[test]
    fn a_field_push_statement_binds_a_local_and_writes_back_only_on_realloc() {
        let mut m = module_with(class(
            vec![field("items", Type::Array(Box::new(Type::Number)))],
            vec![push_stmt("items")],
        ));
        run(&mut m);
        let body = &m.classes[0].methods[0].body;
        assert_eq!(body.len(), 4, "{body:?}");
        let (old_id, recv_id) = match (&body[0], &body[1]) {
            (
                Stmt::Let {
                    id: old,
                    init: Some(Expr::PropertyGet { property, .. }),
                    ..
                },
                Stmt::Let {
                    id: recv,
                    init: Some(Expr::LocalGet(from)),
                    mutable: true,
                    ..
                },
            ) => {
                assert_eq!(property, "items");
                assert_eq!(from, old);
                (*old, *recv)
            }
            other => panic!("expected the two receiver lets, got {other:?}"),
        };
        assert!(
            matches!(&body[2], Stmt::Expr(Expr::ArrayPush { array_id, .. }) if *array_id == recv_id),
            "the push must target the mutable receiver local: {:?}",
            body[2]
        );
        match &body[3] {
            Stmt::If {
                condition:
                    Expr::Compare {
                        op: CompareOp::Ne,
                        left,
                        right,
                    },
                then_branch,
                else_branch: None,
            } => {
                assert!(matches!(left.as_ref(), Expr::LocalGet(id) if *id == recv_id));
                assert!(matches!(right.as_ref(), Expr::LocalGet(id) if *id == old_id));
                assert!(matches!(
                    &then_branch[..],
                    [Stmt::Expr(Expr::PropertySet { property, value, .. })]
                        if property == "items"
                            && matches!(value.as_ref(), Expr::LocalGet(id) if *id == recv_id)
                ));
            }
            other => panic!("expected the guarded write-back, got {other:?}"),
        }
    }

    #[test]
    fn an_accessor_of_the_same_name_or_a_non_array_field_is_left_alone() {
        let mut with_getter = class(
            vec![field("items", Type::Array(Box::new(Type::Number)))],
            vec![push_stmt("items")],
        );
        with_getter
            .getters
            .push(("items".to_string(), method(Vec::new())));
        let mut m = module_with(with_getter);
        run(&mut m);
        assert_eq!(
            m.classes[0].methods[0].body.len(),
            1,
            "a getter-shadowed field must not be rewritten"
        );

        let mut m = module_with(class(
            vec![field("items", Type::Any)],
            vec![push_stmt("items")],
        ));
        run(&mut m);
        assert_eq!(
            m.classes[0].methods[0].body.len(),
            1,
            "an untyped field must not be rewritten"
        );

        let mut m = module_with(class(
            vec![field("items", Type::Array(Box::new(Type::Number)))],
            vec![push_stmt("other")],
        ));
        run(&mut m);
        assert_eq!(
            m.classes[0].methods[0].body.len(),
            1,
            "an undeclared field must not be rewritten"
        );
    }
}
