//! #11743: expose a declared class-field array to the guarded append lane.
//!
//! Bind the receiver once, before its argument. Codegen's `push_field_single`
//! checks the actual receiver and resolves overrides before evaluating that
//! argument; the annotation selects a candidate, never a memory-layout proof.
//! No field write-back is needed: array growth preserves aliases by forwarding.
use std::collections::HashMap;

use perry_hir::types::{LocalId, Type};
use perry_hir::{Expr, Function, Module, Stmt};

use crate::closure_local_inline::nested_stmt_lists;

pub fn run(module: &mut Module) {
    let fields: HashMap<(String, String), Type> = module
        .classes
        .iter()
        .flat_map(|c| {
            c.fields.iter().filter_map(|f| {
                (f.key_expr.is_none() && matches!(f.ty, Type::Array(_)))
                    .then(|| ((c.name.clone(), f.name.clone()), f.ty.clone()))
            })
        })
        .collect();
    let mut next = crate::generator::compute_max_local_id(module).saturating_add(1);
    let mut init_types = HashMap::new();
    collect_types(&mut module.init, &mut init_types);
    rewrite(&mut module.init, &init_types, &fields, &mut next);
    for f in &mut module.functions {
        run_function(f, &fields, &mut next);
    }
    for c in &mut module.classes {
        for f in &mut c.methods {
            run_function(f, &fields, &mut next);
        }
    }
}

fn run_function(f: &mut Function, fields: &HashMap<(String, String), Type>, next: &mut LocalId) {
    if f.is_async || f.is_generator {
        return;
    }
    let mut types: HashMap<LocalId, Type> = f.params.iter().map(|p| (p.id, p.ty.clone())).collect();
    collect_types(&mut f.body, &mut types);
    rewrite(&mut f.body, &types, fields, next);
}

fn collect_types(body: &mut [Stmt], types: &mut HashMap<LocalId, Type>) {
    for s in body {
        if let Stmt::Let { id, ty, .. } = s {
            types.insert(*id, ty.clone());
        }
        for inner in nested_stmt_lists(s) {
            collect_types(inner, types);
        }
    }
}

fn rewrite(
    body: &mut Vec<Stmt>,
    types: &HashMap<LocalId, Type>,
    fields: &HashMap<(String, String), Type>,
    next: &mut LocalId,
) {
    for s in body.iter_mut() {
        for inner in nested_stmt_lists(s) {
            rewrite(inner, types, fields, next);
        }
    }
    let mut i = 0;
    while i < body.len() {
        let Some(ty) = candidate(&body[i], types, fields) else {
            i += 1;
            continue;
        };
        let Stmt::Expr(Expr::Call { callee, args, .. }) = body.remove(i) else {
            unreachable!();
        };
        let Expr::PropertyGet { object, .. } = *callee else {
            unreachable!();
        };
        let id = *next;
        *next += 1;
        body.splice(
            i..i,
            [
                Stmt::Let {
                    id,
                    name: "__field_push_receiver".into(),
                    ty,
                    mutable: true,
                    init: Some(*object),
                },
                Stmt::Expr(Expr::NativeMethodCall {
                    module: "array".into(),
                    class_name: None,
                    method: "push_field_single".into(),
                    object: Some(Box::new(Expr::LocalGet(id))),
                    args,
                }),
            ],
        );
        i += 2;
    }
}

fn candidate(
    stmt: &Stmt,
    types: &HashMap<LocalId, Type>,
    fields: &HashMap<(String, String), Type>,
) -> Option<Type> {
    let Stmt::Expr(Expr::Call { callee, args, .. }) = stmt else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    if property != "push" {
        return None;
    }
    let Expr::PropertyGet {
        object: owner,
        property: field,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(id) = owner.as_ref() else {
        return None;
    };
    let Type::Named(class) = types.get(id)? else {
        return None;
    };
    fields.get(&(class.clone(), field.clone())).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(owner: Expr, args: Vec<Expr>) -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::PropertyGet {
                    object: Box::new(owner),
                    property: "children".into(),
                    byte_offset: 0,
                }),
                property: "push".into(),
                byte_offset: 0,
            }),
            args,
            type_args: vec![],
            byte_offset: 0,
        })
    }

    #[test]
    fn original_field_spelling_binds_before_recursive_argument() {
        let types = HashMap::from([(1, Type::Named("DocNode".into()))]);
        let fields = HashMap::from([(
            ("DocNode".into(), "children".into()),
            Type::Array(Box::new(Type::Named("DocNode".into()))),
        )]);
        let value = Expr::Call {
            callee: Box::new(Expr::FuncRef(7)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        };
        let mut body = vec![call(Expr::LocalGet(1), vec![value.clone()])];
        rewrite(&mut body, &types, &fields, &mut 10);
        assert!(
            matches!(&body[0], Stmt::Let { id: 10, init: Some(Expr::PropertyGet { property, .. }), .. } if property == "children")
        );
        assert!(
            matches!(&body[1], Stmt::Expr(Expr::NativeMethodCall { method, object: Some(object), args, .. })
            if method == "push_field_single" && matches!(object.as_ref(), Expr::LocalGet(10)) && args.len() == 1 && format!("{:?}", args[0]) == format!("{value:?}"))
        );
    }

    #[test]
    fn unknown_fields_nonclass_receivers_and_multiple_arguments_decline() {
        let types = HashMap::from([(1, Type::Any)]);
        let fields = HashMap::from([(
            ("DocNode".into(), "children".into()),
            Type::Array(Box::new(Type::Any)),
        )]);
        assert!(candidate(
            &call(Expr::LocalGet(1), vec![Expr::Number(1.0)]),
            &types,
            &fields
        )
        .is_none());
        let types = HashMap::from([(1, Type::Named("DocNode".into()))]);
        assert!(candidate(
            &call(
                Expr::LocalGet(1),
                vec![Expr::Number(1.0), Expr::Number(2.0)]
            ),
            &types,
            &fields
        )
        .is_none());
        assert!(candidate(&call(Expr::This, vec![Expr::Number(1.0)]), &types, &fields).is_none());
        assert!(candidate(
            &call(Expr::LocalGet(1), vec![Expr::Number(1.0)]),
            &types,
            &HashMap::new()
        )
        .is_none());
    }
}
