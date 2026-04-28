//! Monomorphization pass for generics
//!
//! This module implements monomorphization - the process of generating
//! specialized versions of generic functions and classes for each unique
//! type instantiation. This is similar to how Rust handles generics.
//!
//! For example, given:
//!   function identity<T>(x: T): T { return x; }
//!   identity<number>(42);
//!   identity<string>("hello");
//!
//! We generate:
//!   function identity_number(x: number): number { return x; }
//!   function identity_string(x: string): string { return x; }

use std::collections::{HashMap, HashSet, VecDeque};
use perry_types::{FuncId, ObjectType, Type};
use crate::ir::*;

/// Pre-built index for O(1) lookups into module collections.
/// Built once from the original module state before any specializations are added.
struct ModuleIndex {
    /// Map from function ID to its index in module.functions
    func_by_id: HashMap<FuncId, usize>,
    /// Map from class name to its index in module.classes
    class_by_name: HashMap<String, usize>,
    /// Map from interface name to its index in module.interfaces
    interface_by_name: HashMap<String, usize>,
}

impl ModuleIndex {
    fn new(module: &Module) -> Self {
        let func_by_id: HashMap<FuncId, usize> = module.functions.iter()
            .enumerate()
            .map(|(i, f)| (f.id, i))
            .collect();

        let class_by_name: HashMap<String, usize> = module.classes.iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();

        let interface_by_name: HashMap<String, usize> = module.interfaces.iter()
            .enumerate()
            .map(|(i, iface)| (iface.name.clone(), i))
            .collect();

        Self { func_by_id, class_by_name, interface_by_name }
    }
}

/// Key for function specialization (func_id, mangled_type_args)
type FuncSpecKey = (FuncId, String);

/// Key for class specialization (class_name, mangled_type_args)
type ClassSpecKey = (String, String);

/// Context for monomorphization pass
pub struct MonomorphizationContext {
    /// Map from (original func_id, mangled_type_args) to specialized func_id
    specialized_funcs: HashMap<FuncSpecKey, FuncId>,
    /// Map from (class_name, mangled_type_args) to specialized class name
    specialized_classes: HashMap<ClassSpecKey, String>,
    /// Queue of functions needing specialization
    func_work_queue: VecDeque<FuncSpecRequest>,
    /// Queue of classes needing specialization
    class_work_queue: VecDeque<ClassSpecRequest>,
    /// Counter for generating unique function IDs
    next_func_id: FuncId,
    /// Counter for generating unique class IDs
    next_class_id: ClassId,
    /// Set of already processed specializations (to avoid duplicates)
    processed_funcs: HashSet<FuncSpecKey>,
    processed_classes: HashSet<ClassSpecKey>,
}

/// Request to specialize a function
#[derive(Debug, Clone)]
struct FuncSpecRequest {
    /// Original function ID
    original_id: FuncId,
    /// Type arguments to substitute
    type_args: Vec<Type>,
    /// New function ID for the specialized version
    new_id: FuncId,
}

/// Request to specialize a class
#[derive(Debug, Clone)]
struct ClassSpecRequest {
    /// Original class name
    original_name: String,
    /// Type arguments to substitute
    type_args: Vec<Type>,
    /// New class name for the specialized version
    new_name: String,
}

impl MonomorphizationContext {
    pub fn new(module: &Module) -> Self {
        // Find the highest existing func_id and class_id
        let max_func_id = module.functions.iter()
            .map(|f| f.id)
            .max()
            .unwrap_or(0);

        let max_class_id = module.classes.iter()
            .map(|c| c.id)
            .max()
            .unwrap_or(0);

        Self {
            specialized_funcs: HashMap::new(),
            specialized_classes: HashMap::new(),
            func_work_queue: VecDeque::new(),
            class_work_queue: VecDeque::new(),
            next_func_id: max_func_id + 1000, // Leave room for original IDs
            next_class_id: max_class_id + 1000,
            processed_funcs: HashSet::new(),
            processed_classes: HashSet::new(),
        }
    }

    fn fresh_func_id(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id += 1;
        id
    }

    fn fresh_class_id(&mut self) -> ClassId {
        let id = self.next_class_id;
        self.next_class_id += 1;
        id
    }

    /// Request specialization of a function with given type arguments
    /// Returns the specialized function's ID
    pub fn request_func_specialization(&mut self, func_id: FuncId, type_args: Vec<Type>) -> FuncId {
        let mangled_args = mangle_type_args(&type_args);
        let key = (func_id, mangled_args);

        if let Some(&specialized_id) = self.specialized_funcs.get(&key) {
            return specialized_id;
        }

        let new_id = self.fresh_func_id();
        self.specialized_funcs.insert(key.clone(), new_id);

        if !self.processed_funcs.contains(&key) {
            self.func_work_queue.push_back(FuncSpecRequest {
                original_id: func_id,
                type_args,
                new_id,
            });
        }

        new_id
    }

    /// Request specialization of a class with given type arguments
    /// Returns the specialized class name
    pub fn request_class_specialization(&mut self, class_name: &str, type_args: Vec<Type>) -> String {
        let mangled_args = mangle_type_args(&type_args);
        let key = (class_name.to_string(), mangled_args);

        if let Some(specialized_name) = self.specialized_classes.get(&key) {
            return specialized_name.clone();
        }

        let new_name = generate_specialized_name(class_name, &type_args);
        self.specialized_classes.insert(key.clone(), new_name.clone());

        if !self.processed_classes.contains(&key) {
            self.class_work_queue.push_back(ClassSpecRequest {
                original_name: class_name.to_string(),
                type_args,
                new_name: new_name.clone(),
            });
        }

        new_name
    }
}

/// Mangle type arguments to a string for use as a hash key
fn mangle_type_args(type_args: &[Type]) -> String {
    type_args.iter()
        .map(|t| mangle_type(t))
        .collect::<Vec<_>>()
        .join("_")
}

/// Generate a mangled name for a specialized function/class
/// e.g., "identity" with [Type::Number] becomes "identity$number"
fn generate_specialized_name(base_name: &str, type_args: &[Type]) -> String {
    if type_args.is_empty() {
        return base_name.to_string();
    }

    let type_suffix: Vec<String> = type_args.iter()
        .map(|t| mangle_type(t))
        .collect();

    format!("{}${}", base_name, type_suffix.join("_"))
}

/// Mangle a type into a string suitable for use in identifiers
fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Boolean => "bool".to_string(),
        Type::Number => "num".to_string(),
        Type::Int32 => "i32".to_string(),
        Type::BigInt => "bigint".to_string(),
        Type::String => "str".to_string(),
        Type::Symbol => "sym".to_string(),
        Type::Array(elem) => format!("arr_{}", mangle_type(elem)),
        Type::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(|e| mangle_type(e)).collect();
            format!("tup_{}", parts.join("_"))
        }
        Type::Promise(inner) => format!("promise_{}", mangle_type(inner)),
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Named(name) => name.replace('.', "_"),
        Type::TypeVar(name) => name.clone(),
        Type::Generic { base, type_args } => {
            let args: Vec<String> = type_args.iter().map(|t| mangle_type(t)).collect();
            format!("{}_{}", base, args.join("_"))
        }
        Type::Union(types) => {
            let parts: Vec<String> = types.iter().map(|t| mangle_type(t)).collect();
            format!("union_{}", parts.join("_"))
        }
        Type::Object(_) => "obj".to_string(),
        Type::Function(_) => "fn".to_string(),
    }
}

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
        Expr::Call { callee, type_args, .. } => {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                if let Some(&fi) = idx.func_by_id.get(func_id) {
                    let func = &module.functions[fi];
                    // If explicit type args provided, substitute them
                    if !type_args.is_empty() && !func.type_params.is_empty() {
                        let subs: HashMap<String, Type> = func.type_params.iter()
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
        Expr::New { class_name, .. } => {
            Some(Type::Named(class_name.clone()))
        }

        // Await unwraps a Promise
        Expr::Await(inner) => {
            if let Some(Type::Promise(inner_ty)) = infer_expr_type(inner, module, idx) {
                Some(*inner_ty)
            } else {
                None
            }
        }

        // Conditional returns the type of branches (assuming they match)
        Expr::Conditional { then_expr, .. } => {
            infer_expr_type(then_expr, module, idx)
        }

        // Binary operations
        Expr::Binary { op, .. } => {
            match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div |
                BinaryOp::Mod | BinaryOp::Pow => Some(Type::Number),
                BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor |
                BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr => Some(Type::Number),
            }
        }

        // Comparisons return boolean
        Expr::Compare { .. } => Some(Type::Boolean),

        // Logical operators
        Expr::Logical { op, left, right, .. } => {
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
        Expr::Unary { op, .. } => {
            match op {
                UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => Some(Type::Number),
                UnaryOp::Not => Some(Type::Boolean),
            }
        }

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
fn unify_types(
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
        (Type::Array(p_elem), Type::Array(a_elem)) => {
            unify_types(p_elem, a_elem, bindings)
        }

        // Tuple types - unify element-wise
        (Type::Tuple(p_elems), Type::Tuple(a_elems)) => {
            if p_elems.len() != a_elems.len() {
                return false;
            }
            p_elems.iter().zip(a_elems.iter())
                .all(|(p, a)| unify_types(p, a, bindings))
        }

        // Promise types - unify inner types
        (Type::Promise(p_inner), Type::Promise(a_inner)) => {
            unify_types(p_inner, a_inner, bindings)
        }

        // Union types - arg must be one of the union members
        (Type::Union(p_types), arg) => {
            // Try to unify with any member
            p_types.iter().any(|p| unify_types(p, arg, bindings))
        }

        // Generic types - unify base and type args
        (Type::Generic { base: p_base, type_args: p_args },
         Type::Generic { base: a_base, type_args: a_args }) => {
            if p_base != a_base || p_args.len() != a_args.len() {
                return false;
            }
            p_args.iter().zip(a_args.iter())
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
fn infer_type_args(
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
fn type_contains_type_var(ty: &Type) -> bool {
    match ty {
        Type::TypeVar(_) => true,
        Type::Array(elem) => type_contains_type_var(elem),
        Type::Tuple(elems) => elems.iter().any(type_contains_type_var),
        Type::Promise(inner) => type_contains_type_var(inner),
        Type::Union(types) => types.iter().any(type_contains_type_var),
        Type::Generic { type_args, .. } => type_args.iter().any(type_contains_type_var),
        Type::Function(ft) => {
            ft.params.iter().any(|(_, t, _)| type_contains_type_var(t)) ||
            type_contains_type_var(&ft.return_type)
        }
        _ => false,
    }
}

/// Infer type arguments for a generic class instantiation from constructor args.
/// Returns None if inference fails.
fn infer_type_args_for_class(
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

/// Substitute type parameters with concrete types in a type
pub fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeVar(name) => {
            substitutions.get(name).cloned().unwrap_or_else(|| ty.clone())
        }
        Type::Array(elem) => {
            Type::Array(Box::new(substitute_type(elem, substitutions)))
        }
        Type::Tuple(elems) => {
            Type::Tuple(elems.iter().map(|e| substitute_type(e, substitutions)).collect())
        }
        Type::Promise(inner) => {
            Type::Promise(Box::new(substitute_type(inner, substitutions)))
        }
        Type::Union(types) => {
            Type::Union(types.iter().map(|t| substitute_type(t, substitutions)).collect())
        }
        Type::Generic { base, type_args } => {
            Type::Generic {
                base: base.clone(),
                type_args: type_args.iter().map(|t| substitute_type(t, substitutions)).collect(),
            }
        }
        Type::Function(func_type) => {
            Type::Function(perry_types::FunctionType {
                params: func_type.params.iter()
                    .map(|(name, ty, opt)| (name.clone(), substitute_type(ty, substitutions), *opt))
                    .collect(),
                return_type: Box::new(substitute_type(&func_type.return_type, substitutions)),
                is_async: func_type.is_async,
                is_generator: func_type.is_generator,
            })
        }
        // Primitive types don't need substitution
        _ => ty.clone(),
    }
}

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
pub fn check_constraint(
    type_param: &str,
    concrete_type: &Type,
    constraint: &Type,
    module: &Module,
    idx: &ModuleIndex,
) -> Result<(), ConstraintError> {
    match constraint {
        // Named constraint - check if concrete type is or implements the interface
        Type::Named(name) => {
            check_named_constraint(type_param, concrete_type, name, module, idx)
        }

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
        (Type::Number, Type::Number) | (Type::Int32, Type::Number) | (Type::Number, Type::Int32) => true,
        (Type::String, Type::String) => true,
        (Type::Boolean, Type::Boolean) => true,
        (Type::BigInt, Type::BigInt) => true,
        (Type::Array(a), Type::Array(b)) => types_satisfy(a, b),
        (Type::Named(a), Type::Named(b)) => a == b,
        _ => actual == expected,
    }
}

/// Check all type parameter constraints for a function specialization
pub fn check_function_constraints(
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
pub fn check_class_constraints(
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

/// Substitute types in an expression
fn substitute_expr(expr: &Expr, substitutions: &HashMap<String, Type>) -> Expr {
    match expr {
        // Literals don't need substitution
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) |
        Expr::Integer(_) | Expr::BigInt(_) | Expr::String(_) => expr.clone(),

        // Variables
        Expr::LocalGet(id) => Expr::LocalGet(*id),
        Expr::LocalSet(id, val) => Expr::LocalSet(*id, Box::new(substitute_expr(val, substitutions))),
        Expr::GlobalGet(id) => Expr::GlobalGet(*id),
        Expr::GlobalSet(id, val) => Expr::GlobalSet(*id, Box::new(substitute_expr(val, substitutions))),

        // Update
        Expr::Update { id, op, prefix } => Expr::Update { id: *id, op: *op, prefix: *prefix },

        // Operations
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(substitute_expr(operand, substitutions)),
        },
        Expr::Compare { op, left, right } => Expr::Compare {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },
        Expr::Logical { op, left, right } => Expr::Logical {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },

        // Function call
        Expr::Call { callee, args, type_args } => Expr::Call {
            callee: Box::new(substitute_expr(callee, substitutions)),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
            type_args: type_args.iter().map(|t| substitute_type(t, substitutions)).collect(),
        },

        // References
        Expr::FuncRef(id) => Expr::FuncRef(*id),
        Expr::ExternFuncRef { name, param_types, return_type } => Expr::ExternFuncRef {
            name: name.clone(),
            param_types: param_types.clone(),
            return_type: return_type.clone(),
        },
        Expr::NativeModuleRef(name) => Expr::NativeModuleRef(name.clone()),
        Expr::NativeMethodCall { module, class_name, object, method, args } => Expr::NativeMethodCall {
            module: module.clone(),
            class_name: class_name.clone(),
            object: object.as_ref().map(|o| Box::new(substitute_expr(o, substitutions))),
            method: method.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
        },

        // Property access
        Expr::PropertyGet { object, property } => Expr::PropertyGet {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
        },
        Expr::PropertySet { object, property, value } => Expr::PropertySet {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::PropertyUpdate { object, property, op, prefix } => Expr::PropertyUpdate {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
            op: *op,
            prefix: *prefix,
        },

        // Index access
        Expr::IndexGet { object, index } => Expr::IndexGet {
            object: Box::new(substitute_expr(object, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
        },
        Expr::IndexSet { object, index, value } => Expr::IndexSet {
            object: Box::new(substitute_expr(object, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },

        // Literals
        Expr::Object(props) => Expr::Object(
            props.iter().map(|(k, v)| (k.clone(), substitute_expr(v, substitutions))).collect()
        ),
        Expr::ObjectSpread { parts } => Expr::ObjectSpread {
            parts: parts.iter().map(|(k, v)| (k.clone(), substitute_expr(v, substitutions))).collect()
        },
        Expr::Array(elems) => Expr::Array(
            elems.iter().map(|e| substitute_expr(e, substitutions)).collect()
        ),
        Expr::ArraySpread(elems) => Expr::ArraySpread(
            elems.iter().map(|e| match e {
                ArrayElement::Expr(expr) => ArrayElement::Expr(substitute_expr(expr, substitutions)),
                ArrayElement::Spread(expr) => ArrayElement::Spread(substitute_expr(expr, substitutions)),
            }).collect()
        ),

        // Conditional
        Expr::Conditional { condition, then_expr, else_expr } => Expr::Conditional {
            condition: Box::new(substitute_expr(condition, substitutions)),
            then_expr: Box::new(substitute_expr(then_expr, substitutions)),
            else_expr: Box::new(substitute_expr(else_expr, substitutions)),
        },

        // Type operations
        Expr::TypeOf(inner) => Expr::TypeOf(Box::new(substitute_expr(inner, substitutions))),
        Expr::Void(inner) => Expr::Void(Box::new(substitute_expr(inner, substitutions))),
        Expr::Yield { value, delegate } => Expr::Yield {
            value: value.as_ref().map(|v| Box::new(substitute_expr(v, substitutions))),
            delegate: *delegate,
        },
        Expr::InstanceOf { expr, ty } => Expr::InstanceOf {
            expr: Box::new(substitute_expr(expr, substitutions)),
            ty: ty.clone(),
        },

        // Await
        Expr::Await(inner) => Expr::Await(Box::new(substitute_expr(inner, substitutions))),

        // New
        Expr::New { class_name, args, type_args } => Expr::New {
            class_name: class_name.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
            type_args: type_args.iter().map(|t| substitute_type(t, substitutions)).collect(),
        },

        // Class/Enum references
        Expr::ClassRef(name) => Expr::ClassRef(name.clone()),
        Expr::EnumMember { enum_name, member_name } => Expr::EnumMember {
            enum_name: enum_name.clone(),
            member_name: member_name.clone(),
        },

        // Static field/method access
        Expr::StaticFieldGet { class_name, field_name } => Expr::StaticFieldGet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
        },
        Expr::StaticFieldSet { class_name, field_name, value } => Expr::StaticFieldSet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::StaticMethodCall { class_name, method_name, args } => Expr::StaticMethodCall {
            class_name: class_name.clone(),
            method_name: method_name.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
        },

        // This/Super
        Expr::This => Expr::This,
        Expr::SuperCall(args) => Expr::SuperCall(
            args.iter().map(|a| substitute_expr(a, substitutions)).collect()
        ),
        Expr::SuperMethodCall { method, args } => Expr::SuperMethodCall {
            method: method.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
        },

        // Environment
        Expr::EnvGet(name) => Expr::EnvGet(name.clone()),
        Expr::ProcessUptime => Expr::ProcessUptime,
        Expr::ProcessMemoryUsage => Expr::ProcessMemoryUsage,

        // File system
        Expr::FsReadFileSync(path) => Expr::FsReadFileSync(Box::new(substitute_expr(path, substitutions))),
        Expr::FsWriteFileSync(path, content) => Expr::FsWriteFileSync(
            Box::new(substitute_expr(path, substitutions)),
            Box::new(substitute_expr(content, substitutions)),
        ),
        Expr::FsExistsSync(path) => Expr::FsExistsSync(Box::new(substitute_expr(path, substitutions))),
        Expr::FsMkdirSync(path) => Expr::FsMkdirSync(Box::new(substitute_expr(path, substitutions))),
        Expr::FsUnlinkSync(path) => Expr::FsUnlinkSync(Box::new(substitute_expr(path, substitutions))),
        Expr::FsAppendFileSync(path, content) => Expr::FsAppendFileSync(
            Box::new(substitute_expr(path, substitutions)),
            Box::new(substitute_expr(content, substitutions)),
        ),

        // Path operations
        Expr::PathJoin(a, b) => Expr::PathJoin(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::PathDirname(path) => Expr::PathDirname(Box::new(substitute_expr(path, substitutions))),
        Expr::PathBasename(path) => Expr::PathBasename(Box::new(substitute_expr(path, substitutions))),
        Expr::PathExtname(path) => Expr::PathExtname(Box::new(substitute_expr(path, substitutions))),
        Expr::PathResolve(path) => Expr::PathResolve(Box::new(substitute_expr(path, substitutions))),
        Expr::PathIsAbsolute(path) => Expr::PathIsAbsolute(Box::new(substitute_expr(path, substitutions))),

        // Array methods
        Expr::ArrayPush { array_id, value } => Expr::ArrayPush {
            array_id: *array_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayPushSpread { array_id, source } => Expr::ArrayPushSpread {
            array_id: *array_id,
            source: Box::new(substitute_expr(source, substitutions)),
        },
        Expr::ArrayPop(id) => Expr::ArrayPop(*id),
        Expr::ArrayShift(id) => Expr::ArrayShift(*id),
        Expr::ArrayUnshift { array_id, value } => Expr::ArrayUnshift {
            array_id: *array_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayIndexOf { array, value } => Expr::ArrayIndexOf {
            array: Box::new(substitute_expr(array, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayIncludes { array, value } => Expr::ArrayIncludes {
            array: Box::new(substitute_expr(array, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArraySlice { array, start, end } => Expr::ArraySlice {
            array: Box::new(substitute_expr(array, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            end: end.as_ref().map(|e| Box::new(substitute_expr(e, substitutions))),
        },
        Expr::ArraySplice { array_id, start, delete_count, items } => Expr::ArraySplice {
            array_id: *array_id,
            start: Box::new(substitute_expr(start, substitutions)),
            delete_count: delete_count.as_ref().map(|d| Box::new(substitute_expr(d, substitutions))),
            items: items.iter().map(|i| substitute_expr(i, substitutions)).collect(),
        },
        Expr::ArrayForEach { array, callback } => Expr::ArrayForEach {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayMap { array, callback } => Expr::ArrayMap {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFilter { array, callback } => Expr::ArrayFilter {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFind { array, callback } => Expr::ArrayFind {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFindIndex { array, callback } => Expr::ArrayFindIndex {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArraySort { array, comparator } => Expr::ArraySort {
            array: Box::new(substitute_expr(array, substitutions)),
            comparator: Box::new(substitute_expr(comparator, substitutions)),
        },
        Expr::ArrayReduce { array, callback, initial } => Expr::ArrayReduce {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
            initial: initial.as_ref().map(|i| Box::new(substitute_expr(i, substitutions))),
        },
        Expr::ArrayReduceRight { array, callback, initial } => Expr::ArrayReduceRight {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
            initial: initial.as_ref().map(|i| Box::new(substitute_expr(i, substitutions))),
        },
        Expr::ArrayJoin { array, separator } => Expr::ArrayJoin {
            array: Box::new(substitute_expr(array, substitutions)),
            separator: separator.as_ref().map(|s| Box::new(substitute_expr(s, substitutions))),
        },
        Expr::ArrayFlat { array } => Expr::ArrayFlat {
            array: Box::new(substitute_expr(array, substitutions)),
        },
        Expr::ArrayToReversed { array } => Expr::ArrayToReversed {
            array: Box::new(substitute_expr(array, substitutions)),
        },
        Expr::ArrayToSorted { array, comparator } => Expr::ArrayToSorted {
            array: Box::new(substitute_expr(array, substitutions)),
            comparator: comparator.as_ref().map(|c| Box::new(substitute_expr(c, substitutions))),
        },
        Expr::ArrayToSpliced { array, start, delete_count, items } => Expr::ArrayToSpliced {
            array: Box::new(substitute_expr(array, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            delete_count: Box::new(substitute_expr(delete_count, substitutions)),
            items: items.iter().map(|i| substitute_expr(i, substitutions)).collect(),
        },
        Expr::ArrayWith { array, index, value } => Expr::ArrayWith {
            array: Box::new(substitute_expr(array, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayCopyWithin { array_id, target, start, end } => Expr::ArrayCopyWithin {
            array_id: *array_id,
            target: Box::new(substitute_expr(target, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            end: end.as_ref().map(|e| Box::new(substitute_expr(e, substitutions))),
        },
        Expr::ArrayEntries(array) => Expr::ArrayEntries(Box::new(substitute_expr(array, substitutions))),
        Expr::ArrayKeys(array) => Expr::ArrayKeys(Box::new(substitute_expr(array, substitutions))),
        Expr::ArrayValues(array) => Expr::ArrayValues(Box::new(substitute_expr(array, substitutions))),

        // String methods
        Expr::StringSplit(string, delimiter) => Expr::StringSplit(
            Box::new(substitute_expr(string, substitutions)),
            Box::new(substitute_expr(delimiter, substitutions)),
        ),
        Expr::StringFromCharCode(code) => Expr::StringFromCharCode(
            Box::new(substitute_expr(code, substitutions)),
        ),

        // Map operations
        Expr::MapNew => Expr::MapNew,
        Expr::MapNewFromArray(expr) => Expr::MapNewFromArray(Box::new(substitute_expr(expr, substitutions))),
        Expr::MapSet { map, key, value } => Expr::MapSet {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::MapGet { map, key } => Expr::MapGet {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapHas { map, key } => Expr::MapHas {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapDelete { map, key } => Expr::MapDelete {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapSize(map) => Expr::MapSize(Box::new(substitute_expr(map, substitutions))),
        Expr::MapClear(map) => Expr::MapClear(Box::new(substitute_expr(map, substitutions))),
        Expr::MapEntries(map) => Expr::MapEntries(Box::new(substitute_expr(map, substitutions))),
        Expr::MapKeys(map) => Expr::MapKeys(Box::new(substitute_expr(map, substitutions))),
        Expr::MapValues(map) => Expr::MapValues(Box::new(substitute_expr(map, substitutions))),

        // Set operations
        Expr::SetNew => Expr::SetNew,
        Expr::SetNewFromArray(expr) => Expr::SetNewFromArray(Box::new(substitute_expr(expr, substitutions))),
        Expr::SetAdd { set_id, value } => Expr::SetAdd {
            set_id: *set_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetHas { set, value } => Expr::SetHas {
            set: Box::new(substitute_expr(set, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetDelete { set, value } => Expr::SetDelete {
            set: Box::new(substitute_expr(set, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetSize(set) => Expr::SetSize(Box::new(substitute_expr(set, substitutions))),
        Expr::SetClear(set) => Expr::SetClear(Box::new(substitute_expr(set, substitutions))),
        Expr::SetValues(set) => Expr::SetValues(Box::new(substitute_expr(set, substitutions))),

        // JSON operations
        Expr::JsonParse(expr) => Expr::JsonParse(Box::new(substitute_expr(expr, substitutions))),
        Expr::JsonStringify(expr) => Expr::JsonStringify(Box::new(substitute_expr(expr, substitutions))),

        // Math operations
        Expr::MathFloor(expr) => Expr::MathFloor(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathCeil(expr) => Expr::MathCeil(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathRound(expr) => Expr::MathRound(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathAbs(expr) => Expr::MathAbs(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathSqrt(expr) => Expr::MathSqrt(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathPow(base, exp) => Expr::MathPow(
            Box::new(substitute_expr(base, substitutions)),
            Box::new(substitute_expr(exp, substitutions)),
        ),
        Expr::MathImul(a, b) => Expr::MathImul(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::MathMin(args) => Expr::MathMin(
            args.iter().map(|a| substitute_expr(a, substitutions)).collect()
        ),
        Expr::MathMax(args) => Expr::MathMax(
            args.iter().map(|a| substitute_expr(a, substitutions)).collect()
        ),
        Expr::MathMinSpread(e) => Expr::MathMinSpread(Box::new(substitute_expr(e, substitutions))),
        Expr::MathMaxSpread(e) => Expr::MathMaxSpread(Box::new(substitute_expr(e, substitutions))),
        Expr::MathRandom => Expr::MathRandom,

        // Crypto operations
        Expr::CryptoRandomBytes(expr) => Expr::CryptoRandomBytes(Box::new(substitute_expr(expr, substitutions))),
        Expr::CryptoRandomUUID => Expr::CryptoRandomUUID,
        Expr::CryptoSha256(expr) => Expr::CryptoSha256(Box::new(substitute_expr(expr, substitutions))),
        Expr::CryptoMd5(expr) => Expr::CryptoMd5(Box::new(substitute_expr(expr, substitutions))),

        // Date operations
        Expr::DateNow => Expr::DateNow,
        Expr::DateNew(timestamp) => Expr::DateNew(timestamp.as_ref().map(|ts| Box::new(substitute_expr(ts, substitutions)))),
        Expr::DateGetTime(date) => Expr::DateGetTime(Box::new(substitute_expr(date, substitutions))),
        Expr::DateToISOString(date) => Expr::DateToISOString(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetFullYear(date) => Expr::DateGetFullYear(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetMonth(date) => Expr::DateGetMonth(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetDate(date) => Expr::DateGetDate(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetHours(date) => Expr::DateGetHours(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetMinutes(date) => Expr::DateGetMinutes(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetSeconds(date) => Expr::DateGetSeconds(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetMilliseconds(date) => Expr::DateGetMilliseconds(Box::new(substitute_expr(date, substitutions))),

        // Sequence
        Expr::Sequence(exprs) => Expr::Sequence(
            exprs.iter().map(|e| substitute_expr(e, substitutions)).collect()
        ),

        // Closure
        Expr::Closure { func_id, params, return_type, body, captures, mutable_captures, captures_this, enclosing_class, is_async } => {
            Expr::Closure {
                func_id: *func_id,
                params: params.iter().map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, substitutions),
                    default: p.default.as_ref().map(|d| substitute_expr(d, substitutions)),
                    is_rest: p.is_rest,
                }).collect(),
                return_type: substitute_type(return_type, substitutions),
                body: substitute_stmts(body, substitutions),
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this: *captures_this,
                enclosing_class: enclosing_class.clone(),
                is_async: *is_async,
            }
        }

        // RegExp operations
        Expr::RegExp { pattern, flags } => Expr::RegExp {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::RegExpTest { regex, string } => Expr::RegExpTest {
            regex: Box::new(substitute_expr(regex, substitutions)),
            string: Box::new(substitute_expr(string, substitutions)),
        },
        Expr::StringMatch { string, regex } => Expr::StringMatch {
            string: Box::new(substitute_expr(string, substitutions)),
            regex: Box::new(substitute_expr(regex, substitutions)),
        },
        Expr::StringReplace { string, pattern, replacement } => Expr::StringReplace {
            string: Box::new(substitute_expr(string, substitutions)),
            pattern: Box::new(substitute_expr(pattern, substitutions)),
            replacement: Box::new(substitute_expr(replacement, substitutions)),
        },

        // Object.keys/values/entries
        Expr::ObjectKeys(obj) => Expr::ObjectKeys(Box::new(substitute_expr(obj, substitutions))),
        Expr::ObjectValues(obj) => Expr::ObjectValues(Box::new(substitute_expr(obj, substitutions))),
        Expr::ObjectEntries(obj) => Expr::ObjectEntries(Box::new(substitute_expr(obj, substitutions))),

        // Array.isArray / Array.from
        Expr::ArrayIsArray(value) => Expr::ArrayIsArray(Box::new(substitute_expr(value, substitutions))),
        Expr::ArrayFrom(value) => Expr::ArrayFrom(Box::new(substitute_expr(value, substitutions))),
        Expr::ArrayFromMapped { iterable, map_fn } => Expr::ArrayFromMapped {
            iterable: Box::new(substitute_expr(iterable, substitutions)),
            map_fn: Box::new(substitute_expr(map_fn, substitutions)),
        },

        // Global built-in functions
        Expr::ParseInt { string, radix } => Expr::ParseInt {
            string: Box::new(substitute_expr(string, substitutions)),
            radix: radix.as_ref().map(|r| Box::new(substitute_expr(r, substitutions))),
        },
        Expr::ParseFloat(string) => Expr::ParseFloat(Box::new(substitute_expr(string, substitutions))),
        Expr::NumberCoerce(value) => Expr::NumberCoerce(Box::new(substitute_expr(value, substitutions))),
        Expr::BigIntCoerce(value) => Expr::BigIntCoerce(Box::new(substitute_expr(value, substitutions))),
        Expr::StringCoerce(value) => Expr::StringCoerce(Box::new(substitute_expr(value, substitutions))),
        Expr::IsNaN(value) => Expr::IsNaN(Box::new(substitute_expr(value, substitutions))),
        Expr::IsUndefinedOrBareNan(value) => Expr::IsUndefinedOrBareNan(Box::new(substitute_expr(value, substitutions))),
        Expr::IsFinite(value) => Expr::IsFinite(Box::new(substitute_expr(value, substitutions))),
        Expr::StaticPluginResolve(value) => Expr::StaticPluginResolve(Box::new(substitute_expr(value, substitutions))),
        // JS Runtime expressions - pass through unchanged (no type substitution needed)
        Expr::JsLoadModule { path } => Expr::JsLoadModule { path: path.clone() },
        Expr::JsGetExport { module_handle, export_name } => Expr::JsGetExport {
            module_handle: Box::new(substitute_expr(module_handle, substitutions)),
            export_name: export_name.clone(),
        },
        Expr::JsCallFunction { module_handle, func_name, args } => Expr::JsCallFunction {
            module_handle: Box::new(substitute_expr(module_handle, substitutions)),
            func_name: func_name.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
        },
        Expr::JsCallMethod { object, method_name, args } => Expr::JsCallMethod {
            object: Box::new(substitute_expr(object, substitutions)),
            method_name: method_name.clone(),
            args: args.iter().map(|a| substitute_expr(a, substitutions)).collect(),
        },
        // OS module expressions - pass through unchanged
        Expr::OsPlatform => Expr::OsPlatform,
        Expr::OsArch => Expr::OsArch,
        Expr::OsHostname => Expr::OsHostname,
        Expr::OsType => Expr::OsType,
        Expr::OsRelease => Expr::OsRelease,
        Expr::OsHomedir => Expr::OsHomedir,
        Expr::OsTmpdir => Expr::OsTmpdir,
        Expr::OsTotalmem => Expr::OsTotalmem,
        Expr::OsFreemem => Expr::OsFreemem,
        Expr::OsCpus => Expr::OsCpus,
        // Catch-all for any other expressions that don't need type substitution
        _ => expr.clone(),
    }
}

/// Substitute types in statements
fn substitute_stmts(stmts: &[Stmt], substitutions: &HashMap<String, Type>) -> Vec<Stmt> {
    stmts.iter().map(|stmt| substitute_stmt(stmt, substitutions)).collect()
}

/// Substitute types in a single statement
fn substitute_stmt(stmt: &Stmt, substitutions: &HashMap<String, Type>) -> Stmt {
    match stmt {
        Stmt::Let { id, name, ty, mutable, init } => Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: substitute_type(ty, substitutions),
            mutable: *mutable,
            init: init.as_ref().map(|e| substitute_expr(e, substitutions)),
        },
        Stmt::Expr(expr) => Stmt::Expr(substitute_expr(expr, substitutions)),
        Stmt::Return(expr) => Stmt::Return(expr.as_ref().map(|e| substitute_expr(e, substitutions))),
        Stmt::If { condition, then_branch, else_branch } => Stmt::If {
            condition: substitute_expr(condition, substitutions),
            then_branch: substitute_stmts(then_branch, substitutions),
            else_branch: else_branch.as_ref().map(|b| substitute_stmts(b, substitutions)),
        },
        Stmt::While { condition, body } => Stmt::While {
            condition: substitute_expr(condition, substitutions),
            body: substitute_stmts(body, substitutions),
        },
        Stmt::DoWhile { body, condition } => Stmt::DoWhile {
            body: substitute_stmts(body, substitutions),
            condition: substitute_expr(condition, substitutions),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(substitute_stmt(body, substitutions)),
        },
        Stmt::For { init, condition, update, body } => Stmt::For {
            init: init.as_ref().map(|s| Box::new(substitute_stmt(s, substitutions))),
            condition: condition.as_ref().map(|e| substitute_expr(e, substitutions)),
            update: update.as_ref().map(|e| substitute_expr(e, substitutions)),
            body: substitute_stmts(body, substitutions),
        },
        Stmt::Break => Stmt::Break,
        Stmt::Continue => Stmt::Continue,
        Stmt::LabeledBreak(label) => Stmt::LabeledBreak(label.clone()),
        Stmt::LabeledContinue(label) => Stmt::LabeledContinue(label.clone()),
        Stmt::Throw(expr) => Stmt::Throw(substitute_expr(expr, substitutions)),
        Stmt::Try { body, catch, finally } => Stmt::Try {
            body: substitute_stmts(body, substitutions),
            catch: catch.as_ref().map(|c| CatchClause {
                param: c.param.clone(),
                body: substitute_stmts(&c.body, substitutions),
            }),
            finally: finally.as_ref().map(|f| substitute_stmts(f, substitutions)),
        },
        Stmt::Switch { discriminant, cases } => Stmt::Switch {
            discriminant: substitute_expr(discriminant, substitutions),
            cases: cases.iter().map(|c| SwitchCase {
                test: c.test.as_ref().map(|t| substitute_expr(t, substitutions)),
                body: substitute_stmts(&c.body, substitutions),
            }).collect(),
        },
    }
}

/// Create a specialized version of a function
pub fn specialize_function(
    func: &Function,
    type_args: &[Type],
    new_id: FuncId,
) -> Function {
    // Build substitution map from type params to concrete types
    let substitutions: HashMap<String, Type> = func.type_params
        .iter()
        .zip(type_args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect();

    // Generate specialized name
    let specialized_name = generate_specialized_name(&func.name, type_args);

    Function {
        id: new_id,
        name: specialized_name,
        type_params: Vec::new(), // Specialized function has no type params
        params: func.params.iter().map(|p| Param {
            id: p.id,
            name: p.name.clone(),
            ty: substitute_type(&p.ty, &substitutions),
            default: p.default.as_ref().map(|d| substitute_expr(d, &substitutions)),
            is_rest: p.is_rest,
        }).collect(),
        return_type: substitute_type(&func.return_type, &substitutions),
        body: substitute_stmts(&func.body, &substitutions),
        is_async: func.is_async,
        is_generator: func.is_generator,
        was_plain_async: false,
        is_exported: false, // Specialized versions are internal
        captures: func.captures.clone(),
        decorators: func.decorators.clone(),
    }
}

/// Create a specialized version of a class
pub fn specialize_class(
    class: &Class,
    type_args: &[Type],
    new_id: ClassId,
) -> Class {
    // Build substitution map from type params to concrete types
    let substitutions: HashMap<String, Type> = class.type_params
        .iter()
        .zip(type_args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect();

    // Generate specialized name
    let specialized_name = generate_specialized_name(&class.name, type_args);
    let ctor_name = format!("{}::constructor", specialized_name);

    Class {
        id: new_id,
        name: specialized_name,
        type_params: Vec::new(), // Specialized class has no type params
        extends: class.extends, // TODO: Handle generic extends
        extends_name: class.extends_name.clone(),
        native_extends: class.native_extends.clone(),
        fields: class.fields.iter().map(|f| ClassField {
            name: f.name.clone(),
            ty: substitute_type(&f.ty, &substitutions),
            init: f.init.as_ref().map(|e| substitute_expr(e, &substitutions)),
            is_private: f.is_private,
            is_readonly: f.is_readonly,
        }).collect(),
        constructor: class.constructor.as_ref().map(|ctor| {
            Function {
                id: ctor.id,
                name: ctor_name.clone(),
                type_params: Vec::new(),
                params: ctor.params.iter().map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, &substitutions),
                    default: p.default.as_ref().map(|d| substitute_expr(d, &substitutions)),
                    is_rest: p.is_rest,
                }).collect(),
                return_type: Type::Void,
                body: substitute_stmts(&ctor.body, &substitutions),
                is_async: false,
                is_generator: false,
                was_plain_async: false,
                is_exported: false,
                captures: ctor.captures.clone(),
                decorators: ctor.decorators.clone(),
            }
        }),
        methods: class.methods.iter().map(|m| {
            Function {
                id: m.id,
                name: m.name.clone(),
                type_params: m.type_params.clone(), // Methods can still be generic
                params: m.params.iter().map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, &substitutions),
                    default: p.default.as_ref().map(|d| substitute_expr(d, &substitutions)),
                    is_rest: p.is_rest,
                }).collect(),
                return_type: substitute_type(&m.return_type, &substitutions),
                body: substitute_stmts(&m.body, &substitutions),
                is_async: m.is_async,
                is_generator: m.is_generator,
                was_plain_async: false,
                is_exported: false,
                captures: m.captures.clone(),
                decorators: m.decorators.clone(),
            }
        }).collect(),
        getters: class.getters.iter().map(|(name, f)| {
            (name.clone(), Function {
                id: f.id,
                name: f.name.clone(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: substitute_type(&f.return_type, &substitutions),
                body: substitute_stmts(&f.body, &substitutions),
                is_async: false,
                is_generator: false,
                was_plain_async: false,
                is_exported: false,
                captures: f.captures.clone(),
                decorators: f.decorators.clone(),
            })
        }).collect(),
        setters: class.setters.iter().map(|(name, f)| {
            (name.clone(), Function {
                id: f.id,
                name: f.name.clone(),
                type_params: Vec::new(),
                params: f.params.iter().map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, &substitutions),
                    default: p.default.as_ref().map(|d| substitute_expr(d, &substitutions)),
                    is_rest: p.is_rest,
                }).collect(),
                return_type: Type::Void,
                body: substitute_stmts(&f.body, &substitutions),
                is_async: false,
                is_generator: false,
                was_plain_async: false,
                is_exported: false,
                captures: f.captures.clone(),
                decorators: f.decorators.clone(),
            })
        }).collect(),
        static_fields: class.static_fields.clone(),
        static_methods: class.static_methods.clone(),
        is_exported: class.is_exported,
    }
}

/// Main monomorphization pass
/// Processes the module and generates specialized versions of generic functions/classes
pub fn monomorphize_module(module: &mut Module) {
    let mut ctx = MonomorphizationContext::new(module);
    let idx = ModuleIndex::new(module);

    // First pass: collect all generic instantiations from the code
    collect_instantiations(module, &mut ctx, &idx);

    // Process work queues until empty
    let mut new_functions = Vec::new();
    let mut new_classes = Vec::new();

    while !ctx.func_work_queue.is_empty() || !ctx.class_work_queue.is_empty() {
        // Process function specializations
        while let Some(request) = ctx.func_work_queue.pop_front() {
            let mangled_args = mangle_type_args(&request.type_args);
            let key = (request.original_id, mangled_args);
            if ctx.processed_funcs.contains(&key) {
                continue;
            }
            ctx.processed_funcs.insert(key);

            // Find the original function
            if let Some(&fi) = idx.func_by_id.get(&request.original_id) {
                let original = &module.functions[fi];
                // Check type parameter constraints
                if let Err(errors) = check_function_constraints(original, &request.type_args, module, &idx) {
                    for err in errors {
                        eprintln!("Warning: Constraint violation in function '{}': {:?}", original.name, err);
                    }
                    // Continue with specialization even on constraint errors (for now)
                }
                let specialized = specialize_function(original, &request.type_args, request.new_id);
                new_functions.push(specialized);
            }
        }

        // Process class specializations
        while let Some(request) = ctx.class_work_queue.pop_front() {
            let mangled_args = mangle_type_args(&request.type_args);
            let key = (request.original_name.clone(), mangled_args);
            if ctx.processed_classes.contains(&key) {
                continue;
            }
            ctx.processed_classes.insert(key);

            // Find the original class
            if let Some(&ci) = idx.class_by_name.get(&request.original_name) {
                let original = &module.classes[ci];
                // Check type parameter constraints
                if let Err(errors) = check_class_constraints(original, &request.type_args, module, &idx) {
                    for err in errors {
                        eprintln!("Warning: Constraint violation in class '{}': {:?}", original.name, err);
                    }
                    // Continue with specialization even on constraint errors (for now)
                }
                let new_id = ctx.fresh_class_id();
                let specialized = specialize_class(original, &request.type_args, new_id);
                new_classes.push(specialized);
            }
        }
    }

    // Add specialized functions and classes to the module
    module.functions.extend(new_functions);
    module.classes.extend(new_classes);

    // Update call sites to use specialized versions
    update_call_sites(module, &ctx);

    // Fill in default arguments for constructor calls
    fill_default_arguments(module);
}

/// Collect all generic instantiations from the module
fn collect_instantiations(module: &Module, ctx: &mut MonomorphizationContext, idx: &ModuleIndex) {
    // Scan all functions for generic calls
    for func in &module.functions {
        collect_instantiations_in_stmts(&func.body, ctx, module, idx);
    }

    // Scan all class methods
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            collect_instantiations_in_stmts(&ctor.body, ctx, module, idx);
        }
        for method in &class.methods {
            collect_instantiations_in_stmts(&method.body, ctx, module, idx);
        }
    }

    // Scan init statements
    collect_instantiations_in_stmts(&module.init, ctx, module, idx);
}

fn collect_instantiations_in_stmts(stmts: &[Stmt], ctx: &mut MonomorphizationContext, module: &Module, idx: &ModuleIndex) {
    for stmt in stmts {
        collect_instantiations_in_stmt(stmt, ctx, module, idx);
    }
}

fn collect_instantiations_in_stmt(stmt: &Stmt, ctx: &mut MonomorphizationContext, module: &Module, idx: &ModuleIndex) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                collect_instantiations_in_expr(expr, ctx, module, idx);
            }
        }
        Stmt::Expr(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_stmts(then_branch, ctx, module, idx);
            if let Some(else_b) = else_branch {
                collect_instantiations_in_stmts(else_b, ctx, module, idx);
            }
        }
        Stmt::While { condition, body } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        Stmt::DoWhile { body, condition } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
            collect_instantiations_in_expr(condition, ctx, module, idx);
        }
        Stmt::Labeled { body, .. } => {
            collect_instantiations_in_stmt(body, ctx, module, idx);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init {
                collect_instantiations_in_stmt(init_stmt, ctx, module, idx);
            }
            if let Some(cond) = condition {
                collect_instantiations_in_expr(cond, ctx, module, idx);
            }
            if let Some(upd) = update {
                collect_instantiations_in_expr(upd, ctx, module, idx);
            }
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        Stmt::Throw(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
        Stmt::Try { body, catch, finally } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
            if let Some(c) = catch {
                collect_instantiations_in_stmts(&c.body, ctx, module, idx);
            }
            if let Some(f) = finally {
                collect_instantiations_in_stmts(f, ctx, module, idx);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_instantiations_in_expr(discriminant, ctx, module, idx);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_instantiations_in_expr(test, ctx, module, idx);
                }
                collect_instantiations_in_stmts(&case.body, ctx, module, idx);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn collect_instantiations_in_expr(expr: &Expr, ctx: &mut MonomorphizationContext, module: &Module, idx: &ModuleIndex) {
    match expr {
        // Check for generic function calls
        Expr::Call { callee, args, type_args } => {
            // First collect in the callee and args
            collect_instantiations_in_expr(callee, ctx, module, idx);
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }

            // Check if callee is a function reference
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                // Find the function and check if it's generic
                if let Some(&fi) = idx.func_by_id.get(func_id) {
                    let func = &module.functions[fi];
                    if !func.type_params.is_empty() {
                        // Use explicit type args if provided, otherwise try to infer
                        let resolved_type_args = if !type_args.is_empty() {
                            Some(type_args.clone())
                        } else {
                            // Try to infer type arguments from the call arguments
                            infer_type_args(func, args, module, idx)
                        };

                        if let Some(ta) = resolved_type_args {
                            ctx.request_func_specialization(*func_id, ta);
                        }
                    }
                }
            }
        }

        // Check for generic class instantiation
        Expr::New { class_name, args, type_args } => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }

            // Find the class
            if let Some(&ci) = idx.class_by_name.get(class_name.as_str()) {
                let class = &module.classes[ci];
                if !class.type_params.is_empty() {
                    // Use explicit type args if provided, otherwise try to infer from constructor
                    let resolved_type_args = if !type_args.is_empty() {
                        Some(type_args.clone())
                    } else if let Some(ref ctor) = class.constructor {
                        // Try to infer from constructor parameters
                        infer_type_args_for_class(class, ctor, args, module, idx)
                    } else {
                        None
                    };

                    if let Some(ta) = resolved_type_args {
                        ctx.request_class_specialization(class_name, ta);
                    }
                }
            }
        }

        // Recurse into other expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            collect_instantiations_in_expr(val, ctx, module, idx);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
        Expr::Logical { left, right, .. } => {
            collect_instantiations_in_expr(left, ctx, module, idx);
            collect_instantiations_in_expr(right, ctx, module, idx);
        }
        Expr::Unary { operand, .. } => {
            collect_instantiations_in_expr(operand, ctx, module, idx);
        }
        Expr::PropertyGet { object, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::PropertyUpdate { object, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
        }
        Expr::IndexGet { object, index } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(index, ctx, module, idx);
        }
        Expr::IndexSet { object, index, value } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(index, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_instantiations_in_expr(v, ctx, module, idx);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_instantiations_in_expr(v, ctx, module, idx);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::ArraySpread(elems) => {
            for e in elems {
                match e {
                    ArrayElement::Expr(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
                    ArrayElement::Spread(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
                }
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_expr(then_expr, ctx, module, idx);
            collect_instantiations_in_expr(else_expr, ctx, module, idx);
        }
        Expr::TypeOf(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::Void(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::Yield { value, .. } => {
            if let Some(v) = value { collect_instantiations_in_expr(v, ctx, module, idx); }
        }
        Expr::InstanceOf { expr, .. } => collect_instantiations_in_expr(expr, ctx, module, idx),
        Expr::Await(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::SuperCall(args) => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::FsReadFileSync(path) => collect_instantiations_in_expr(path, ctx, module, idx),
        Expr::FsWriteFileSync(path, content) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
            collect_instantiations_in_expr(content, ctx, module, idx);
        }
        Expr::FsExistsSync(path) | Expr::FsMkdirSync(path) | Expr::FsUnlinkSync(path) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
        }
        Expr::FsAppendFileSync(path, content) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
            collect_instantiations_in_expr(content, ctx, module, idx);
        }
        Expr::PathJoin(a, b) => {
            collect_instantiations_in_expr(a, ctx, module, idx);
            collect_instantiations_in_expr(b, ctx, module, idx);
        }
        Expr::PathDirname(p) | Expr::PathBasename(p) | Expr::PathExtname(p) | Expr::PathResolve(p) | Expr::PathIsAbsolute(p) => {
            collect_instantiations_in_expr(p, ctx, module, idx);
        }
        Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
            collect_instantiations_in_expr(array, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::ArraySlice { array, start, end } => {
            collect_instantiations_in_expr(array, ctx, module, idx);
            collect_instantiations_in_expr(start, ctx, module, idx);
            if let Some(e) = end {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::ArraySplice { array_id: _, start, delete_count, items } => {
            collect_instantiations_in_expr(start, ctx, module, idx);
            if let Some(dc) = delete_count {
                collect_instantiations_in_expr(dc, ctx, module, idx);
            }
            for item in items {
                collect_instantiations_in_expr(item, ctx, module, idx);
            }
        }
        Expr::StringSplit(string, delimiter) => {
            collect_instantiations_in_expr(string, ctx, module, idx);
            collect_instantiations_in_expr(delimiter, ctx, module, idx);
        }
        Expr::StringFromCharCode(code) => {
            collect_instantiations_in_expr(code, ctx, module, idx);
        }
        Expr::MapNew => {}
        Expr::MapNewFromArray(expr) => { collect_instantiations_in_expr(expr, ctx, module, idx); }
        Expr::MapSet { map, key, value } => {
            collect_instantiations_in_expr(map, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            collect_instantiations_in_expr(map, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
        }
        Expr::MapSize(map) | Expr::MapClear(map) |
        Expr::MapEntries(map) | Expr::MapKeys(map) | Expr::MapValues(map) => {
            collect_instantiations_in_expr(map, ctx, module, idx);
        }
        Expr::SetNew => {}
        Expr::SetNewFromArray(expr) => { collect_instantiations_in_expr(expr, ctx, module, idx); }
        Expr::SetAdd { set_id: _, value } => {
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            collect_instantiations_in_expr(set, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            collect_instantiations_in_expr(set, ctx, module, idx);
        }
        // JSON operations
        Expr::JsonParse(expr) | Expr::JsonStringify(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        // Math operations
        Expr::MathFloor(expr) | Expr::MathCeil(expr) | Expr::MathRound(expr) |
        Expr::MathAbs(expr) | Expr::MathSqrt(expr) |
        Expr::MathLog(expr) | Expr::MathLog2(expr) | Expr::MathLog10(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            collect_instantiations_in_expr(base, ctx, module, idx);
            collect_instantiations_in_expr(exp, ctx, module, idx);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
            collect_instantiations_in_expr(e, ctx, module, idx);
        }
        Expr::MathRandom => {}
        // Crypto operations
        Expr::CryptoRandomBytes(expr) | Expr::CryptoSha256(expr) | Expr::CryptoMd5(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::CryptoRandomUUID => {}
        // Date operations
        Expr::DateNow => {}
        Expr::DateNew(timestamp) => {
            if let Some(ts) = timestamp {
                collect_instantiations_in_expr(ts, ctx, module, idx);
            }
        }
        Expr::DateGetTime(date) | Expr::DateToISOString(date) |
        Expr::DateGetFullYear(date) | Expr::DateGetMonth(date) | Expr::DateGetDate(date) |
        Expr::DateGetHours(date) | Expr::DateGetMinutes(date) | Expr::DateGetSeconds(date) |
        Expr::DateGetMilliseconds(date) => {
            collect_instantiations_in_expr(date, ctx, module, idx);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::Closure { body, .. } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        // Primitives and simple references don't need processing
        _ => {}
    }
}

/// Lightweight function info for inference during update phase
#[derive(Clone)]
struct FuncInfo {
    id: FuncId,
    type_params: Vec<perry_types::TypeParam>,
    params: Vec<Param>,
    return_type: Type,
}

/// Lightweight class info for inference during update phase
#[derive(Clone)]
struct ClassInfo {
    name: String,
    type_params: Vec<perry_types::TypeParam>,
    constructor_params: Option<Vec<Param>>,
}

/// Lookup table for type inference during update phase
struct InferenceLookup {
    funcs: HashMap<FuncId, FuncInfo>,
    classes: HashMap<String, ClassInfo>,
}

impl InferenceLookup {
    fn from_module(module: &Module) -> Self {
        let funcs = module.functions.iter()
            .map(|f| (f.id, FuncInfo {
                id: f.id,
                type_params: f.type_params.clone(),
                params: f.params.clone(),
                return_type: f.return_type.clone(),
            }))
            .collect();

        let classes = module.classes.iter()
            .map(|c| (c.name.clone(), ClassInfo {
                name: c.name.clone(),
                type_params: c.type_params.clone(),
                constructor_params: c.constructor.as_ref().map(|ctor| ctor.params.clone()),
            }))
            .collect();

        Self { funcs, classes }
    }
}

/// Update call sites to use specialized versions
fn update_call_sites(module: &mut Module, ctx: &MonomorphizationContext) {
    // Build lookup table for inference (before mutating)
    let lookup = InferenceLookup::from_module(module);

    // Update all functions
    for func in &mut module.functions {
        update_call_sites_in_stmts(&mut func.body, ctx, &lookup);
    }

    // Update all class methods
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            update_call_sites_in_stmts(&mut ctor.body, ctx, &lookup);
        }
        for method in &mut class.methods {
            update_call_sites_in_stmts(&mut method.body, ctx, &lookup);
        }
        for method in &mut class.static_methods {
            update_call_sites_in_stmts(&mut method.body, ctx, &lookup);
        }
    }

    // Update init statements
    update_call_sites_in_stmts(&mut module.init, ctx, &lookup);
}

fn update_call_sites_in_stmts(stmts: &mut [Stmt], ctx: &MonomorphizationContext, lookup: &InferenceLookup) {
    for stmt in stmts {
        update_call_sites_in_stmt(stmt, ctx, lookup);
    }
}

fn update_call_sites_in_stmt(stmt: &mut Stmt, ctx: &MonomorphizationContext, lookup: &InferenceLookup) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                update_call_sites_in_expr(expr, ctx, lookup);
            }
        }
        Stmt::Expr(expr) => update_call_sites_in_expr(expr, ctx, lookup),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_stmts(then_branch, ctx, lookup);
            if let Some(else_b) = else_branch {
                update_call_sites_in_stmts(else_b, ctx, lookup);
            }
        }
        Stmt::While { condition, body } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        Stmt::DoWhile { body, condition } => {
            update_call_sites_in_stmts(body, ctx, lookup);
            update_call_sites_in_expr(condition, ctx, lookup);
        }
        Stmt::Labeled { body, .. } => {
            update_call_sites_in_stmt(body, ctx, lookup);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init {
                update_call_sites_in_stmt(init_stmt, ctx, lookup);
            }
            if let Some(cond) = condition {
                update_call_sites_in_expr(cond, ctx, lookup);
            }
            if let Some(upd) = update {
                update_call_sites_in_expr(upd, ctx, lookup);
            }
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        Stmt::Throw(expr) => update_call_sites_in_expr(expr, ctx, lookup),
        Stmt::Try { body, catch, finally } => {
            update_call_sites_in_stmts(body, ctx, lookup);
            if let Some(c) = catch {
                update_call_sites_in_stmts(&mut c.body, ctx, lookup);
            }
            if let Some(f) = finally {
                update_call_sites_in_stmts(f, ctx, lookup);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            update_call_sites_in_expr(discriminant, ctx, lookup);
            for case in cases {
                if let Some(ref mut test) = case.test {
                    update_call_sites_in_expr(test, ctx, lookup);
                }
                update_call_sites_in_stmts(&mut case.body, ctx, lookup);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn update_call_sites_in_expr(expr: &mut Expr, ctx: &MonomorphizationContext, lookup: &InferenceLookup) {
    match expr {
        // Update generic function calls to use specialized version
        Expr::Call { callee, args, type_args } => {
            // First update the callee and args recursively
            update_call_sites_in_expr(callee, ctx, lookup);
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }

            // Check if callee is a FuncRef
            if let Expr::FuncRef(func_id) = callee.as_mut() {
                // Get resolved type args - either explicit or inferred
                let resolved_type_args = if !type_args.is_empty() {
                    Some(type_args.clone())
                } else if let Some(func_info) = lookup.funcs.get(func_id) {
                    if !func_info.type_params.is_empty() {
                        // Try to infer type arguments
                        infer_type_args_from_lookup(func_info, args, lookup)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // If we have type args (explicit or inferred), update to specialized version
                if let Some(ta) = resolved_type_args {
                    let mangled_args = mangle_type_args(&ta);
                    let key = (*func_id, mangled_args);
                    if let Some(&specialized_id) = ctx.specialized_funcs.get(&key) {
                        *func_id = specialized_id;
                        type_args.clear();
                    }
                }
            }
        }

        // Update generic class instantiation to use specialized class
        Expr::New { class_name, args, type_args } => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }

            // Get resolved type args - either explicit or inferred
            let resolved_type_args = if !type_args.is_empty() {
                Some(type_args.clone())
            } else if let Some(class_info) = lookup.classes.get(class_name) {
                if !class_info.type_params.is_empty() {
                    // Try to infer type arguments from constructor
                    infer_type_args_for_class_from_lookup(class_info, args, lookup)
                } else {
                    None
                }
            } else {
                None
            };

            // If we have type args (explicit or inferred), update to specialized class
            if let Some(ta) = resolved_type_args {
                let mangled_args = mangle_type_args(&ta);
                let key = (class_name.clone(), mangled_args);
                if let Some(specialized_name) = ctx.specialized_classes.get(&key) {
                    *class_name = specialized_name.clone();
                    type_args.clear();
                }
            }
        }

        // Recurse into other expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            update_call_sites_in_expr(val, ctx, lookup);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
        Expr::Logical { left, right, .. } => {
            update_call_sites_in_expr(left, ctx, lookup);
            update_call_sites_in_expr(right, ctx, lookup);
        }
        Expr::Unary { operand, .. } => {
            update_call_sites_in_expr(operand, ctx, lookup);
        }
        Expr::PropertyGet { object, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
        }
        Expr::PropertySet { object, value, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::PropertyUpdate { object, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
        }
        Expr::IndexGet { object, index } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(index, ctx, lookup);
        }
        Expr::IndexSet { object, index, value } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(index, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                update_call_sites_in_expr(v, ctx, lookup);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts.iter_mut() {
                update_call_sites_in_expr(v, ctx, lookup);
            }
        }
        Expr::Array(elems) => {
            for e in elems.iter_mut() {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::ArraySpread(elems) => {
            for e in elems.iter_mut() {
                match e {
                    ArrayElement::Expr(expr) => update_call_sites_in_expr(expr, ctx, lookup),
                    ArrayElement::Spread(expr) => update_call_sites_in_expr(expr, ctx, lookup),
                }
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_expr(then_expr, ctx, lookup);
            update_call_sites_in_expr(else_expr, ctx, lookup);
        }
        Expr::TypeOf(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::Void(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::Yield { value, .. } => {
            if let Some(v) = value { update_call_sites_in_expr(v, ctx, lookup); }
        }
        Expr::InstanceOf { expr, .. } => update_call_sites_in_expr(expr, ctx, lookup),
        Expr::Await(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::SuperCall(args) => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                update_call_sites_in_expr(obj, ctx, lookup);
            }
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::FsReadFileSync(path) => update_call_sites_in_expr(path, ctx, lookup),
        Expr::FsWriteFileSync(path, content) => {
            update_call_sites_in_expr(path, ctx, lookup);
            update_call_sites_in_expr(content, ctx, lookup);
        }
        Expr::FsExistsSync(path) | Expr::FsMkdirSync(path) | Expr::FsUnlinkSync(path) => {
            update_call_sites_in_expr(path, ctx, lookup);
        }
        Expr::FsAppendFileSync(path, content) => {
            update_call_sites_in_expr(path, ctx, lookup);
            update_call_sites_in_expr(content, ctx, lookup);
        }
        Expr::PathJoin(a, b) => {
            update_call_sites_in_expr(a, ctx, lookup);
            update_call_sites_in_expr(b, ctx, lookup);
        }
        Expr::PathDirname(p) | Expr::PathBasename(p) | Expr::PathExtname(p) | Expr::PathResolve(p) | Expr::PathIsAbsolute(p) => {
            update_call_sites_in_expr(p, ctx, lookup);
        }
        Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
            update_call_sites_in_expr(array, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::ArraySlice { array, start, end } => {
            update_call_sites_in_expr(array, ctx, lookup);
            update_call_sites_in_expr(start, ctx, lookup);
            if let Some(e) = end {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::ArraySplice { array_id: _, start, delete_count, items } => {
            update_call_sites_in_expr(start, ctx, lookup);
            if let Some(dc) = delete_count {
                update_call_sites_in_expr(dc, ctx, lookup);
            }
            for item in items {
                update_call_sites_in_expr(item, ctx, lookup);
            }
        }
        Expr::StringSplit(string, delimiter) => {
            update_call_sites_in_expr(string, ctx, lookup);
            update_call_sites_in_expr(delimiter, ctx, lookup);
        }
        Expr::StringFromCharCode(code) => {
            update_call_sites_in_expr(code, ctx, lookup);
        }
        Expr::MapNew => {}
        Expr::MapNewFromArray(expr) => { update_call_sites_in_expr(expr, ctx, lookup); }
        Expr::MapSet { map, key, value } => {
            update_call_sites_in_expr(map, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            update_call_sites_in_expr(map, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
        }
        Expr::MapSize(map) | Expr::MapClear(map) |
        Expr::MapEntries(map) | Expr::MapKeys(map) | Expr::MapValues(map) => {
            update_call_sites_in_expr(map, ctx, lookup);
        }
        Expr::SetNew => {}
        Expr::SetNewFromArray(expr) => { update_call_sites_in_expr(expr, ctx, lookup); }
        Expr::SetAdd { set_id: _, value } => {
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            update_call_sites_in_expr(set, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            update_call_sites_in_expr(set, ctx, lookup);
        }
        // JSON operations
        Expr::JsonParse(expr) | Expr::JsonStringify(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        // Math operations
        Expr::MathFloor(expr) | Expr::MathCeil(expr) | Expr::MathRound(expr) |
        Expr::MathAbs(expr) | Expr::MathSqrt(expr) |
        Expr::MathLog(expr) | Expr::MathLog2(expr) | Expr::MathLog10(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            update_call_sites_in_expr(base, ctx, lookup);
            update_call_sites_in_expr(exp, ctx, lookup);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
            update_call_sites_in_expr(e, ctx, lookup);
        }
        Expr::MathRandom => {}
        // Crypto operations
        Expr::CryptoRandomBytes(expr) | Expr::CryptoSha256(expr) | Expr::CryptoMd5(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::CryptoRandomUUID => {}
        // Date operations
        Expr::DateNow => {}
        Expr::DateNew(timestamp) => {
            if let Some(ts) = timestamp {
                update_call_sites_in_expr(ts, ctx, lookup);
            }
        }
        Expr::DateGetTime(date) | Expr::DateToISOString(date) |
        Expr::DateGetFullYear(date) | Expr::DateGetMonth(date) | Expr::DateGetDate(date) |
        Expr::DateGetHours(date) | Expr::DateGetMinutes(date) | Expr::DateGetSeconds(date) |
        Expr::DateGetMilliseconds(date) => {
            update_call_sites_in_expr(date, ctx, lookup);
        }
        Expr::Sequence(exprs) => {
            for e in exprs.iter_mut() {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::Closure { body, .. } => {
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        // Primitives and simple references don't need updating
        _ => {}
    }
}

/// Infer type arguments using the lightweight FuncInfo (for update phase)
fn infer_type_args_from_lookup(
    func_info: &FuncInfo,
    args: &[Expr],
    lookup: &InferenceLookup,
) -> Option<Vec<Type>> {
    if func_info.type_params.is_empty() {
        return None;
    }

    let mut bindings: HashMap<String, Type> = HashMap::new();

    for (param, arg) in func_info.params.iter().zip(args.iter()) {
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        if let Some(arg_ty) = infer_expr_type_from_lookup(arg, lookup) {
            if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                return None;
            }
        }
    }

    let mut inferred_args = Vec::new();
    for type_param in &func_info.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            inferred_args.push((**default).clone());
        } else {
            return None;
        }
    }

    Some(inferred_args)
}

/// Infer type arguments for class using the lightweight ClassInfo (for update phase)
fn infer_type_args_for_class_from_lookup(
    class_info: &ClassInfo,
    args: &[Expr],
    lookup: &InferenceLookup,
) -> Option<Vec<Type>> {
    if class_info.type_params.is_empty() {
        return None;
    }

    let ctor_params = class_info.constructor_params.as_ref()?;

    let mut bindings: HashMap<String, Type> = HashMap::new();

    for (param, arg) in ctor_params.iter().zip(args.iter()) {
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        if let Some(arg_ty) = infer_expr_type_from_lookup(arg, lookup) {
            if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                return None;
            }
        }
    }

    let mut inferred_args = Vec::new();
    for type_param in &class_info.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            inferred_args.push((**default).clone());
        } else {
            return None;
        }
    }

    Some(inferred_args)
}

/// Infer expression type using the lookup table (for update phase)
fn infer_expr_type_from_lookup(expr: &Expr, lookup: &InferenceLookup) -> Option<Type> {
    match expr {
        Expr::Number(_) => Some(Type::Number),
        Expr::String(_) => Some(Type::String),
        Expr::Bool(_) => Some(Type::Boolean),
        Expr::Null => Some(Type::Null),
        Expr::Undefined => Some(Type::Void),
        Expr::BigInt(_) => Some(Type::BigInt),

        Expr::Array(elems) => {
            if let Some(first) = elems.first() {
                if let Some(elem_ty) = infer_expr_type_from_lookup(first, lookup) {
                    return Some(Type::Array(Box::new(elem_ty)));
                }
            }
            Some(Type::Array(Box::new(Type::Any)))
        }

        Expr::Object(_) | Expr::ObjectSpread { .. } => Some(Type::Object(ObjectType::default())),

        Expr::Call { callee, type_args, .. } => {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                if let Some(func_info) = lookup.funcs.get(func_id) {
                    if !type_args.is_empty() && !func_info.type_params.is_empty() {
                        let subs: HashMap<String, Type> = func_info.type_params.iter()
                            .zip(type_args.iter())
                            .map(|(p, t)| (p.name.clone(), t.clone()))
                            .collect();
                        return Some(substitute_type(&func_info.return_type, &subs));
                    }
                    return Some(func_info.return_type.clone());
                }
            }
            None
        }

        Expr::New { class_name, .. } => Some(Type::Named(class_name.clone())),

        Expr::Binary { op, .. } => {
            match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div |
                BinaryOp::Mod | BinaryOp::Pow | BinaryOp::BitAnd | BinaryOp::BitOr |
                BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr => Some(Type::Number),
            }
        }

        Expr::Compare { .. } => Some(Type::Boolean),

        Expr::Logical { left, right, .. } => {
            infer_expr_type_from_lookup(left, lookup)
                .or_else(|| infer_expr_type_from_lookup(right, lookup))
        }

        Expr::Unary { op, .. } => {
            match op {
                UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => Some(Type::Number),
                UnaryOp::Not => Some(Type::Boolean),
            }
        }

        Expr::TypeOf(_) => Some(Type::String),
        Expr::Void(_) => Some(Type::Void),
        Expr::InstanceOf { .. } => Some(Type::Boolean),

        Expr::Conditional { then_expr, .. } => infer_expr_type_from_lookup(then_expr, lookup),

        _ => None,
    }
}

// ============================================================================
// Default Argument Filling
// ============================================================================

/// Fill in default arguments for New expressions where fewer args are provided
/// than the constructor expects
fn fill_default_arguments(module: &mut Module) {
    // Build a map of class name -> constructor param defaults
    let mut ctor_defaults: HashMap<String, Vec<Option<Expr>>> = HashMap::new();
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            let defaults: Vec<Option<Expr>> = ctor.params.iter()
                .map(|p| p.default.clone())
                .collect();
            ctor_defaults.insert(class.name.clone(), defaults);
        }
    }

    // Fill defaults in init statements
    fill_defaults_in_stmts(&mut module.init, &ctor_defaults);

    // Fill defaults in function bodies
    for func in &mut module.functions {
        fill_defaults_in_stmts(&mut func.body, &ctor_defaults);
    }

    // Fill defaults in class methods
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            fill_defaults_in_stmts(&mut ctor.body, &ctor_defaults);
        }
        for method in &mut class.methods {
            fill_defaults_in_stmts(&mut method.body, &ctor_defaults);
        }
    }
}

fn fill_defaults_in_stmts(stmts: &mut [Stmt], ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    for stmt in stmts {
        fill_defaults_in_stmt(stmt, ctor_defaults);
    }
}

fn fill_defaults_in_stmt(stmt: &mut Stmt, ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                fill_defaults_in_expr(expr, ctor_defaults);
            }
        }
        Stmt::Expr(expr) => fill_defaults_in_expr(expr, ctor_defaults),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                fill_defaults_in_expr(e, ctor_defaults);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_stmts(then_branch, ctor_defaults);
            if let Some(else_b) = else_branch {
                fill_defaults_in_stmts(else_b, ctor_defaults);
            }
        }
        Stmt::While { condition, body } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Stmt::DoWhile { body, condition } => {
            fill_defaults_in_stmts(body, ctor_defaults);
            fill_defaults_in_expr(condition, ctor_defaults);
        }
        Stmt::Labeled { body, .. } => {
            fill_defaults_in_stmt(body, ctor_defaults);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(init_stmt) = init {
                fill_defaults_in_stmt(init_stmt, ctor_defaults);
            }
            if let Some(cond) = condition {
                fill_defaults_in_expr(cond, ctor_defaults);
            }
            if let Some(upd) = update {
                fill_defaults_in_expr(upd, ctor_defaults);
            }
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Stmt::Throw(expr) => fill_defaults_in_expr(expr, ctor_defaults),
        Stmt::Try { body, catch, finally } => {
            fill_defaults_in_stmts(body, ctor_defaults);
            if let Some(ref mut c) = catch {
                fill_defaults_in_stmts(&mut c.body, ctor_defaults);
            }
            if let Some(f) = finally {
                fill_defaults_in_stmts(f, ctor_defaults);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            fill_defaults_in_expr(discriminant, ctor_defaults);
            for case in cases {
                fill_defaults_in_stmts(&mut case.body, ctor_defaults);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn fill_defaults_in_expr(expr: &mut Expr, ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    match expr {
        Expr::New { class_name, args, .. } => {
            // First, recurse into the arguments
            for arg in args.iter_mut() {
                fill_defaults_in_expr(arg, ctor_defaults);
            }

            // Check if we need to fill in defaults
            if let Some(defaults) = ctor_defaults.get(class_name) {
                let param_count = defaults.len();
                let arg_count = args.len();

                if arg_count < param_count {
                    // Fill in missing arguments with defaults
                    for i in arg_count..param_count {
                        if let Some(ref default_expr) = defaults[i] {
                            args.push(default_expr.clone());
                        } else {
                            // No default for this parameter - this is an error
                            // For now, push a null placeholder
                            args.push(Expr::Null);
                        }
                    }
                }
            }
        }
        // Recurse into sub-expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            fill_defaults_in_expr(val, ctor_defaults);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
        Expr::Logical { left, right, .. } => {
            fill_defaults_in_expr(left, ctor_defaults);
            fill_defaults_in_expr(right, ctor_defaults);
        }
        Expr::Unary { operand, .. } => {
            fill_defaults_in_expr(operand, ctor_defaults);
        }
        Expr::Update { .. } => {
            // Update expressions (++/--) don't contain sub-expressions
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_expr(then_expr, ctor_defaults);
            fill_defaults_in_expr(else_expr, ctor_defaults);
        }
        Expr::Call { callee, args, .. } => {
            fill_defaults_in_expr(callee, ctor_defaults);
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                fill_defaults_in_expr(elem, ctor_defaults);
            }
        }
        Expr::Object(fields) => {
            for (_, val) in fields {
                fill_defaults_in_expr(val, ctor_defaults);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, val) in parts {
                fill_defaults_in_expr(val, ctor_defaults);
            }
        }
        Expr::IndexGet { object, index } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(index, ctor_defaults);
        }
        Expr::IndexSet { object, index, value } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(index, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::PropertyGet { object, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
        }
        Expr::PropertySet { object, value, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::PropertyUpdate { object, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
        }
        Expr::Await(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::TypeOf(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::Void(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value { fill_defaults_in_expr(v, ctor_defaults); }
        }
        Expr::InstanceOf { expr, .. } => {
            fill_defaults_in_expr(expr, ctor_defaults);
        }
        Expr::Closure { body, .. } => {
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fill_defaults_in_expr(obj, ctor_defaults);
            }
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::SuperCall(args) => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_types::TypeParam;

    #[test]
    fn test_mangle_type() {
        assert_eq!(mangle_type(&Type::Number), "num");
        assert_eq!(mangle_type(&Type::String), "str");
        assert_eq!(mangle_type(&Type::Array(Box::new(Type::Number))), "arr_num");
    }

    #[test]
    fn test_generate_specialized_name() {
        assert_eq!(
            generate_specialized_name("identity", &[Type::Number]),
            "identity$num"
        );
        assert_eq!(
            generate_specialized_name("pair", &[Type::Number, Type::String]),
            "pair$num_str"
        );
    }

    #[test]
    fn test_substitute_type() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), Type::Number);

        assert_eq!(
            substitute_type(&Type::TypeVar("T".to_string()), &subs),
            Type::Number
        );
        assert_eq!(
            substitute_type(&Type::Array(Box::new(Type::TypeVar("T".to_string()))), &subs),
            Type::Array(Box::new(Type::Number))
        );
    }

    #[test]
    fn test_monomorphize_generic_function() {
        // Create a generic identity function: function identity<T>(x: T): T { return x; }
        let identity_func = Function {
            id: 1,
            name: "identity".to_string(),
            type_params: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            params: vec![Param {
                id: 0,
                name: "x".to_string(),
                ty: Type::TypeVar("T".to_string()),
                default: None,
                is_rest: false,
            }],
            return_type: Type::TypeVar("T".to_string()),
            body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
            is_async: false,
            is_generator: false,
            was_plain_async: false,
            is_exported: true,
            captures: vec![],
            decorators: vec![],
        };

        // Create a module with the generic function and a call to it with type args
        let mut module = Module::new("test");
        module.functions.push(identity_func);

        // Add init code that calls identity<number>(42)
        module.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::Number(42.0)],
            type_args: vec![Type::Number],
        }));

        // Run monomorphization
        monomorphize_module(&mut module);

        // Verify that a specialized function was created
        assert_eq!(module.functions.len(), 2, "Should have original + specialized function");

        // Find the specialized function
        let specialized = module.functions.iter()
            .find(|f| f.name == "identity$num")
            .expect("Specialized function identity$num should exist");

        // Verify the specialized function has correct types
        assert!(specialized.type_params.is_empty(), "Specialized function should have no type params");
        assert_eq!(specialized.params[0].ty, Type::Number, "Param should be Number");
        assert_eq!(specialized.return_type, Type::Number, "Return type should be Number");
    }

    #[test]
    fn test_monomorphize_updates_call_sites() {
        // Create a generic function
        let identity_func = Function {
            id: 1,
            name: "identity".to_string(),
            type_params: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            params: vec![Param {
                id: 0,
                name: "x".to_string(),
                ty: Type::TypeVar("T".to_string()),
                default: None,
                is_rest: false,
            }],
            return_type: Type::TypeVar("T".to_string()),
            body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
            is_async: false,
            is_generator: false,
            was_plain_async: false,
            is_exported: true,
            captures: vec![],
            decorators: vec![],
        };

        let mut module = Module::new("test");
        module.functions.push(identity_func);

        // Add call to identity<string>("hello")
        module.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::String("hello".to_string())],
            type_args: vec![Type::String],
        }));

        // Run monomorphization
        monomorphize_module(&mut module);

        // Check that the call site was updated to use the specialized function
        if let Stmt::Expr(Expr::Call { callee, type_args, .. }) = &module.init[0] {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                // The call should now reference the specialized function (id >= 1000)
                assert!(*func_id >= 1000, "Call should reference specialized function, got id {}", func_id);
                // Type args should be cleared
                assert!(type_args.is_empty(), "Type args should be cleared after monomorphization");
            } else {
                panic!("Expected FuncRef callee");
            }
        } else {
            panic!("Expected Call expression");
        }
    }

    #[test]
    fn test_type_inference_from_arguments() {
        // Create a generic identity function: function identity<T>(x: T): T { return x; }
        let identity_func = Function {
            id: 1,
            name: "identity".to_string(),
            type_params: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            params: vec![Param {
                id: 0,
                name: "x".to_string(),
                ty: Type::TypeVar("T".to_string()),
                default: None,
                is_rest: false,
            }],
            return_type: Type::TypeVar("T".to_string()),
            body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
            is_async: false,
            is_generator: false,
            was_plain_async: false,
            is_exported: true,
            captures: vec![],
            decorators: vec![],
        };

        let mut module = Module::new("test");
        module.functions.push(identity_func);

        // Add call to identity(42) WITHOUT explicit type args - should infer number
        module.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::Number(42.0)],
            type_args: vec![], // Empty - should be inferred!
        }));

        // Run monomorphization
        monomorphize_module(&mut module);

        // Verify that a specialized function was created even without explicit type args
        assert_eq!(module.functions.len(), 2, "Should have original + specialized function");

        // Find the specialized function
        let specialized = module.functions.iter()
            .find(|f| f.name == "identity$num")
            .expect("Specialized function identity$num should exist (inferred from Number argument)");

        // Verify the specialized function has correct types
        assert!(specialized.type_params.is_empty(), "Specialized function should have no type params");
        assert_eq!(specialized.params[0].ty, Type::Number, "Param should be Number");
        assert_eq!(specialized.return_type, Type::Number, "Return type should be Number");

        // Check that the call site was updated to use the specialized function
        if let Stmt::Expr(Expr::Call { callee, type_args, .. }) = &module.init[0] {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                // The call should now reference the specialized function (id >= 1000)
                assert!(*func_id >= 1000, "Call should reference specialized function, got id {}", func_id);
                // Type args should remain empty
                assert!(type_args.is_empty(), "Type args should be empty");
            } else {
                panic!("Expected FuncRef callee");
            }
        } else {
            panic!("Expected Call expression");
        }
    }

    #[test]
    fn test_type_inference_string() {
        // Create a generic identity function
        let identity_func = Function {
            id: 1,
            name: "identity".to_string(),
            type_params: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            params: vec![Param {
                id: 0,
                name: "x".to_string(),
                ty: Type::TypeVar("T".to_string()),
                default: None,
                is_rest: false,
            }],
            return_type: Type::TypeVar("T".to_string()),
            body: vec![Stmt::Return(Some(Expr::LocalGet(0)))],
            is_async: false,
            is_generator: false,
            was_plain_async: false,
            is_exported: true,
            captures: vec![],
            decorators: vec![],
        };

        let mut module = Module::new("test");
        module.functions.push(identity_func);

        // Add call to identity("hello") WITHOUT explicit type args - should infer string
        module.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::String("hello".to_string())],
            type_args: vec![], // Empty - should be inferred!
        }));

        // Run monomorphization
        monomorphize_module(&mut module);

        // Find the specialized function
        let specialized = module.functions.iter()
            .find(|f| f.name == "identity$str")
            .expect("Specialized function identity$str should exist (inferred from String argument)");

        // Verify the specialized function has correct types
        assert_eq!(specialized.params[0].ty, Type::String, "Param should be String");
        assert_eq!(specialized.return_type, Type::String, "Return type should be String");
    }
}
