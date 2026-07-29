//! Enum import fixup pass.
//!
//! Contains `fix_imported_enums` and related functions that resolve
//! imported enum member references after HIR lowering.

use std::collections::BTreeMap;

use crate::ir::*;

pub fn fix_imported_enums(
    module: &mut Module,
    imported_enums: &BTreeMap<String, Vec<(String, EnumValue)>>,
) {
    if imported_enums.is_empty() {
        return;
    }
    // Fix expressions in functions
    for func in &mut module.functions {
        fix_imported_enums_in_stmts(&mut func.body, imported_enums);
    }
    // Fix expressions in class methods and constructors
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            fix_imported_enums_in_stmts(&mut ctor.body, imported_enums);
        }
        for method in &mut class.methods {
            fix_imported_enums_in_stmts(&mut method.body, imported_enums);
        }
        // #6879: field initializers and computed keys are expression sites too
        // (`class P { kind = TokenKind.Ident }`), and were never visited.
        for field in &mut class.fields {
            if let Some(init) = field.init.as_mut() {
                fix_imported_enums_in_expr(init, imported_enums);
            }
            if let Some(key_expr) = field.key_expr.as_mut() {
                fix_imported_enums_in_expr(key_expr, imported_enums);
            }
        }
    }
    // Fix expressions in module init
    fix_imported_enums_in_stmts(&mut module.init, imported_enums);
}

pub(crate) fn fix_imported_enums_in_stmts(
    stmts: &mut Vec<Stmt>,
    enums: &BTreeMap<String, Vec<(String, EnumValue)>>,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => fix_imported_enums_in_expr(expr, enums),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                fix_imported_enums_in_expr(expr, enums);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                fix_imported_enums_in_expr(condition, enums);
                fix_imported_enums_in_stmts(then_branch, enums);
                if let Some(else_b) = else_branch {
                    fix_imported_enums_in_stmts(else_b, enums);
                }
            }
            Stmt::While { condition, body } => {
                fix_imported_enums_in_expr(condition, enums);
                fix_imported_enums_in_stmts(body, enums);
            }
            // #6879: `do { … } while (…)` and labeled statements were reached
            // by the old `_ => {}` catch-all and silently skipped.
            Stmt::DoWhile { body, condition } => {
                fix_imported_enums_in_stmts(body, enums);
                fix_imported_enums_in_expr(condition, enums);
            }
            Stmt::Labeled { body, .. } => {
                let mut one = vec![(**body).clone()];
                fix_imported_enums_in_stmts(&mut one, enums);
                if one.len() == 1 {
                    **body = one.remove(0);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut v = vec![*init_stmt.clone()];
                    fix_imported_enums_in_stmts(&mut v, enums);
                    if v.len() == 1 {
                        **init_stmt = v.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    fix_imported_enums_in_expr(cond, enums);
                }
                if let Some(upd) = update {
                    fix_imported_enums_in_expr(upd, enums);
                }
                fix_imported_enums_in_stmts(body, enums);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                fix_imported_enums_in_expr(discriminant, enums);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        fix_imported_enums_in_expr(test, enums);
                    }
                    fix_imported_enums_in_stmts(&mut case.body, enums);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                fix_imported_enums_in_stmts(body, enums);
                if let Some(catch_clause) = catch {
                    fix_imported_enums_in_stmts(&mut catch_clause.body, enums);
                }
                if let Some(finally_stmts) = finally {
                    fix_imported_enums_in_stmts(finally_stmts, enums);
                }
            }
            // Exhaustive on purpose (#6879): a catch-all here is what let
            // `DoWhile` / `Labeled` fall through unnoticed. Adding a new
            // `Stmt` variant should be a compile error, not a silent miss.
            Stmt::Let { init: None, .. }
            | Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

pub(crate) fn fix_imported_enums_in_expr(
    expr: &mut Expr,
    enums: &BTreeMap<String, Vec<(String, EnumValue)>>,
) {
    match expr {
        // The key pattern: PropertyGet on an ExternFuncRef that's actually an enum
        Expr::PropertyGet {
            object, property, ..
        } => {
            if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                if let Some(members) = enums.get(name.as_str()) {
                    // Look up the member value
                    if let Some((_, value)) = members.iter().find(|(n, _)| n == property.as_str()) {
                        // For string enums, inline the string value directly
                        // so it's recognized by is_string_expr throughout codegen
                        match value {
                            EnumValue::String(s) => {
                                *expr = Expr::String(s.clone());
                            }
                            _ => {
                                *expr = Expr::EnumMember {
                                    enum_name: name.clone(),
                                    member_name: property.clone(),
                                };
                            }
                        }
                    } else {
                        // Unknown member, still replace to avoid ExternFuncRef property access
                        *expr = Expr::EnumMember {
                            enum_name: name.clone(),
                            member_name: property.clone(),
                        };
                    }
                    return;
                }
            }
            fix_imported_enums_in_expr(object, enums);
        }
        // #6879: everything else just needs generic descent. The hand-rolled
        // match this replaces ended in `_ => {}`, so any variant it did not
        // list stopped the walk dead — including `StringCoerce`, which wraps
        // `String(x)` and every template-literal interpolation. That single
        // gap made `String(TokenKind.Ident)`, `` `${TokenKind.Ident}` ``,
        // `[TokenKind.Ident][0]` and `{ a: TokenKind.Ident }.a` all read
        // `undefined`, while the unwrapped forms (`const k = …`, a user-fn
        // argument, `"" + …`) worked — exactly the bug class
        // `crate::walker`'s module docs describe. Delegating to the shared,
        // compiler-enforced walker means a new `Expr` variant can never
        // reintroduce it.
        _ => {
            // Closure bodies are `Vec<Stmt>`; the shared walker deliberately
            // does not descend into them (see walker module docs), so do it
            // here. Param defaults are covered by the walker itself.
            if let Expr::Closure { body, .. } = expr {
                fix_imported_enums_in_stmts(body, enums);
            }
            crate::walker::walk_expr_children_mut(expr, &mut |child| {
                fix_imported_enums_in_expr(child, enums);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    fn token_kind() -> BTreeMap<String, Vec<(String, EnumValue)>> {
        let mut m = BTreeMap::new();
        m.insert(
            "TokenKind".to_string(),
            vec![("Ident".to_string(), EnumValue::String("IDENT".to_string()))],
        );
        m
    }

    /// `TokenKind.Ident` as it lowers in a module that IMPORTED the enum: a
    /// `PropertyGet` on an `ExternFuncRef`, which this pass rewrites.
    fn imported_member(enum_name: &str, member: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::ExternFuncRef {
                name: enum_name.to_string(),
                param_types: vec![],
                return_type: Type::Any,
            }),
            property: member.to_string(),
            byte_offset: 0,
        }
    }

    fn ident_member() -> Expr {
        imported_member("TokenKind", "Ident")
    }

    /// `Expr` has no `PartialEq`, so compare the derived Debug rendering.
    fn shape(expr: &Expr) -> String {
        format!("{expr:?}")
    }

    fn fixed(expr: Expr) -> Expr {
        let mut e = expr;
        fix_imported_enums_in_expr(&mut e, &token_kind());
        e
    }

    /// The bare form always worked; lock it in.
    #[test]
    fn rewrites_bare_imported_enum_member() {
        assert_eq!(
            shape(&fixed(ident_member())),
            shape(&Expr::String("IDENT".to_string()))
        );
    }

    /// #6879: `String(x)` and every template-literal interpolation wrap the
    /// access in `StringCoerce`, which the old hand-rolled walker did not
    /// list — its `_ => {}` arm stopped the walk and the member read
    /// `undefined`.
    #[test]
    fn descends_through_string_coerce() {
        assert_eq!(
            shape(&fixed(Expr::StringCoerce(Box::new(ident_member())))),
            shape(&Expr::StringCoerce(Box::new(Expr::String(
                "IDENT".to_string()
            ))))
        );
    }

    /// `String([TokenKind.Ident][0])`. Array/IndexGet were listed by the old
    /// walker but unreachable behind the unlisted StringCoerce wrapper.
    #[test]
    fn descends_through_string_coerce_into_array() {
        let got = fixed(Expr::StringCoerce(Box::new(Expr::IndexGet {
            object: Box::new(Expr::Array(vec![ident_member()])),
            index: Box::new(Expr::Integer(0)),
        })));
        let want = Expr::StringCoerce(Box::new(Expr::IndexGet {
            object: Box::new(Expr::Array(vec![Expr::String("IDENT".to_string())])),
            index: Box::new(Expr::Integer(0)),
        }));
        assert_eq!(shape(&got), shape(&want));
    }

    /// A numeric enum member becomes `EnumMember`, not an inlined string —
    /// still true when reached through the generic descent.
    #[test]
    fn numeric_member_becomes_enum_member() {
        let mut enums = BTreeMap::new();
        enums.insert(
            "Color".to_string(),
            vec![("Red".to_string(), EnumValue::Number(2))],
        );
        let mut got = Expr::StringCoerce(Box::new(imported_member("Color", "Red")));
        fix_imported_enums_in_expr(&mut got, &enums);
        let want = Expr::StringCoerce(Box::new(Expr::EnumMember {
            enum_name: "Color".to_string(),
            member_name: "Red".to_string(),
        }));
        assert_eq!(shape(&got), shape(&want));
    }

    /// A PropertyGet on something that is not an imported enum is left alone,
    /// and the walk still descends into its receiver.
    #[test]
    fn leaves_unrelated_property_gets_alone() {
        let got = fixed(Expr::PropertyGet {
            object: Box::new(Expr::StringCoerce(Box::new(ident_member()))),
            property: "length".to_string(),
            byte_offset: 0,
        });
        let want = Expr::PropertyGet {
            object: Box::new(Expr::StringCoerce(Box::new(Expr::String(
                "IDENT".to_string(),
            )))),
            property: "length".to_string(),
            byte_offset: 0,
        };
        assert_eq!(shape(&got), shape(&want));
    }

    /// #6879: `do { … } while (…)` was skipped by the statement walker's
    /// catch-all.
    #[test]
    fn descends_into_do_while() {
        let mut stmts = vec![Stmt::DoWhile {
            body: vec![Stmt::Expr(ident_member())],
            condition: ident_member(),
        }];
        fix_imported_enums_in_stmts(&mut stmts, &token_kind());
        let want = Stmt::DoWhile {
            body: vec![Stmt::Expr(Expr::String("IDENT".to_string()))],
            condition: Expr::String("IDENT".to_string()),
        };
        assert_eq!(format!("{:?}", stmts[0]), format!("{want:?}"));
    }

    /// #6879: labeled statements were skipped by the same catch-all.
    #[test]
    fn descends_into_labeled() {
        let mut stmts = vec![Stmt::Labeled {
            label: "outer".to_string(),
            body: Box::new(Stmt::Expr(ident_member())),
        }];
        fix_imported_enums_in_stmts(&mut stmts, &token_kind());
        let want = Stmt::Labeled {
            label: "outer".to_string(),
            body: Box::new(Stmt::Expr(Expr::String("IDENT".to_string()))),
        };
        assert_eq!(format!("{:?}", stmts[0]), format!("{want:?}"));
    }
}
