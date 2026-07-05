use super::*;

/// Mangle type arguments to a string for use as a hash key
pub(crate) fn mangle_type_args(type_args: &[Type]) -> String {
    type_args
        .iter()
        .map(mangle_type)
        .collect::<Vec<_>>()
        .join("_")
}

/// Generate a mangled name for a specialized function/class
/// e.g., "identity" with [Type::Number] becomes "identity$number"
pub fn generate_specialized_name(base_name: &str, type_args: &[Type]) -> String {
    if type_args.is_empty() {
        return base_name.to_string();
    }

    let type_suffix: Vec<String> = type_args.iter().map(mangle_type).collect();

    format!("{}${}", base_name, type_suffix.join("_"))
}

/// Mangle a type into a string suitable for use in identifiers
pub fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Boolean => "bool".to_string(),
        Type::Number => "num".to_string(),
        Type::Int32 => "i32".to_string(),
        Type::BigInt => "bigint".to_string(),
        Type::String => "str".to_string(),
        Type::StringLiteral(_) => "str".to_string(),
        Type::Symbol => "sym".to_string(),
        Type::Array(elem) => format!("arr_{}", mangle_type(elem)),
        Type::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(mangle_type).collect();
            format!("tup_{}", parts.join("_"))
        }
        Type::Promise(inner) => format!("promise_{}", mangle_type(inner)),
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Named(name) => name.replace('.', "_"),
        Type::TypeVar(name) => name.clone(),
        Type::Generic { base, type_args } => {
            let args: Vec<String> = type_args.iter().map(mangle_type).collect();
            format!("{}_{}", base, args.join("_"))
        }
        Type::Union(types) => {
            let parts: Vec<String> = types.iter().map(mangle_type).collect();
            format!("union_{}", parts.join("_"))
        }
        Type::Object(_) => "obj".to_string(),
        Type::Function(_) => "fn".to_string(),
    }
}
