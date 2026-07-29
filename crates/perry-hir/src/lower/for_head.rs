//! `for-in` / `for-of` loop-head target resolution.
//!
//! Extracted from `lower/stmt_loops.rs` (2,000-LOC cap). Resolves every
//! legal loop-head shape — fresh decl bindings (ident + destructuring
//! patterns), bare-ident / member-expression assignment targets, and
//! destructuring-assignment heads — into a `ForHeadBinding` consumed by
//! the for-of / for-in desugars in `stmt_loops.rs` and
//! `lower_decl/body_stmt.rs`.

use crate::types::{LocalId, Type};
use anyhow::{anyhow, Result};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

/// Pre-resolved `for-in` / `for-of` head target. Built BEFORE the loop body
/// is lowered (so pattern leaves are in scope for body references), consumed
/// AFTER (to build the per-iteration binding statements).
pub(crate) enum ForHeadBinding {
    /// `for (var/let/const x …)` — fresh per-loop binding.
    DeclIdent { name: String, id: LocalId },
    /// `for (var/let/const [x, y] …)` / object patterns — leaves pre-defined.
    DeclPattern {
        pat: ast::Pat,
        var_ids: Vec<(String, LocalId)>,
    },
    /// `for (x …)` / `for ((x) …)` where `x` resolves to an existing
    /// binding — plain assignment each iteration (the binding leaks).
    AssignLocal { id: LocalId },
    /// `for (x.y …)` / `for (x[k] …)` — member store each iteration.
    AssignMember { member: ast::MemberExpr },
    /// `for ([a, b] …)` with pre-existing targets — destructuring
    /// assignment each iteration.
    AssignPattern { pat: ast::AssignTargetPat },
}

fn unwrap_parens_expr(mut e: &ast::Expr) -> &ast::Expr {
    while let ast::Expr::Paren(p) = e {
        e = &p.expr;
    }
    e
}

/// Phase A: resolve the head, defining any fresh bindings so the loop body
/// (lowered next) sees them. `elem_ty` types a simple decl-ident binding.
pub(crate) fn predefine_for_head(
    ctx: &mut LoweringContext,
    left: &ast::ForHead,
    elem_ty: Type,
) -> Result<ForHeadBinding> {
    match left {
        ast::ForHead::VarDecl(var_decl) => {
            let decl = var_decl
                .decls
                .first()
                .ok_or_else(|| anyhow!("for head requires a variable declaration"))?;
            match &decl.name {
                ast::Pat::Ident(ident) => {
                    let name = ident.id.sym.to_string();
                    let id = ctx.define_local(name.clone(), elem_ty);
                    if var_decl.kind == ast::VarDeclKind::Const {
                        // `for (const k in/of …) { k = 1; }` → TypeError.
                        ctx.mark_local_immutable(id);
                    }
                    Ok(ForHeadBinding::DeclIdent { name, id })
                }
                pat => {
                    let mut var_ids = Vec::new();
                    collect_for_of_pattern_leaves(ctx, pat, &mut var_ids);
                    Ok(ForHeadBinding::DeclPattern {
                        pat: pat.clone(),
                        var_ids,
                    })
                }
            }
        }
        ast::ForHead::Pat(pat) => match pat.as_ref() {
            ast::Pat::Ident(ident) => {
                let name = ident.id.sym.to_string();
                let id = ctx
                    .lookup_local(&name)
                    .unwrap_or_else(|| ctx.define_sloppy_implicit_global(name));
                Ok(ForHeadBinding::AssignLocal { id })
            }
            ast::Pat::Expr(expr) => match unwrap_parens_expr(expr) {
                ast::Expr::Ident(ident) => {
                    let name = ident.sym.to_string();
                    let id = ctx
                        .lookup_local(&name)
                        .unwrap_or_else(|| ctx.define_sloppy_implicit_global(name));
                    Ok(ForHeadBinding::AssignLocal { id })
                }
                ast::Expr::Member(member) => Ok(ForHeadBinding::AssignMember {
                    member: member.clone(),
                }),
                other => Err(anyhow!(
                    "Unsupported for-in/for-of head expression: {:?}",
                    std::mem::discriminant(other)
                )),
            },
            ast::Pat::Array(arr_pat) => Ok(ForHeadBinding::AssignPattern {
                pat: ast::AssignTargetPat::Array(arr_pat.clone()),
            }),
            ast::Pat::Object(obj_pat) => Ok(ForHeadBinding::AssignPattern {
                pat: ast::AssignTargetPat::Object(obj_pat.clone()),
            }),
            other => Err(anyhow!(
                "Unsupported for-in/for-of head pattern: {:?}",
                std::mem::discriminant(other)
            )),
        },
        _ => Err(anyhow!("Unsupported for-in/for-of left-hand side")),
    }
}

/// Phase B: build the per-iteration statements that bind/assign `source`
/// (the current key/element) into the head target.
pub(crate) fn for_head_binding_stmts(
    ctx: &mut LoweringContext,
    binding: &ForHeadBinding,
    source: Expr,
    elem_ty: Type,
) -> Result<Vec<Stmt>> {
    match binding {
        ForHeadBinding::DeclIdent { name, id } => Ok(vec![Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: elem_ty,
            mutable: false,
            init: Some(source),
        }]),
        ForHeadBinding::DeclPattern { pat, var_ids } => {
            let mut out = Vec::new();
            let mut var_idx = 0usize;
            // An array pattern iterates the bound value — for for-in keys
            // (strings) that means destructuring by code point. ForOfToArray
            // handles strings/arrays/iterables uniformly.
            let source = if matches!(pat, ast::Pat::Array(_)) {
                Expr::ForOfToArray(Box::new(source))
            } else {
                source
            };
            crate::lower::emit_for_of_pattern_binding(
                ctx,
                pat,
                source,
                var_ids,
                &mut var_idx,
                &mut out,
            )?;
            Ok(out)
        }
        ForHeadBinding::AssignLocal { id } => {
            Ok(vec![Stmt::Expr(Expr::LocalSet(*id, Box::new(source)))])
        }
        ForHeadBinding::AssignMember { member } => {
            let object = Box::new(lower_expr(ctx, &member.obj)?);
            let assign = match &member.prop {
                ast::MemberProp::Ident(prop) => Expr::PropertySet {
                    object,
                    property: prop.sym.to_string(),
                    value: Box::new(source),
                },
                ast::MemberProp::Computed(c) => Expr::IndexSet {
                    object,
                    index: Box::new(lower_expr(ctx, &c.expr)?),
                    value: Box::new(source),
                },
                ast::MemberProp::PrivateName(p) => {
                    // `for (o.#f of iter)` — assign each iteration value to the
                    // private field, brand-guarding the receiver (write op) so a
                    // receiver without the field throws TypeError per spec
                    // (test262 elements/privatefieldset-typeerror-6/7).
                    let property = format!("#{}", p.name);
                    let object = crate::lower::expr_member::wrap_private_guard(
                        ctx,
                        object,
                        &property,
                        crate::lower::expr_member::PRIV_OP_WRITE,
                    );
                    Expr::PropertySet {
                        object,
                        property,
                        value: Box::new(source),
                    }
                }
            };
            Ok(vec![Stmt::Expr(assign)])
        }
        ForHeadBinding::AssignPattern { pat } => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__forhead_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            let mut out = vec![Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            }];
            out.extend(
                crate::destructuring::lower_destructuring_assignment_stmt_from_local(
                    ctx, pat, tmp_id,
                )?,
            );
            Ok(out)
        }
    }
}

/// Wrap a desugared for-in loop body so a key that is deleted from the
/// receiver *before it is visited* is skipped, per ECMAScript for-in deletion
/// semantics (EnumerateObjectProperties: "If a property that has not yet been
/// visited during enumeration is deleted, then it will not be visited").
///
/// The keys are snapshotted once (`ForInKeys`), so without this guard a key
/// deleted mid-iteration would still be visited. `obj_id` holds the receiver
/// (spilled to a temp by the caller so it can be re-read each iteration),
/// `keys_id`/`idx_id` the snapshot array and cursor.
///
/// A primitive string is the only primitive whose for-in snapshot is non-empty
/// (its indices); its keys cannot be deleted, and the `in` operator *throws* on
/// a primitive receiver — so strings bypass the recheck and are always visited.
/// Objects/functions go through `key in obj`, which is `false` for a deleted
/// key and skips it. Nullish receivers never reach here (empty snapshot).
pub(crate) fn guard_for_in_body(
    obj_id: LocalId,
    keys_id: LocalId,
    idx_id: LocalId,
    body: Vec<Stmt>,
) -> Vec<Stmt> {
    let guard = Expr::Conditional {
        condition: Box::new(Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::TypeOf(Box::new(Expr::LocalGet(obj_id)))),
            right: Box::new(Expr::String("string".to_string())),
        }),
        then_expr: Box::new(Expr::Bool(true)),
        else_expr: Box::new(Expr::In {
            property: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(keys_id)),
                index: Box::new(Expr::LocalGet(idx_id)),
            }),
            object: Box::new(Expr::LocalGet(obj_id)),
        }),
    };
    vec![Stmt::If {
        condition: guard,
        then_branch: body,
        else_branch: None,
    }]
}

/// Build the delete-safe control for the Map/Set `for-of` fast path (#6075).
///
/// The fast path iterates the backing entries array by index (`arr_id` holds the
/// collection, `idx_id` the cursor). But `delete` compacts that array (entries
/// after the hole shift down), so deleting an entry at index ≤ the cursor moves
/// an unvisited entry below the cursor and skips it. This re-derives the read
/// index each iteration from the last-returned key, so a shift can't skip.
///
/// Returns `(init_lets, condition, body_prefix)`:
/// - `init_lets` — declare the state temps; push BEFORE the `for`.
/// - `condition` — replaces `idx < size`. Overwrites `idx_id` with the corrected
///   read index (plain cursor while the previously-read key is still at
///   `cursor-1`; else locate the last key — `+1` after it, or into its vacated
///   slot if it was itself deleted) and yields whether that index is in range.
/// - `body_prefix` — prepend to the loop body (runs after the in-range check):
///   records the visited key for the next iteration's in-place check.
///
/// `find` is O(1) for numeric/string keys and only called when a shift is
/// detected, so normal / append-only iteration keeps the plain cursor path.
pub(crate) fn map_set_delete_safe_for_of(
    ctx: &mut LoweringContext,
    arr_id: LocalId,
    idx_id: LocalId,
    is_set: bool,
) -> (Vec<Stmt>, Expr, Vec<Stmt>) {
    let lk_id = ctx.fresh_local(); // last-returned key
    let sz_id = ctx.fresh_local(); // current size (spilled once per iteration)
    let fk_id = ctx.fresh_local(); // find() result

    // `.size` on a Map/Set-typed receiver is codegen-recognized and lowered to
    // js_map_size / js_set_size (the raw `MapSize`/`SetSize` nodes are not
    // codegen expressions), matching the original loop bound.
    let size_of = |a: Expr| -> Expr {
        Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(a),
            property: "size".to_string(),
        }
    };
    let key_at = |a: Expr, i: Expr| -> Expr {
        if is_set {
            Expr::SetValueAt {
                set: Box::new(a),
                idx: Box::new(i),
            }
        } else {
            Expr::MapEntryKeyAt {
                map: Box::new(a),
                idx: Box::new(i),
            }
        }
    };
    let find_fn = if is_set {
        "js_set_find_value_index"
    } else {
        "js_map_find_key_index"
    };
    let find_call = |args: Vec<Expr>| -> Expr {
        Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: find_fn.to_string(),
                param_types: Vec::new(),
                return_type: Type::Number,
            }),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    };
    let cmp = |op: CompareOp, l: Expr, r: Expr| -> Expr {
        Expr::Compare {
            op,
            left: Box::new(l),
            right: Box::new(r),
        }
    };

    // cursor-1 (index of the entry read on the previous iteration).
    let prev_idx = |i: Expr| Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(i),
        right: Box::new(Expr::Number(1.0)),
    };

    // read_idx = cursor == 0                              // not started
    //            ? cursor
    //            : key_at(coll, cursor-1) === last_key    // last entry still in place
    //              ? cursor                               // no shift at/below cursor
    //              : (j = find(coll, last_key)) >= 0 ? j + 1 : cursor - 1
    //
    // Comparing the entry now at cursor-1 to the last-returned key detects ANY
    // delete that compacted an entry at/below the cursor — including a delete
    // balanced by an add in the same turn (which leaves `size` unchanged), which
    // a size-only gate would miss. Map/Set keys are unique under SameValueZero,
    // so `===` here is exact except for a NaN key (which just forces the `find`
    // path — still correct).
    let rederive = Expr::Conditional {
        condition: Box::new(cmp(
            CompareOp::Eq,
            Expr::LocalGet(idx_id),
            Expr::Number(0.0),
        )),
        then_expr: Box::new(Expr::LocalGet(idx_id)),
        else_expr: Box::new(Expr::Conditional {
            condition: Box::new(cmp(
                CompareOp::Eq,
                key_at(Expr::LocalGet(arr_id), prev_idx(Expr::LocalGet(idx_id))),
                Expr::LocalGet(lk_id),
            )),
            then_expr: Box::new(Expr::LocalGet(idx_id)),
            else_expr: Box::new(Expr::Sequence(vec![
                Expr::LocalSet(
                    fk_id,
                    Box::new(find_call(vec![
                        Expr::LocalGet(arr_id),
                        Expr::LocalGet(lk_id),
                    ])),
                ),
                Expr::Conditional {
                    // A delete only shifts entries down: a merely-shifted last key
                    // is now below the cursor (`0 <= j < cursor`) → resume after
                    // it. Deleted (`j < 0`) or deleted-then-re-added at the end
                    // (`j >= cursor`) → read the entry now in its old slot
                    // (`cursor - 1`).
                    condition: Box::new(cmp(
                        CompareOp::Ge,
                        Expr::LocalGet(fk_id),
                        Expr::Number(0.0),
                    )),
                    then_expr: Box::new(Expr::Conditional {
                        condition: Box::new(cmp(
                            CompareOp::Lt,
                            Expr::LocalGet(fk_id),
                            Expr::LocalGet(idx_id),
                        )),
                        then_expr: Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(fk_id)),
                            right: Box::new(Expr::Number(1.0)),
                        }),
                        else_expr: Box::new(prev_idx(Expr::LocalGet(idx_id))),
                    }),
                    else_expr: Box::new(prev_idx(Expr::LocalGet(idx_id))),
                },
            ])),
        }),
    };

    let condition = Expr::Sequence(vec![
        Expr::LocalSet(sz_id, Box::new(size_of(Expr::LocalGet(arr_id)))),
        Expr::LocalSet(idx_id, Box::new(rederive)),
        cmp(CompareOp::Lt, Expr::LocalGet(idx_id), Expr::LocalGet(sz_id)),
    ]);

    // Record the key just read so the next iteration can check it is still in
    // place at cursor-1.
    let body_prefix = vec![Stmt::Expr(Expr::LocalSet(
        lk_id,
        Box::new(key_at(Expr::LocalGet(arr_id), Expr::LocalGet(idx_id))),
    ))];

    let mk_let = |id: LocalId, tag: &str, ty: Type, init: Expr| Stmt::Let {
        id,
        name: format!("__miter_{}_{}", tag, id),
        ty,
        mutable: true,
        init: Some(init),
    };
    let init_lets = vec![
        mk_let(lk_id, "lk", Type::Any, Expr::Undefined),
        mk_let(sz_id, "sz", Type::Number, Expr::Number(0.0)),
        mk_let(fk_id, "fk", Type::Number, Expr::Number(0.0)),
    ];

    (init_lets, condition, body_prefix)
}
