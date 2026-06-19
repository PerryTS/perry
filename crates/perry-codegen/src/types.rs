//! LLVM IR type constants.
//!
//! LLVM types are plain strings in textual IR; we just use `&'static str`
//! aliases so the codegen reads like anvil's TypeScript port.

pub type LlvmType = &'static str;

pub const DOUBLE: LlvmType = "double";
pub const F32: LlvmType = "float";
pub const I128: LlvmType = "i128";
pub const I64: LlvmType = "i64";
pub const I32: LlvmType = "i32";
pub const I16: LlvmType = "i16";
pub const I8: LlvmType = "i8";
pub const I1: LlvmType = "i1";
pub const PTR: LlvmType = "ptr";
pub const VOID: LlvmType = "void";
