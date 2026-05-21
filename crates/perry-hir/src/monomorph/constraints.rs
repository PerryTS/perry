use super::*;

// ============================================================================
// Constraint Checking
// ============================================================================

/// Result of constraint checking
#[derive(Debug)]
pub enum ConstraintError {
    /// Type does not satisfy the constraint
    TypeMismatch {
        type_param: String,
        expected: Type,
        actual: Type,
    },
    /// Interface property missing
    MissingProperty {
        type_param: String,
        interface: String,
        property: String,
    },
    /// Interface method missing
    MissingMethod {
        type_param: String,
        interface: String,
        method: String,
    },
}

/// Check if a concrete type satisfies a constraint.
/// Returns Ok(()) if satisfied, Err with details if not.
fn check_constraint(
    type_param: &str,
    concrete_type: &Type,
    constraint: &Type,
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), ConstraintError> {
    match constraint {
        // Named constraint - check if concrete type is or implements the interface
        Type::Named(name) => check_named_constraint(type_param, concrete_type, name, module, idx),

        // Primitive constraints - simple type checking
        Type::Number | Type::String | Type::Boolean | Type::BigInt => {
            if types_satisfy(concrete_type, constraint) {
                Ok(())
            } else {
                Err(ConstraintError::TypeMismatch {
                    type_param: type_param.to_string(),
                    expected: constraint.clone(),
                    actual: concrete_type.clone(),
                })
            }
        }

        // Array constraint
        Type::Array(elem_constraint) => {
            if let Type::Array(elem_type) = concrete_type {
                check_constraint(type_param, elem_type, elem_constraint, module, idx)
            } else {
                Err(ConstraintError::TypeMismatch {
                    type_param: type_param.to_string(),
                    expected: constraint.clone(),
                    actual: concrete_type.clone(),
                })
            }
        }

        // Union constraint - concrete type must satisfy at least one branch
        Type::Union(branches) => {
            for branch in branches {
                if check_constraint(type_param, concrete_type, branch, module, idx).is_ok() {
                    return Ok(());
                }
            }
            Err(ConstraintError::TypeMismatch {
                type_param: type_param.to_string(),
                expected: constraint.clone(),
                actual: concrete_type.clone(),
            })
        }

        // Any/Unknown - everything satisfies these
        Type::Any | Type::Unknown => Ok(()),

        // Other constraints default to type equality check
        _ => {
            if types_satisfy(concrete_type, constraint) {
                Ok(())
            } else {
                Err(ConstraintError::TypeMismatch {
                    type_param: type_param.to_string(),
                    expected: constraint.clone(),
                    actual: concrete_type.clone(),
                })
            }
        }
    }
}

/// Check if a concrete type satisfies a named (interface/class) constraint
fn check_named_constraint(
    type_param: &str,
    concrete_type: &Type,
    constraint_name: &str,
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), ConstraintError> {
    // If the concrete type is the same named type, it satisfies
    if let Type::Named(name) = concrete_type {
        if name == constraint_name {
            return Ok(());
        }
    }

    // Look up the interface to check structural compatibility
    if let Some(&ii) = idx.interface_by_name.get(constraint_name) {
        let interface = &module.interfaces[ii];
        return check_interface_satisfaction(type_param, concrete_type, interface, module, idx);
    }

    // Look up class constraints
    if let Some(&_ci) = idx.class_by_name.get(constraint_name) {
        // For class constraints, the concrete type must be that class or a subclass
        if let Type::Named(name) = concrete_type {
            if name == constraint_name {
                return Ok(());
            }
        }
        return Err(ConstraintError::TypeMismatch {
            type_param: type_param.to_string(),
            expected: Type::Named(constraint_name.to_string()),
            actual: concrete_type.clone(),
        });
    }

    // Unknown constraint name - for now, be permissive
    Ok(())
}

/// Check if a concrete type satisfies an interface (structural typing)
fn check_interface_satisfaction(
    type_param: &str,
    concrete_type: &Type,
    interface: &Interface,
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), ConstraintError> {
    // Check built-in types against common interfaces
    match concrete_type {
        Type::String => {
            // String has 'length' property
            if interface.name == "HasLength" {
                return Ok(());
            }
            // Check if interface only requires 'length: number'
            if interface.properties.len() == 1 && interface.methods.is_empty() {
                if let Some(prop) = interface.properties.first() {
                    if prop.name == "length" && matches!(prop.ty, Type::Number | Type::Int32) {
                        return Ok(());
                    }
                }
            }
        }
        Type::Array(_) => {
            // Array has 'length' property
            if interface.name == "HasLength" {
                return Ok(());
            }
            // Check if interface only requires 'length: number'
            if interface.properties.len() == 1 && interface.methods.is_empty() {
                if let Some(prop) = interface.properties.first() {
                    if prop.name == "length" && matches!(prop.ty, Type::Number | Type::Int32) {
                        return Ok(());
                    }
                }
            }
        }
        Type::Object(obj_type) => {
            // Check all required interface properties exist in object with
            // compatible types.
            for prop in &interface.properties {
                if prop.optional {
                    continue; // Optional properties don't need to be present
                }
                let Some(actual) = obj_type.properties.get(&prop.name) else {
                    return Err(ConstraintError::MissingProperty {
                        type_param: type_param.to_string(),
                        interface: interface.name.clone(),
                        property: prop.name.clone(),
                    });
                };
                if !types_satisfy(&actual.ty, &prop.ty) {
                    return Err(ConstraintError::TypeMismatch {
                        type_param: format!("{}.{}", type_param, prop.name),
                        expected: prop.ty.clone(),
                        actual: actual.ty.clone(),
                    });
                }
            }
            return Ok(());
        }
        Type::Named(name) => {
            // Look up the named type (could be a class)
            if let Some(&ci) = idx.class_by_name.get(name.as_str()) {
                let class = &module.classes[ci];
                // Check all required interface properties exist in class
                // fields with compatible types.
                for prop in &interface.properties {
                    if prop.optional {
                        continue;
                    }
                    let Some(field) = class.fields.iter().find(|f| f.name == prop.name) else {
                        return Err(ConstraintError::MissingProperty {
                            type_param: type_param.to_string(),
                            interface: interface.name.clone(),
                            property: prop.name.clone(),
                        });
                    };
                    if !types_satisfy(&field.ty, &prop.ty) {
                        return Err(ConstraintError::TypeMismatch {
                            type_param: format!("{}.{}", type_param, prop.name),
                            expected: prop.ty.clone(),
                            actual: field.ty.clone(),
                        });
                    }
                }
                // Check all required interface methods exist in class methods
                for method in &interface.methods {
                    let has_method = class.methods.iter().any(|m| m.name == method.name);
                    if !has_method {
                        return Err(ConstraintError::MissingMethod {
                            type_param: type_param.to_string(),
                            interface: interface.name.clone(),
                            method: method.name.clone(),
                        });
                    }
                }
                return Ok(());
            }
        }
        _ => {}
    }

    // For types we can't structurally check, be permissive for now
    // A full type checker would be more strict
    Ok(())
}

/// Check if a type satisfies another type (simple structural check)
fn types_satisfy(actual: &Type, expected: &Type) -> bool {
    match (actual, expected) {
        (Type::Any, _) | (_, Type::Any) => true,
        (Type::Unknown, _) | (_, Type::Unknown) => true,
        (Type::Number, Type::Number)
        | (Type::Int32, Type::Number)
        | (Type::Number, Type::Int32) => true,
        (Type::String, Type::String) => true,
        (Type::Boolean, Type::Boolean) => true,
        (Type::BigInt, Type::BigInt) => true,
        (Type::Array(a), Type::Array(b)) => types_satisfy(a, b),
        (Type::Named(a), Type::Named(b)) => a == b,
        _ => actual == expected,
    }
}

/// Check all type parameter constraints for a function specialization
pub(crate) fn check_function_constraints(
    func: &Function,
    type_args: &[Type],
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), Vec<ConstraintError>> {
    let mut errors = Vec::new();

    for (param, arg) in func.type_params.iter().zip(type_args.iter()) {
        if let Some(ref constraint) = param.constraint {
            if let Err(e) = check_constraint(&param.name, arg, constraint, module, idx) {
                errors.push(e);
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check all type parameter constraints for a class specialization
pub(crate) fn check_class_constraints(
    class: &Class,
    type_args: &[Type],
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), Vec<ConstraintError>> {
    let mut errors = Vec::new();

    for (param, arg) in class.type_params.iter().zip(type_args.iter()) {
        if let Some(ref constraint) = param.constraint {
            if let Err(e) = check_constraint(&param.name, arg, constraint, module, idx) {
                errors.push(e);
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
