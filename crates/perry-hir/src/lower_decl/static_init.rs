//! Interleaved static field/static-block init statement emission.
//!
//! Split out of `class_decl.rs` to stay under the file-size CI gate.

use swc_ecma_ast as ast;

use crate::ir::*;

fn to_property_key(key: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_to_property_key".to_string(),
            param_types: vec![crate::types::Type::Any],
            return_type: crate::types::Type::Any,
        }),
        args: vec![key],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// Hidden slot/value pairs for public computed fields in source order. The
/// values include the required ToPropertyKey coercion.
pub(crate) fn computed_field_key_initializers(
    class_body: &[ast::ClassMember],
    fields: &[ClassField],
    static_fields: &[ClassField],
) -> Vec<(String, Expr)> {
    let mut result = Vec::new();
    let mut field_idx = 0usize;
    let mut static_field_idx = 0usize;
    for member in class_body {
        match member {
            ast::ClassMember::ClassProp(prop) if !prop.declare && !prop.is_abstract => {
                let field = if prop.is_static {
                    let field = static_fields.get(static_field_idx);
                    static_field_idx += 1;
                    field
                } else {
                    let field = fields.get(field_idx);
                    field_idx += 1;
                    field
                };
                if let Some(field) = field {
                    if let Some(key) = field.key_expr.as_ref() {
                        result.push((field.name.clone(), to_property_key(key.clone())));
                    }
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                if prop.is_static {
                    static_field_idx += 1;
                } else {
                    field_idx += 1;
                }
            }
            _ => {}
        }
    }
    result
}

/// Per ClassDefinitionEvaluation step 34, a class's static fields and
/// static blocks evaluate in a single pass over source order — a static
/// block sequenced between two static fields must run between them, not
/// after every field has already been set. `static_fields` and
/// `static_methods` (which folds each `static { ... }` block in as a
/// synthetic `__perry_static_init_N` method, see the `StaticBlock` arm
/// above) are each individually in source order relative to their own
/// kind — the first pass over `class_body` appends to exactly one of the
/// two per relevant member, never reordering within a kind — so replaying
/// `class_body` and advancing one cursor per kind reconstructs the true
/// interleaving without matching by name (a computed-key field's `name` is
/// only a synthetic placeholder, see `ClassField::key_expr`).
///
/// Callers (module-level class decls, function-nested class decls, and
/// `var C = class { ... }`) previously emitted ALL static-field-init
/// statements before ANY static-block-call statement, so `static x = 1;
/// static { blockRan = true }; static y = 2;` ran block after both fields
/// instead of between them — test262
/// language/statements/class/static-init-sequence.js and
/// static-init-abrupt.js (a throw inside an earlier block must also skip
/// this now-later-positioned field).
pub(crate) fn build_interleaved_static_init_stmts(
    class_body: &[ast::ClassMember],
    class_name: &str,
    fields: &[ClassField],
    static_fields: &[ClassField],
    static_methods: &[Function],
) -> Vec<Stmt> {
    let emit_field = |out: &mut Vec<Stmt>, sf: &ClassField| {
        // A COMPUTED-key static field with no initializer still performs
        // CreateDataProperty(F, key, undefined) at class-eval time, so it must
        // be emitted (value = undefined) rather than dropped: (1) the key
        // expression has observable side effects, and (2) a key that evaluates
        // to "prototype" is a TypeError (test262 fields-computed-name-static-
        // *propname-prototype). A NON-computed uninit static field is a plain
        // named slot and keeps the pre-existing "skip when no init" behavior.
        if sf.init.is_none() && sf.key_expr.is_none() {
            return;
        }
        // `this` in a static field initializer is the class constructor.
        let mut init_value = sf.init.clone().unwrap_or(Expr::Undefined);
        crate::analysis::substitute_lexical_this_in_expr(
            &mut init_value,
            &Expr::ClassRef(class_name.to_string()),
        );
        out.push(if sf.key_expr.is_some() {
            Stmt::Expr(Expr::ClassStaticSymbolSet {
                class_name: class_name.to_string(),
                key: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::ClassRef(class_name.to_string())),
                    property: sf.name.clone(),
                    byte_offset: 0,
                }),
                value: Box::new(init_value),
            })
        } else {
            Stmt::Expr(Expr::StaticFieldSet {
                class_name: class_name.to_string(),
                field_name: sf.name.clone(),
                value: Box::new(init_value),
            })
        });
    };

    // ClassDefinitionEvaluation first evaluates every ComputedPropertyName in
    // source order. Keep the resolved keys on hidden static slots so static
    // initialization and each later instance construction reuse the same key.
    let mut out = Vec::new();
    for (field_name, value) in computed_field_key_initializers(class_body, fields, static_fields) {
        out.push(Stmt::Expr(Expr::StaticFieldSet {
            class_name: class_name.to_string(),
            field_name,
            value: Box::new(value),
        }));
    }

    // Static fields and blocks initialize only after all computed keys above
    // have been resolved. Their relative source order is still preserved.
    let mut field_idx = 0usize;
    let mut block_idx = 0usize;
    for member in class_body {
        match member {
            ast::ClassMember::ClassProp(prop) if !prop.declare && prop.is_static => {
                if let Some(sf) = static_fields.get(field_idx) {
                    emit_field(&mut out, sf);
                }
                field_idx += 1;
            }
            ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                if let Some(sf) = static_fields.get(field_idx) {
                    emit_field(&mut out, sf);
                }
                field_idx += 1;
            }
            ast::ClassMember::StaticBlock(_) => {
                let method_name = format!("__perry_static_init_{}", block_idx);
                block_idx += 1;
                if static_methods.iter().any(|m| m.name == method_name) {
                    out.push(Stmt::Expr(Expr::StaticMethodCall {
                        class_name: class_name.to_string(),
                        method_name,
                        args: Vec::new(),
                    }));
                }
            }
            _ => {}
        }
    }
    out
}
