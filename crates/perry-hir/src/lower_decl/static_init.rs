//! Interleaved static field/static-block init statement emission.
//!
//! Split out of `class_decl.rs` to stay under the file-size CI gate.

use swc_ecma_ast as ast;

use crate::ir::*;

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
    static_fields: &[ClassField],
    static_methods: &[Function],
) -> Vec<Stmt> {
    let emit_field = |out: &mut Vec<Stmt>, sf: &ClassField| {
        let Some(init) = &sf.init else { return };
        // `this` in a static field initializer is the class constructor.
        let mut init_value = init.clone();
        crate::analysis::substitute_lexical_this_in_expr(
            &mut init_value,
            &Expr::ClassRef(class_name.to_string()),
        );
        out.push(if let Some(key) = sf.key_expr.as_ref() {
            Stmt::Expr(Expr::ClassStaticSymbolSet {
                class_name: class_name.to_string(),
                key: Box::new(key.clone()),
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

    let mut out = Vec::new();
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
