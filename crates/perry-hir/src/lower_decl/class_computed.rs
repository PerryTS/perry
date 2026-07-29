//! Helpers for lowering a class's computed-key members (methods / accessors)
//! declared inside a function body. Extracted from `body_stmt.rs` to keep that
//! file under the source-size gate.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};

/// Build the registration expression for one computed-key class member
/// (`[expr]() {}` / `get [expr]() {}` / `set [expr](v) {}`). Codegen lowers
/// these to `js_register_class_computed_{method,accessor}` calls that evaluate
/// the key at the class-definition site.
pub(crate) fn class_computed_member_registration_expr(
    class_name: &str,
    member: &ClassComputedMember,
) -> Expr {
    match member.kind {
        ClassComputedMemberKind::Method => Expr::RegisterClassComputedMethod {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            method_name: member.function.name.clone(),
            is_static: member.is_static,
            param_count: member.function.params.len() as u32,
            has_rest: member
                .function
                .params
                .last()
                .map(|p| p.is_rest)
                .unwrap_or(false),
        },
        ClassComputedMemberKind::Getter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: Some(member.function.name.clone()),
            setter_name: None,
            is_static: member.is_static,
        },
        ClassComputedMemberKind::Setter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: None,
            setter_name: Some(member.function.name.clone()),
            is_static: member.is_static,
        },
    }
}

/// A class declared inside a function body is name-deduped against an earlier
/// same-named class (Perry's codegen is name-keyed; #336). But ECMA-262
/// ClassDefinitionEvaluation still evaluates every `class` expression's
/// ComputedPropertyName in source order, so a computed member key with side
/// effects (a throw, an assignment, a call) must still run — e.g. two
/// `assert.throws(() => { class C { set [unresolvable](_) {} } })` helpers both
/// named `C` (Test262 accessor-name-*/computed-err). Evaluate just the key
/// expressions (applying `ToPropertyKey`); the duplicate class body stays
/// deduped.
pub(crate) fn push_deduped_class_computed_keys(
    ctx: &mut LoweringContext,
    class: &ast::Class,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    for member in &class.body {
        let computed_key = match member {
            ast::ClassMember::Method(m) => match &m.key {
                ast::PropName::Computed(c) => Some(c.expr.as_ref()),
                _ => None,
            },
            ast::ClassMember::ClassProp(p) => match &p.key {
                ast::PropName::Computed(c) => Some(c.expr.as_ref()),
                _ => None,
            },
            _ => None,
        };
        if let Some(key_ast) = computed_key {
            let lowered = lower_expr(ctx, key_ast)?;
            // ComputedPropertyName is `ToPropertyKey(GetValue(eval))` — apply
            // ToPropertyKey too so a non-primitive key with no callable
            // toString/valueOf (e.g. `Object.create(null)`) throws TypeError,
            // matching the non-deduped registration path (Test262
            // computed-err-to-prop-key).
            result.push(Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::ExternFuncRef {
                    name: "js_to_property_key".to_string(),
                    param_types: vec![Type::Any],
                    return_type: Type::Any,
                }),
                args: vec![lowered],
                type_args: Vec::new(),
                byte_offset: 0,
            }));
        }
    }
    Ok(())
}
