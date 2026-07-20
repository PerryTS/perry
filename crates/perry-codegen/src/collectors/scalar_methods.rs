//! Conservative method summaries used by scalar replacement.
//!
//! This is intentionally much narrower than Perry's eventual effect-summary
//! system. It only admits own, fixed-arity, synchronous methods whose entire
//! body is either:
//! - `return <numeric expression>` over numeric parameters, numeric literals,
//!   and direct `this.field` reads of public numeric fields; or
//! - `return <Int32 expression>` over public Int32 fields/params/in-range
//!   integer literals, signed bitwise binary operators, and immutable local
//!   temporaries built from those expressions; or
//! - `return <numeric expression> <cmp> <numeric expression>` for boolean
//!   predicates over the same safe numeric expression subset.

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, Class, CompareOp, Expr, Function, Stmt, UnaryOp};
use perry_types::Type;

#[derive(Clone, Copy)]
enum ScalarMethodReturnKind {
    Numeric,
    Int32,
    Boolean,
}

pub(crate) fn simple_scalar_method_summary<'a>(
    classes: &'a HashMap<String, &'a Class>,
    class_name: &str,
    method_name: &str,
    arg_count: usize,
) -> Option<&'a Function> {
    let class = classes.get(class_name).copied()?;
    let method = class.methods.iter().find(|m| m.name == method_name)?;
    if !is_simple_scalar_method(classes, class_name, method, arg_count) {
        return None;
    }
    Some(method)
}

pub(crate) fn is_simple_scalar_method(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    method: &Function,
    arg_count: usize,
) -> bool {
    if method.is_async
        || method.is_generator
        || method.was_plain_async
        || !method.captures.is_empty()
        || !method.decorators.is_empty()
        || method.params.len() != arg_count
    {
        return false;
    }
    let Some(return_kind) = scalar_method_return_kind(&method.return_type) else {
        return false;
    };
    if class_declares_or_writes_own_property(classes, class_name, &method.name) {
        return false;
    }

    let mut numeric_locals = HashSet::new();
    for param in &method.params {
        let param_type_is_safe = match return_kind {
            ScalarMethodReturnKind::Int32 => is_int32_type(&param.ty),
            ScalarMethodReturnKind::Numeric | ScalarMethodReturnKind::Boolean => {
                is_numeric_type(&param.ty)
            }
        };
        if param.default.is_some()
            || param.is_rest
            || param.arguments_object.is_some()
            || !param.decorators.is_empty()
            || !param_type_is_safe
        {
            return false;
        }
        numeric_locals.insert(param.id);
    }

    let Some((return_expr, local_temps)) = scalar_method_straight_line_return(method, return_kind)
    else {
        return false;
    };
    for (id, init) in local_temps {
        if !scalar_method_return_expr_is_safe(
            classes,
            class_name,
            init,
            &numeric_locals,
            return_kind,
        ) {
            return false;
        }
        numeric_locals.insert(id);
    }
    scalar_method_return_expr_is_safe(
        classes,
        class_name,
        return_expr,
        &numeric_locals,
        return_kind,
    )
}

fn scalar_method_straight_line_return<'a>(
    method: &'a Function,
    return_kind: ScalarMethodReturnKind,
) -> Option<(&'a Expr, Vec<(u32, &'a Expr)>)> {
    let mut local_temps = Vec::new();
    for (idx, stmt) in method.body.iter().enumerate() {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable,
                init: Some(init),
                ..
            } if !*mutable && scalar_method_temp_type_is_safe(ty, return_kind) => {
                local_temps.push((*id, init));
            }
            Stmt::Return(Some(expr)) if idx + 1 == method.body.len() => {
                return Some((expr, local_temps));
            }
            _ => return None,
        }
    }
    None
}

fn scalar_method_temp_type_is_safe(ty: &Type, return_kind: ScalarMethodReturnKind) -> bool {
    match return_kind {
        ScalarMethodReturnKind::Int32 => is_int32_type(ty),
        ScalarMethodReturnKind::Numeric | ScalarMethodReturnKind::Boolean => is_numeric_type(ty),
    }
}

fn scalar_method_return_kind(ty: &Type) -> Option<ScalarMethodReturnKind> {
    if matches!(ty, Type::Int32) {
        Some(ScalarMethodReturnKind::Int32)
    } else if matches!(ty, Type::Number) {
        Some(ScalarMethodReturnKind::Numeric)
    } else if matches!(ty, Type::Boolean) {
        Some(ScalarMethodReturnKind::Boolean)
    } else {
        None
    }
}

fn scalar_method_return_expr_is_safe(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    expr: &Expr,
    numeric_params: &HashSet<u32>,
    return_kind: ScalarMethodReturnKind,
) -> bool {
    match return_kind {
        ScalarMethodReturnKind::Numeric => {
            numeric_scalar_method_expr_is_safe(classes, class_name, expr, numeric_params)
        }
        ScalarMethodReturnKind::Int32 => {
            int32_scalar_method_expr_is_safe(classes, class_name, expr, numeric_params)
        }
        ScalarMethodReturnKind::Boolean => {
            boolean_scalar_method_expr_is_safe(classes, class_name, expr, numeric_params)
        }
    }
}

fn boolean_scalar_method_expr_is_safe(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    expr: &Expr,
    numeric_params: &HashSet<u32>,
) -> bool {
    match expr {
        Expr::Compare { op, left, right } => {
            matches!(
                op,
                CompareOp::Eq
                    | CompareOp::Ne
                    | CompareOp::Lt
                    | CompareOp::Le
                    | CompareOp::Gt
                    | CompareOp::Ge
            ) && numeric_scalar_method_expr_is_safe(classes, class_name, left, numeric_params)
                && numeric_scalar_method_expr_is_safe(classes, class_name, right, numeric_params)
        }
        _ => false,
    }
}

fn numeric_scalar_method_expr_is_safe(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    expr: &Expr,
    numeric_params: &HashSet<u32>,
) -> bool {
    match expr {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) => numeric_params.contains(id),
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Pos | UnaryOp::Neg)
                && numeric_scalar_method_expr_is_safe(classes, class_name, operand, numeric_params)
        }
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
            ) && numeric_scalar_method_expr_is_safe(classes, class_name, left, numeric_params)
                && numeric_scalar_method_expr_is_safe(classes, class_name, right, numeric_params)
        }
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            public_numeric_field(classes, class_name, property)
        }
        _ => false,
    }
}

fn int32_scalar_method_expr_is_safe(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    expr: &Expr,
    numeric_params: &HashSet<u32>,
) -> bool {
    match expr {
        Expr::Integer(value) => i32::try_from(*value).is_ok(),
        Expr::LocalGet(id) => numeric_params.contains(id),
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
            ) && int32_scalar_method_expr_is_safe(classes, class_name, left, numeric_params)
                && int32_scalar_method_expr_is_safe(classes, class_name, right, numeric_params)
        }
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            public_int32_field(classes, class_name, property)
        }
        _ => false,
    }
}

fn is_numeric_type(ty: &Type) -> bool {
    matches!(ty, Type::Number | Type::Int32)
}

fn is_int32_type(ty: &Type) -> bool {
    matches!(ty, Type::Int32)
}

fn public_numeric_field(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    field_name: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    let mut seen = HashSet::new();
    let mut depth = 0usize;
    while let Some(name) = current {
        depth += 1;
        if depth > 64 || !seen.insert(name.clone()) {
            return false;
        }
        let Some(class) = classes.get(&name).copied() else {
            return false;
        };
        if class.getters.iter().any(|(name, _)| name == field_name)
            || class.setters.iter().any(|(name, _)| name == field_name)
        {
            return false;
        }
        if class.fields.iter().any(|field| {
            field.key_expr.is_none()
                && !field.is_private
                && field.name == field_name
                && is_numeric_type(&field.ty)
        }) {
            return true;
        }
        current = class.extends_name.clone();
    }
    false
}

fn public_int32_field(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    field_name: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    let mut seen = HashSet::new();
    let mut depth = 0usize;
    while let Some(name) = current {
        depth += 1;
        if depth > 64 || !seen.insert(name.clone()) {
            return false;
        }
        let Some(class) = classes.get(&name).copied() else {
            return false;
        };
        if class.getters.iter().any(|(name, _)| name == field_name)
            || class.setters.iter().any(|(name, _)| name == field_name)
        {
            return false;
        }
        if class.fields.iter().any(|field| {
            field.key_expr.is_none()
                && !field.is_private
                && field.name == field_name
                && is_int32_type(&field.ty)
        }) {
            return true;
        }
        current = class.extends_name.clone();
    }
    false
}

fn class_declares_or_writes_own_property(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut current = Some(class_name.to_string());
    let mut seen = HashSet::new();
    let mut depth = 0usize;
    let mut receiver_class = true;
    while let Some(name) = current {
        depth += 1;
        if depth > 64 || !seen.insert(name.clone()) {
            return true;
        }
        let Some(class) = classes.get(&name).copied() else {
            return true;
        };
        if class
            .fields
            .iter()
            .any(|field| field.key_expr.is_some() || (!field.is_private && field.name == property))
            || class
                .constructor
                .as_ref()
                .is_some_and(|ctor| stmts_write_this_property(&ctor.body, property))
        {
            return true;
        }
        if receiver_class
            && (class.getters.iter().any(|(name, _)| name == property)
                || class.setters.iter().any(|(name, _)| name == property)
                || class
                    .computed_members
                    .iter()
                    .any(|member| !member.is_static))
        {
            return true;
        }
        receiver_class = false;
        current = class.extends_name.clone();
    }
    false
}

fn stmts_write_this_property(stmts: &[Stmt], property: &str) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_writes_this_property(stmt, property))
}

fn stmt_writes_this_property(stmt: &Stmt, property: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Throw(expr) => expr_writes_this_property(expr, property),
        Stmt::Return(Some(expr)) => expr_writes_this_property(expr, property),
        Stmt::Let {
            init: Some(expr), ..
        } => expr_writes_this_property(expr, property),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_writes_this_property(condition, property)
                || stmts_write_this_property(then_branch, property)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| stmts_write_this_property(branch, property))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            expr_writes_this_property(condition, property)
                || stmts_write_this_property(body, property)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_writes_this_property(stmt, property))
                || condition
                    .as_ref()
                    .is_some_and(|expr| expr_writes_this_property(expr, property))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_writes_this_property(expr, property))
                || stmts_write_this_property(body, property)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_write_this_property(body, property)
                || catch
                    .as_ref()
                    .is_some_and(|catch| stmts_write_this_property(&catch.body, property))
                || finally
                    .as_ref()
                    .is_some_and(|branch| stmts_write_this_property(branch, property))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_writes_this_property(discriminant, property)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_writes_this_property(expr, property))
                        || stmts_write_this_property(&case.body, property)
                })
        }
        Stmt::Labeled { body, .. } => stmt_writes_this_property(body, property),
        Stmt::Return(None)
        | Stmt::Let { init: None, .. }
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
    }
}

fn expr_writes_this_property(expr: &Expr, property: &str) -> bool {
    match expr {
        Expr::PropertySet {
            object,
            property: name,
            ..
        }
        | Expr::PropertyUpdate {
            object,
            property: name,
            ..
        } if matches!(object.as_ref(), Expr::This) && name == property => true,
        Expr::PutValueSet { receiver, key, .. }
            if matches!(receiver.as_ref(), Expr::This)
                && matches!(key.as_ref(), Expr::String(name) if name == property) =>
        {
            true
        }
        _ => false,
    }
}
