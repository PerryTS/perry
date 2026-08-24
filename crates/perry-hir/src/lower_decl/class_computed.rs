//! Helpers for lowering a class's computed-key members (methods / accessors)
//! declared inside a function body. Extracted from `body_stmt.rs` to keep that
//! file under the source-size gate.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::types::Type;

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

/// Evaluate and install every computed class-element name in one
/// ClassBody-ordered pass. Field names are stored in hidden class slots and
/// returned as inert reads for `ClassExprFresh` to copy onto its per-evaluation
/// class object. `ToPropertyKey` is part of the ordered evaluation so an
/// allocating/user-defined coercion cannot move after a later element.
pub(crate) fn prepare_ordered_class_computed_names(
    class_body: &[ast::ClassMember],
    class: &Class,
    registration_class_name: &str,
) -> (Vec<Expr>, Vec<(String, Expr)>) {
    let mut ordered: Vec<(usize, Expr)> = Vec::new();
    let mut field_keys = Vec::new();
    for (source_order, name, value) in super::computed_field_key_initializers_with_order(
        class_body,
        &class.fields,
        &class.static_fields,
    ) {
        ordered.push((
            source_order,
            Expr::StaticFieldSet {
                class_name: registration_class_name.to_string(),
                field_name: name.clone(),
                value: Box::new(value),
            },
        ));
        field_keys.push((
            name.clone(),
            Expr::PropertyGet {
                object: Box::new(Expr::ClassRef(registration_class_name.to_string())),
                property: name,
                byte_offset: 0,
            },
        ));
    }

    for member in &class.computed_members {
        let to_property_key = Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_to_property_key".to_string(),
                param_types: vec![Type::Any],
                return_type: Type::Any,
            }),
            args: vec![member.key_expr.clone()],
            type_args: Vec::new(),
            byte_offset: 0,
        };
        let mut resolved = member.clone();
        resolved.key_expr = to_property_key;
        ordered.push((
            member.source_order,
            class_computed_member_registration_expr(registration_class_name, &resolved),
        ));
    }
    ordered.sort_by_key(|(source_order, _)| *source_order);
    (
        ordered.into_iter().map(|(_, expr)| expr).collect(),
        field_keys,
    )
}

/// Reconstruct the source order of static fields and static blocks for the
/// `ClassExprFresh` codegen path. Computed-name evaluation remains a separate,
/// earlier phase as required by ClassDefinitionEvaluation.
pub(crate) fn fresh_class_static_init_order(
    class_body: &[ast::ClassMember],
    static_fields: &[ClassField],
) -> Vec<ClassFreshStaticInit> {
    let mut result = Vec::new();
    let mut static_field_index = 0usize;
    let mut named_index = 0u32;
    let mut computed_index = 0u32;
    let mut block_index = 0u32;
    for member in class_body {
        match member {
            ast::ClassMember::ClassProp(prop)
                if prop.is_static && !prop.declare && !prop.is_abstract =>
            {
                if let Some(field) = static_fields.get(static_field_index) {
                    if field.key_expr.is_some() {
                        result.push(ClassFreshStaticInit::Computed(computed_index));
                        computed_index += 1;
                    } else {
                        result.push(ClassFreshStaticInit::Named(named_index));
                        named_index += 1;
                    }
                }
                static_field_index += 1;
            }
            ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                result.push(ClassFreshStaticInit::Named(named_index));
                named_index += 1;
                static_field_index += 1;
            }
            ast::ClassMember::StaticBlock(_) => {
                result.push(ClassFreshStaticInit::Block(block_index));
                block_index += 1;
            }
            _ => {}
        }
    }
    result
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
