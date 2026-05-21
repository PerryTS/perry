use super::*;

// ============================================================================
// Type Inference for Generic Calls
// ============================================================================

/// Infer the type of an expression from its structure
/// Returns None if the type cannot be determined
fn infer_expr_type(expr: &Expr, module: &Module, idx: &ModuleIndex) -> Option<Type> {
    match expr {
        // Literals have known types
        Expr::Number(_) => Some(Type::Number),
        Expr::String(_) => Some(Type::String),
        Expr::Bool(_) => Some(Type::Boolean),
        Expr::Null => Some(Type::Null),
        Expr::Undefined => Some(Type::Void),
        Expr::BigInt(_) => Some(Type::BigInt),

        // Array literals - infer element type from first element
        Expr::Array(elems) => {
            if let Some(first) = elems.first() {
                if let Some(elem_ty) = infer_expr_type(first, module, idx) {
                    return Some(Type::Array(Box::new(elem_ty)));
                }
            }
            // Empty array or unknown element type
            Some(Type::Array(Box::new(Type::Any)))
        }

        // Object literals
        Expr::Object(_) | Expr::ObjectSpread { .. } => Some(Type::Object(ObjectType::default())),

        // Function calls - try to get return type
        Expr::Call {
            callee, type_args, ..
        } => {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                if let Some(&fi) = idx.func_by_id.get(func_id) {
                    let func = &module.functions[fi];
                    // If explicit type args provided, substitute them
                    if !type_args.is_empty() && !func.type_params.is_empty() {
                        let subs: HashMap<String, Type> = func
                            .type_params
                            .iter()
                            .zip(type_args.iter())
                            .map(|(p, t)| (p.name.clone(), t.clone()))
                            .collect();
                        return Some(substitute_type(&func.return_type, &subs));
                    }
                    // Otherwise return the declared return type (may contain type vars)
                    return Some(func.return_type.clone());
                }
            }
            None
        }

        // New expressions - return the class type
        Expr::New { class_name, .. } => Some(Type::Named(class_name.clone())),

        // Await unwraps a Promise
        Expr::Await(inner) => {
            if let Some(Type::Promise(inner_ty)) = infer_expr_type(inner, module, idx) {
                Some(*inner_ty)
            } else {
                None
            }
        }

        // Conditional returns the type of branches (assuming they match)
        Expr::Conditional { then_expr, .. } => infer_expr_type(then_expr, module, idx),

        // Binary operations
        Expr::Binary { op, .. } => match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow => Some(Type::Number),
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => Some(Type::Number),
        },

        // Comparisons return boolean
        Expr::Compare { .. } => Some(Type::Boolean),

        // Logical operators
        Expr::Logical {
            op, left, right, ..
        } => {
            match op {
                LogicalOp::And | LogicalOp::Or => {
                    // Returns one of the operands, try to infer from left
                    infer_expr_type(left, module, idx)
                        .or_else(|| infer_expr_type(right, module, idx))
                }
                LogicalOp::Coalesce => {
                    // Returns non-null operand
                    infer_expr_type(left, module, idx)
                        .or_else(|| infer_expr_type(right, module, idx))
                }
            }
        }

        // Unary operations
        Expr::Unary { op, .. } => match op {
            UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => Some(Type::Number),
            UnaryOp::Not => Some(Type::Boolean),
        },

        // TypeOf always returns string
        Expr::TypeOf(_) => Some(Type::String),

        // Void always returns undefined
        Expr::Void(_) => Some(Type::Void),

        // InstanceOf always returns boolean
        Expr::InstanceOf { .. } => Some(Type::Boolean),

        // For other expressions, we can't easily infer the type
        _ => None,
    }
}

/// Unify a parameter type with an argument type, collecting type variable bindings.
/// Returns true if unification succeeded.
pub(crate) fn unify_types(
    param_ty: &Type,
    arg_ty: &Type,
    bindings: &mut HashMap<String, Type>,
) -> bool {
    match (param_ty, arg_ty) {
        // Type variable - bind it to the argument type
        (Type::TypeVar(name), ty) => {
            if let Some(existing) = bindings.get(name) {
                // Already bound - check consistency
                types_compatible(existing, ty)
            } else {
                // Bind the type variable
                bindings.insert(name.clone(), ty.clone());
                true
            }
        }

        // Array types - unify element types
        (Type::Array(p_elem), Type::Array(a_elem)) => unify_types(p_elem, a_elem, bindings),

        // Tuple types - unify element-wise
        (Type::Tuple(p_elems), Type::Tuple(a_elems)) => {
            if p_elems.len() != a_elems.len() {
                return false;
            }
            p_elems
                .iter()
                .zip(a_elems.iter())
                .all(|(p, a)| unify_types(p, a, bindings))
        }

        // Promise types - unify inner types
        (Type::Promise(p_inner), Type::Promise(a_inner)) => unify_types(p_inner, a_inner, bindings),

        // Union types - arg must be one of the union members
        (Type::Union(p_types), arg) => {
            // Try to unify with any member
            p_types.iter().any(|p| unify_types(p, arg, bindings))
        }

        // Generic types - unify base and type args
        (
            Type::Generic {
                base: p_base,
                type_args: p_args,
            },
            Type::Generic {
                base: a_base,
                type_args: a_args,
            },
        ) => {
            if p_base != a_base || p_args.len() != a_args.len() {
                return false;
            }
            p_args
                .iter()
                .zip(a_args.iter())
                .all(|(p, a)| unify_types(p, a, bindings))
        }

        // Any matches anything
        (Type::Any, _) | (_, Type::Any) => true,

        // Unknown matches anything (for inference purposes)
        (Type::Unknown, _) | (_, Type::Unknown) => true,

        // Same concrete types match
        (p, a) if p == a => true,

        // Number literals can unify with Number
        (Type::Number, Type::Int32) | (Type::Int32, Type::Number) => true,

        // Named types might match if names are the same
        (Type::Named(p_name), Type::Named(a_name)) => p_name == a_name,

        // Otherwise, no match
        _ => false,
    }
}

/// Check if two types are compatible (for consistency checking)
fn types_compatible(ty1: &Type, ty2: &Type) -> bool {
    match (ty1, ty2) {
        (Type::Any, _) | (_, Type::Any) => true,
        (Type::Unknown, _) | (_, Type::Unknown) => true,
        (Type::Number, Type::Int32) | (Type::Int32, Type::Number) => true,
        (Type::Array(e1), Type::Array(e2)) => types_compatible(e1, e2),
        (Type::Promise(i1), Type::Promise(i2)) => types_compatible(i1, i2),
        (t1, t2) => t1 == t2,
    }
}

/// Infer type arguments for a generic function call.
/// Returns None if inference fails.
pub(crate) fn infer_type_args(
    func: &Function,
    args: &[Expr],
    module: &Module,
    idx: &ModuleIndex,
) -> Option<Vec<Type>> {
    if func.type_params.is_empty() {
        return None; // Not a generic function
    }

    let mut bindings: HashMap<String, Type> = HashMap::new();

    // Try to unify each parameter with its corresponding argument
    for (param, arg) in func.params.iter().zip(args.iter()) {
        // Skip if parameter type doesn't contain type variables
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        // Try to infer the argument's type
        if let Some(arg_ty) = infer_expr_type(arg, module, idx) {
            // Unify parameter type with argument type
            if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                // Unification failed - can't infer
                return None;
            }
        }
    }

    // Check if all type parameters were inferred
    let mut inferred_args = Vec::new();
    for type_param in &func.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            // Use default type if available
            inferred_args.push((**default).clone());
        } else {
            // Could not infer this type parameter
            return None;
        }
    }

    Some(inferred_args)
}

/// Check if a type contains any type variables
pub(crate) fn type_contains_type_var(ty: &Type) -> bool {
    match ty {
        Type::TypeVar(_) => true,
        Type::Array(elem) => type_contains_type_var(elem),
        Type::Tuple(elems) => elems.iter().any(type_contains_type_var),
        Type::Promise(inner) => type_contains_type_var(inner),
        Type::Union(types) => types.iter().any(type_contains_type_var),
        Type::Generic { type_args, .. } => type_args.iter().any(type_contains_type_var),
        Type::Function(ft) => {
            ft.params.iter().any(|(_, t, _)| type_contains_type_var(t))
                || type_contains_type_var(&ft.return_type)
        }
        _ => false,
    }
}

/// Infer type arguments for a generic class instantiation from constructor args.
/// Returns None if inference fails.
pub(crate) fn infer_type_args_for_class(
    class: &Class,
    constructor: &Function,
    args: &[Expr],
    module: &Module,
    idx: &ModuleIndex,
) -> Option<Vec<Type>> {
    if class.type_params.is_empty() {
        return None; // Not a generic class
    }

    let mut bindings: HashMap<String, Type> = HashMap::new();

    // Try to unify each constructor parameter with its corresponding argument
    for (param, arg) in constructor.params.iter().zip(args.iter()) {
        // Skip if parameter type doesn't contain type variables
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        // Try to infer the argument's type
        if let Some(arg_ty) = infer_expr_type(arg, module, idx) {
            // Unify parameter type with argument type
            if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                // Unification failed - can't infer
                return None;
            }
        }
    }

    // Check if all class type parameters were inferred
    let mut inferred_args = Vec::new();
    for type_param in &class.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            // Use default type if available
            inferred_args.push((**default).clone());
        } else {
            // Could not infer this type parameter
            return None;
        }
    }

    Some(inferred_args)
}
