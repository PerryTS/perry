use super::*;

/// Substitute type parameters with concrete types in a type
pub fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeVar(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Type::Array(elem) => Type::Array(Box::new(substitute_type(elem, substitutions))),
        Type::Tuple(elems) => Type::Tuple(
            elems
                .iter()
                .map(|e| substitute_type(e, substitutions))
                .collect(),
        ),
        Type::Promise(inner) => Type::Promise(Box::new(substitute_type(inner, substitutions))),
        Type::Union(types) => Type::Union(
            types
                .iter()
                .map(|t| substitute_type(t, substitutions))
                .collect(),
        ),
        Type::Generic { base, type_args } => Type::Generic {
            base: base.clone(),
            type_args: type_args
                .iter()
                .map(|t| substitute_type(t, substitutions))
                .collect(),
        },
        Type::Function(func_type) => Type::Function(crate::types::FunctionType {
            params: func_type
                .params
                .iter()
                .map(|(name, ty, opt)| (name.clone(), substitute_type(ty, substitutions), *opt))
                .collect(),
            return_type: Box::new(substitute_type(&func_type.return_type, substitutions)),
            is_async: func_type.is_async,
            is_generator: func_type.is_generator,
        }),
        // Primitive types don't need substitution
        _ => ty.clone(),
    }
}
