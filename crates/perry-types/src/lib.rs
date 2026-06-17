//! Type system for Perry
//!
//! Defines the type representations used throughout the compiler,
//! from parsing through code generation.

use std::collections::HashMap;

/// Unique identifier for types
pub type TypeId = u32;

/// Unique identifier for functions
pub type FuncId = u32;

/// Unique identifier for local variables
pub type LocalId = u32;

/// Unique identifier for global variables
pub type GlobalId = u32;

/// Core type representation
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Void type (undefined in JS terms)
    Void,
    /// Null type
    Null,
    /// Boolean type
    Boolean,
    /// Number type (f64)
    Number,
    /// Integer type (optimization for known integers)
    Int32,
    /// BigInt type (arbitrary precision)
    BigInt,
    /// String type
    String,
    /// String literal type (e.g. `"./a.ts"`). Most runtime/type-analysis
    /// paths treat this as `String`; AOT-only analyses can inspect the value.
    StringLiteral(String),
    /// Symbol type
    Symbol,
    /// Array type with element type
    Array(Box<Type>),
    /// Tuple type with fixed element types
    Tuple(Vec<Type>),
    /// Object type with known properties
    Object(ObjectType),
    /// Function type
    Function(FunctionType),
    /// Union type (e.g., string | number)
    Union(Vec<Type>),
    /// Promise type
    Promise(Box<Type>),
    /// Any type (boxed value, escape hatch)
    Any,
    /// Unknown type (requires type guards)
    Unknown,
    /// Never type (unreachable)
    Never,
    /// Reference to a named type (interface, class, type alias)
    Named(String),
    /// Type parameter reference (e.g., T in function<T>)
    /// This refers to a type parameter by name
    TypeVar(String),
    /// Generic type instantiation (e.g., Array<number>, Box<string>)
    /// Represents a generic type with concrete type arguments
    Generic {
        /// The base type name (e.g., "Array", "Map", "Box")
        base: String,
        /// Concrete type arguments
        type_args: Vec<Type>,
    },
}

/// Type parameter definition (used in generic functions/classes)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    /// Name of the type parameter (e.g., "T", "K", "V")
    pub name: String,
    /// Upper bound constraint (e.g., T extends SomeType)
    pub constraint: Option<Box<Type>>,
    /// Default type (e.g., T = string)
    pub default: Option<Box<Type>>,
}

/// Object type with property information
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectType {
    /// Optional name (for classes/interfaces)
    pub name: Option<String>,
    /// Property name -> type mapping
    pub properties: HashMap<String, PropertyInfo>,
    /// Declared source order for closed object type literals/interfaces when
    /// an order-sensitive lowering needs it. Ordinary structural lookups must
    /// continue to use `properties`.
    pub property_order: Option<Vec<String>>,
    /// Index signature (if any)
    pub index_signature: Option<Box<Type>>,
}

/// Property information including mutability
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyInfo {
    pub ty: Type,
    pub optional: bool,
    pub readonly: bool,
}

/// Function type information
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    /// Parameter types with names
    pub params: Vec<(String, Type, bool)>, // (name, type, optional)
    /// Return type
    pub return_type: Box<Type>,
    /// Whether the function is async
    pub is_async: bool,
    /// Whether the function is a generator
    pub is_generator: bool,
}

impl Type {
    /// Check if this type is a primitive (number, string, boolean, etc.)
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::Void
                | Type::Null
                | Type::Boolean
                | Type::Number
                | Type::Int32
                | Type::BigInt
                | Type::String
                | Type::StringLiteral(_)
                | Type::Symbol
        )
    }

    /// Check if this type could be undefined/null
    pub fn is_nullable(&self) -> bool {
        matches!(self, Type::Void | Type::Null | Type::Any | Type::Unknown)
    }
}
