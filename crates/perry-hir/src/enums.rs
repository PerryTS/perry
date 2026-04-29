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
            _ => {}
        }
    }
}

pub(crate) fn fix_imported_enums_in_expr(
    expr: &mut Expr,
    enums: &BTreeMap<String, Vec<(String, EnumValue)>>,
) {
    match expr {
        // The key pattern: PropertyGet on an ExternFuncRef that's actually an enum
        Expr::PropertyGet { object, property } => {
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
        Expr::PropertySet { object, value, .. } => {
            fix_imported_enums_in_expr(object, enums);
            fix_imported_enums_in_expr(value, enums);
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            fix_imported_enums_in_expr(left, enums);
            fix_imported_enums_in_expr(right, enums);
        }
        Expr::Unary { operand, .. } => {
            fix_imported_enums_in_expr(operand, enums);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fix_imported_enums_in_expr(condition, enums);
            fix_imported_enums_in_expr(then_expr, enums);
            fix_imported_enums_in_expr(else_expr, enums);
        }
        Expr::Call { callee, args, .. } => {
            fix_imported_enums_in_expr(callee, enums);
            for arg in args {
                fix_imported_enums_in_expr(arg, enums);
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                fix_imported_enums_in_expr(elem, enums);
            }
        }
        Expr::IndexGet { object, index } => {
            fix_imported_enums_in_expr(object, enums);
            fix_imported_enums_in_expr(index, enums);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fix_imported_enums_in_expr(object, enums);
            fix_imported_enums_in_expr(index, enums);
            fix_imported_enums_in_expr(value, enums);
        }
        Expr::Object(fields) => {
            for (_, value) in fields {
                fix_imported_enums_in_expr(value, enums);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                fix_imported_enums_in_expr(value, enums);
            }
        }
        Expr::LocalSet(_, value) => {
            fix_imported_enums_in_expr(value, enums);
        }
        Expr::Closure { body, .. } => {
            fix_imported_enums_in_stmts(body, enums);
        }
        Expr::NativeMethodCall { args, .. } => {
            for arg in args {
                fix_imported_enums_in_expr(arg, enums);
            }
        }
        Expr::New { args, .. } => {
            for arg in args {
                fix_imported_enums_in_expr(arg, enums);
            }
        }
        Expr::Await(inner) | Expr::TypeOf(inner) => {
            fix_imported_enums_in_expr(inner, enums);
        }
        _ => {}
    }
}
