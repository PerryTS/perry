use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

use super::*;

pub fn collect_pointer_typed_locals(
    params: &[perry_hir::Param],
    stmts: &[perry_hir::Stmt],
) -> std::collections::HashMap<u32, u32> {
    use perry_hir::Stmt;
    use perry_types::Type;
    fn is_ptr_typed(ty: &Type) -> bool {
        matches!(
            ty,
            Type::String
                | Type::Array(_)
                | Type::Tuple(_)
                | Type::Object(_)
                | Type::Named(_)
                | Type::Promise(_)
                | Type::Function(_)
                | Type::BigInt
                | Type::Any
                | Type::Unknown
        ) || matches!(ty, Type::Union(variants) if variants.iter().any(is_ptr_typed))
    }
    let mut out = std::collections::HashMap::new();
    let mut next_slot: u32 = 0;
    for p in params {
        if is_ptr_typed(&p.ty) {
            out.insert(p.id, next_slot);
            next_slot += 1;
        }
    }
    fn walk(stmts: &[Stmt], out: &mut std::collections::HashMap<u32, u32>, next_slot: &mut u32) {
        for s in stmts {
            match s {
                Stmt::Let { id, ty, .. } if is_ptr_typed(ty) => {
                    out.insert(*id, *next_slot);
                    *next_slot += 1;
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out, next_slot);
                    if let Some(eb) = else_branch {
                        walk(eb, out, next_slot);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    walk(body, out, next_slot);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        walk(std::slice::from_ref(i.as_ref()), out, next_slot);
                    }
                    walk(body, out, next_slot);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out, next_slot);
                    if let Some(c) = catch {
                        if let Some((id, _)) = &c.param {
                            // Catch parameter is implicitly bound;
                            // treat as Any (pointer-possible).
                            out.insert(*id, *next_slot);
                            *next_slot += 1;
                        }
                        walk(&c.body, out, next_slot);
                    }
                    if let Some(fb) = finally {
                        walk(fb, out, next_slot);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(&c.body, out, next_slot);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    walk(std::slice::from_ref(body.as_ref()), out, next_slot)
                }
                _ => {}
            }
        }
    }
    walk(stmts, &mut out, &mut next_slot);
    out
}
