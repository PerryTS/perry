//! `useDefineForClassFields: false` (TypeScript's default before ES2022).
//!
//! An instance field is then not defined on the instance. A field without an
//! initializer (`x: T`, `x!: T`, `x?: T`) creates nothing; an initializer is
//! an assignment (`this.x = v`, a [[Set]] that reaches setters on the
//! prototype chain) in the constructor, after `super()` and the parameter
//! properties, in source order. Both are constructor-store reservations: the
//! key appears at the store, as for `this.x = v` written in the constructor.
//! A static field without an initializer creates nothing either. The module
//! chooses this at compile time (`LoweringContext::assign_class_fields`);
//! `#x` fields and computed keys keep [[Define]], as TypeScript emits them.

use swc_common::{SyntaxContext, DUMMY_SP};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

use crate::ir::{ClassField, ClassFieldOrigin};
use crate::lower::LoweringContext;

/// The rest parameter of a synthesized derived constructor.
const FORWARDED_ARGS: &str = "__perry_field_args";

/// An instance field this module assigns in the constructor.
pub(crate) fn assigned_instance_field(ctx: &LoweringContext, prop: &ast::ClassProp) -> bool {
    ctx.assign_class_fields
        && !prop.is_static
        && !prop.declare
        && !prop.is_abstract
        && !matches!(prop.key, ast::PropName::Computed(_))
}

/// The assigned fields with an initializer, in source order.
pub(crate) fn assigned_field_inits<'a>(
    ctx: &LoweringContext,
    class: &'a ast::Class,
) -> Vec<&'a ast::ClassProp> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ast::ClassMember::ClassProp(prop)
                if prop.value.is_some() && assigned_instance_field(ctx, prop) =>
            {
                Some(prop)
            }
            _ => None,
        })
        .collect()
}

/// The class with the constructor TypeScript emits for it when it has none
/// but assigns field initializers: `constructor() {}`, or for a derived class
/// `constructor(...args) { super(...args); }`. `None` when nothing changes.
pub(crate) fn with_synthesized_constructor(
    ctx: &LoweringContext,
    class: &ast::Class,
) -> Option<ast::Class> {
    if class
        .body
        .iter()
        .any(|member| matches!(member, ast::ClassMember::Constructor(_)))
        || assigned_field_inits(ctx, class).is_empty()
    {
        return None;
    }
    let ident = || ast::Ident::new_no_ctxt(FORWARDED_ARGS.into(), DUMMY_SP);
    let (params, stmts) = if class.super_class.is_some() {
        let rest = ast::Pat::Rest(ast::RestPat {
            span: DUMMY_SP,
            dot3_token: DUMMY_SP,
            arg: Box::new(ast::Pat::Ident(ident().into())),
            type_ann: None,
        });
        let super_call = ast::Expr::Call(ast::CallExpr {
            span: DUMMY_SP,
            ctxt: SyntaxContext::empty(),
            callee: ast::Callee::Super(ast::Super { span: DUMMY_SP }),
            args: vec![ast::ExprOrSpread {
                spread: Some(DUMMY_SP),
                expr: Box::new(ast::Expr::Ident(ident())),
            }],
            type_args: None,
        });
        (
            vec![ast::ParamOrTsParamProp::Param(rest.into())],
            vec![ast::Stmt::Expr(ast::ExprStmt {
                span: DUMMY_SP,
                expr: Box::new(super_call),
            })],
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let constructor = ast::Constructor {
        span: class.span,
        ctxt: SyntaxContext::empty(),
        key: ast::PropName::Ident(ast::IdentName::new("constructor".into(), DUMMY_SP)),
        params,
        body: Some(ast::FunctionBody {
            stmts,
            ..Default::default()
        }),
        accessibility: None,
        is_optional: false,
    };
    let mut class = class.clone();
    class
        .body
        .insert(0, ast::ClassMember::Constructor(constructor));
    Some(class)
}

/// Keeps the constructor-store reservations the constructor stores, in the
/// order it creates their keys: parameter properties, assigned initializers,
/// then its top-level `this.x = v` statements. These are the reservations the
/// constructor scan finds for JavaScript; codegen predicts the completed
/// layout (`birth_lanes`) and a static family's keys from them. A field the
/// constructor does not store (`fileName!: string` set by a caller) is no
/// reservation: its key appears wherever it is first stored.
pub(crate) fn settle_reservations(
    ctx: &LoweringContext,
    class: &ast::Class,
    fields: &mut Vec<ClassField>,
) {
    if !ctx.assign_class_fields {
        return;
    }
    let mut order: Vec<String> = Vec::new();
    let ctor = class.body.iter().find_map(|member| match member {
        ast::ClassMember::Constructor(ctor) => Some(ctor),
        _ => None,
    });
    if let Some(ctor) = ctor {
        for param in &ctor.params {
            let ast::ParamOrTsParamProp::TsParamProp(prop) = param else {
                continue;
            };
            let ident = match &prop.param {
                ast::TsParamPropParam::Ident(ident) => Some(&ident.id),
                ast::TsParamPropParam::Assign(assign) => match assign.left.as_ref() {
                    ast::Pat::Ident(ident) => Some(&ident.id),
                    _ => None,
                },
            };
            order.extend(ident.map(|ident| ident.sym.to_string()));
        }
        order.extend(
            assigned_field_inits(ctx, class)
                .into_iter()
                .filter_map(|prop| key_name(&prop.key)),
        );
        for stmt in ctor.body.iter().flat_map(|body| &body.stmts) {
            if let ast::Stmt::Expr(stmt) = stmt {
                super::class_decl::collect_this_field_assigns(&stmt.expr, &mut order);
            }
        }
    }
    fields.retain(|field| {
        field.origin == ClassFieldOrigin::Definition || order.contains(&field.name)
    });
    fields.sort_by_key(|field| match field.origin {
        ClassFieldOrigin::Definition => 0,
        ClassFieldOrigin::Declared => usize::MAX,
        ClassFieldOrigin::ConstructorStore => order
            .iter()
            .position(|name| *name == field.name)
            .map_or(usize::MAX, |at| at + 1),
    });
}

/// A field key's name as `lower_class_prop` spells it.
fn key_name(key: &ast::PropName) -> Option<String> {
    match key {
        ast::PropName::Ident(ident) => Some(ident.sym.to_string()),
        ast::PropName::Str(s) => Some(s.value.as_str().unwrap_or("").to_string()),
        ast::PropName::Num(n) => Some(crate::lower::number_to_js_key(n.value)),
        ast::PropName::BigInt(b) => Some(b.value.to_string()),
        ast::PropName::Computed(_) => None,
    }
}

/// `this.<key> = <initializer>` for an assigned field with an initializer.
pub(crate) fn field_store(prop: &ast::ClassProp) -> Option<ast::Expr> {
    let span = prop.span;
    let computed = |lit: ast::Lit| {
        ast::MemberProp::Computed(ast::ComputedPropName {
            span,
            expr: Box::new(ast::Expr::Lit(lit)),
        })
    };
    let member = match &prop.key {
        ast::PropName::Ident(ident) => ast::MemberProp::Ident(ident.clone()),
        ast::PropName::Str(s) => computed(ast::Lit::Str(s.clone())),
        ast::PropName::Num(n) => computed(ast::Lit::Num(n.clone())),
        ast::PropName::BigInt(b) => computed(ast::Lit::BigInt(b.clone())),
        ast::PropName::Computed(_) => return None,
    };
    let target = ast::SimpleAssignTarget::Member(ast::MemberExpr {
        span,
        obj: Box::new(ast::Expr::This(ast::ThisExpr { span })),
        prop: member,
    });
    Some(ast::Expr::Assign(ast::AssignExpr {
        span,
        op: ast::AssignOp::Assign,
        left: ast::AssignTarget::Simple(target),
        right: prop.value.clone()?,
    }))
}

/// A static field without an initializer, which creates no property.
fn uninitialized_static(prop: &ast::ClassProp) -> bool {
    prop.is_static
        && prop.value.is_none()
        && !prop.declare
        && !matches!(prop.key, ast::PropName::Computed(_))
}

/// The module with every uninitialized static field marked `declare`, so
/// each class-body walk sees the same members. `None` when there is none.
pub(crate) fn declare_uninitialized_statics(module: &ast::Module) -> Option<ast::Module> {
    struct Find(bool);
    impl Visit for Find {
        fn visit_class_prop(&mut self, prop: &ast::ClassProp) {
            self.0 |= uninitialized_static(prop);
            prop.visit_children_with(self);
        }
    }
    struct Declare;
    impl VisitMut for Declare {
        fn visit_mut_class_prop(&mut self, prop: &mut ast::ClassProp) {
            if uninitialized_static(prop) {
                prop.declare = true;
            }
            prop.visit_mut_children_with(self);
        }
    }
    let mut find = Find(false);
    module.visit_with(&mut find);
    if !find.0 {
        return None;
    }
    let mut module = module.clone();
    module.visit_mut_with(&mut Declare);
    Some(module)
}
