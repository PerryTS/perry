//! The WIT contract shared by component binding generation and validation.
//! TypeScript declarations never substitute `any` for an unsupported WIT type.
pub mod canonical;
mod guest_c;
mod resolve;
mod typescript;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use guest_c::canonical_guest;
pub use resolve::{parse, parse_file};
pub use typescript::{typescript, typescript_guest};

/// This is a binding manifest version, independent of a WIT package version.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub schema_version: u32,
    pub name: String,
    pub imports: Vec<Interface>,
    pub exports: Vec<Interface>,
    pub types: BTreeMap<String, Definition>,
}

/// An empty name denotes freestanding world functions. Other names retain the
/// exact component import/export identifier, including the package version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    pub functions: Vec<Function>,
    pub resources: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub name: String,
    pub params: Vec<Field>,
    pub result: Option<Type>,
    pub kind: FunctionKind,
    /// Canonical async calling convention; absent in legacy synchronous manifests.
    #[serde(default, rename = "async", skip_serializing_if = "is_false")]
    pub async_: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "resource", rename_all = "kebab-case")]
pub enum FunctionKind {
    Function,
    Constructor(String),
    Method(String),
    Static(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum Type {
    Bool,
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
    F32,
    F64,
    Char,
    String,
    Ref(String),
    Record(Vec<Field>),
    Tuple(Vec<Type>),
    Flags(Vec<String>),
    Enum(Vec<String>),
    Variant(Vec<Case>),
    Option(Box<Type>),
    Result {
        ok: Option<Box<Type>>,
        err: Option<Box<Type>>,
    },
    List(Box<Type>),
    Future(Option<Box<Type>>),
    Stream(Option<Box<Type>>),
    Resource,
    Own(String),
    Borrow(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub ty: Option<Type>,
}

#[cfg(test)]
mod tests;
