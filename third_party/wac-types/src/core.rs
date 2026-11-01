use indexmap::IndexMap;
use std::fmt;

/// Represents a core module type.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ModuleType {
    /// The imports of the module type.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub imports: IndexMap<(String, String), CoreExtern>,
    /// The exports of the module type.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "IndexMap::is_empty"))]
    pub exports: IndexMap<String, CoreExtern>,
}

/// Represents a core extern imported or exported from a core module.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum CoreExtern {
    /// The item is a function.
    Func(CoreFuncType),
    /// The item is a table.
    #[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
    Table {
        /// The table's element type.
        element_type: CoreRefType,
        /// Initial size of this table, in elements.
        initial: u64,
        /// Optional maximum size of the table, in elements.
        maximum: Option<u64>,
        /// Whether or not this is a 64-bit table, using i64 as an index. If this
        /// is false it's a 32-bit memory using i32 as an index.
        ///
        /// This is part of the memory64 proposal in WebAssembly.
        table64: bool,
        /// Whether or not this is a "shared" memory, indicating that it should be
        /// send-able across threads and the `maximum` field is always present for
        /// valid types.
        ///
        /// This is part of the threads proposal in WebAssembly.
        shared: bool,
    },
    /// The item is a memory.
    #[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
    Memory {
        /// Whether or not this is a 64-bit memory, using i64 as an index. If this
        /// is false it's a 32-bit memory using i32 as an index.
        ///
        /// This is part of the memory64 proposal in WebAssembly.
        memory64: bool,

        /// Whether or not this is a "shared" memory, indicating that it should be
        /// send-able across threads and the `maximum` field is always present for
        /// valid types.
        ///
        /// This is part of the threads proposal in WebAssembly.
        shared: bool,

        /// Initial size of this memory, in wasm pages.
        ///
        /// For 32-bit memories (when `memory64` is `false`) this is guaranteed to
        /// be at most `u32::MAX` for valid types.
        initial: u64,

        /// Optional maximum size of this memory, in wasm pages.
        ///
        /// For 32-bit memories (when `memory64` is `false`) this is guaranteed to
        /// be at most `u32::MAX` for valid types. This field is always present for
        /// valid wasm memories when `shared` is `true`.
        maximum: Option<u64>,

        /// The log base 2 of the memory's custom page size.
        ///
        /// Memory pages are, by default, 64KiB large (i.e. 2<sup>16</sup> or
        /// `65536`).
        ///
        /// [The custom-page-sizes proposal] allows changing it to other values.
        ///
        /// [The custom-page-sizes proposal]: https://github.com/WebAssembly/custom-page-sizes
        page_size_log2: Option<u32>,
    },
    /// The item is a global.
    #[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
    Global {
        /// The global's type.
        val_type: CoreType,
        /// Whether or not the global is mutable.
        mutable: bool,
        /// Whether or not this is a "shared" memory, indicating that it should be
        /// send-able across threads and the `maximum` field is always present for
        /// valid types.
        ///
        /// This is part of the threads proposal in WebAssembly.
        shared: bool,
    },
    /// The item is a tag.
    Tag(CoreFuncType),
}

impl fmt::Display for CoreExtern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Func(_) => write!(f, "function"),
            Self::Table { .. } => write!(f, "table"),
            Self::Memory { .. } => write!(f, "memory"),
            Self::Global { .. } => write!(f, "global"),
            Self::Tag(_) => write!(f, "tag"),
        }
    }
}

impl TryFrom<wasmparser::TableType> for CoreExtern {
    type Error = anyhow::Error;

    fn try_from(ty: wasmparser::TableType) -> anyhow::Result<Self> {
        Ok(Self::Table {
            element_type: ty.element_type.try_into()?,
            initial: ty.initial,
            maximum: ty.maximum,
            table64: ty.table64,
            shared: ty.shared,
        })
    }
}

impl From<wasmparser::MemoryType> for CoreExtern {
    fn from(ty: wasmparser::MemoryType) -> Self {
        Self::Memory {
            memory64: ty.memory64,
            shared: ty.shared,
            initial: ty.initial,
            maximum: ty.maximum,
            page_size_log2: ty.page_size_log2,
        }
    }
}

impl TryFrom<wasmparser::GlobalType> for CoreExtern {
    type Error = anyhow::Error;

    fn try_from(ty: wasmparser::GlobalType) -> anyhow::Result<Self> {
        Ok(Self::Global {
            val_type: ty.content_type.try_into()?,
            mutable: ty.mutable,
            shared: ty.shared,
        })
    }
}

/// Represents the value types in a WebAssembly module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum CoreType {
    /// The value type is i32.
    I32,
    /// The value type is i64.
    I64,
    /// The value type is f32.
    F32,
    /// The value type is f64.
    F64,
    /// The value type is v128.
    V128,
    /// The value type is a reference.
    Ref(CoreRefType),
}

impl fmt::Display for CoreType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I32 => write!(f, "i32"),
            Self::I64 => write!(f, "i64"),
            Self::F32 => write!(f, "f32"),
            Self::F64 => write!(f, "f64"),
            Self::V128 => write!(f, "v128"),
            Self::Ref(r) => r.fmt(f),
        }
    }
}

impl TryFrom<wasmparser::ValType> for CoreType {
    type Error = anyhow::Error;

    fn try_from(ty: wasmparser::ValType) -> anyhow::Result<Self> {
        Ok(match ty {
            wasmparser::ValType::I32 => Self::I32,
            wasmparser::ValType::I64 => Self::I64,
            wasmparser::ValType::F32 => Self::F32,
            wasmparser::ValType::F64 => Self::F64,
            wasmparser::ValType::V128 => Self::V128,
            wasmparser::ValType::Ref(ty) => Self::Ref(ty.try_into()?),
        })
    }
}

impl From<CoreType> for wasm_encoder::ValType {
    fn from(value: CoreType) -> Self {
        match value {
            CoreType::I32 => Self::I32,
            CoreType::I64 => Self::I64,
            CoreType::F32 => Self::F32,
            CoreType::F64 => Self::F64,
            CoreType::V128 => Self::V128,
            CoreType::Ref(r) => Self::Ref(r.into()),
        }
    }
}

/// Represents the type of a reference in a WebAssembly module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CoreRefType {
    /// Whether or not the ref type is nullable.
    pub nullable: bool,
    /// The heap type of the ref type.
    pub heap_type: HeapType,
}

impl fmt::Display for CoreRefType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match (self.nullable, self.heap_type) {
            (true, HeapType::Func) => "funcref",
            (true, HeapType::Extern) => "externref",
            (true, HeapType::Concrete(i)) => return write!(f, "(ref null {i})"),
            (true, HeapType::Any) => "anyref",
            (true, HeapType::None) => "nullref",
            (true, HeapType::NoExtern) => "nullexternref",
            (true, HeapType::NoFunc) => "nullfuncref",
            (true, HeapType::Eq) => "eqref",
            (true, HeapType::Struct) => "structref",
            (true, HeapType::Array) => "arrayref",
            (true, HeapType::I31) => "i31ref",
            (true, HeapType::Exn) => "exnref",
            (true, HeapType::NoExn) => "nullexnref",
            (true, HeapType::Cont) => "contref",
            (true, HeapType::NoCont) => "nullcontref",
            (false, HeapType::Func) => "(ref func)",
            (false, HeapType::Extern) => "(ref extern)",
            (false, HeapType::Concrete(i)) => return write!(f, "(ref {i})"),
            (false, HeapType::Any) => "(ref any)",
            (false, HeapType::None) => "(ref none)",
            (false, HeapType::NoExtern) => "(ref noextern)",
            (false, HeapType::NoFunc) => "(ref nofunc)",
            (false, HeapType::Eq) => "(ref eq)",
            (false, HeapType::Struct) => "(ref struct)",
            (false, HeapType::Array) => "(ref array)",
            (false, HeapType::I31) => "(ref i31)",
            (false, HeapType::Exn) => "(ref exn)",
            (false, HeapType::NoExn) => "(ref noexn)",
            (false, HeapType::Cont) => "(ref cont)",
            (false, HeapType::NoCont) => "(ref nocont)",
        };

        f.write_str(s)
    }
}

impl TryFrom<wasmparser::RefType> for CoreRefType {
    type Error = anyhow::Error;

    fn try_from(ty: wasmparser::RefType) -> anyhow::Result<Self> {
        Ok(Self {
            nullable: ty.is_nullable(),
            heap_type: ty.heap_type().try_into()?,
        })
    }
}

impl From<CoreRefType> for wasm_encoder::RefType {
    fn from(value: CoreRefType) -> Self {
        wasm_encoder::RefType {
            nullable: value.nullable,
            heap_type: value.heap_type.into(),
        }
    }
}

/// A heap type of a reference type.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum HeapType {
    /// User defined type at the given index.
    Concrete(u32),
    /// Untyped (any) function.
    Func,
    /// External heap type.
    Extern,
    /// The `any` heap type.
    Any,
    /// The `none` heap type.
    None,
    /// The `noextern` heap type.
    NoExtern,
    /// The `nofunc` heap type.
    NoFunc,
    /// The `eq` heap type.
    Eq,
    /// The `struct` heap type. The common supertype of all struct types.
    Struct,
    /// The `array` heap type. The common supertype of all array types.
    Array,
    /// The i31 heap type.
    I31,
    /// The abstraction `exception` heap type.
    ///
    /// Introduced in the exception-handling proposal.
    Exn,
    /// The common subtype (a.k.a. bottom) of all exception types.
    ///
    /// Introduced in the exception-handling proposal.
    NoExn,
    /// The abstract `continuation` heap type.
    ///
    /// Introduced in the stack-switching proposal.
    Cont,
    /// The common subtype (a.k.a. bottom) of all continuation types.
    ///
    /// Introduced in the stack-switching proposal.
    NoCont,
}

impl TryFrom<wasmparser::HeapType> for HeapType {
    type Error = anyhow::Error;

    fn try_from(ty: wasmparser::HeapType) -> anyhow::Result<Self> {
        Ok(match ty {
            wasmparser::HeapType::Abstract { shared: false, ty } => match ty {
                wasmparser::AbstractHeapType::Any => Self::Any,
                wasmparser::AbstractHeapType::Func => Self::Func,
                wasmparser::AbstractHeapType::Extern => Self::Extern,
                wasmparser::AbstractHeapType::Eq => Self::Eq,
                wasmparser::AbstractHeapType::I31 => Self::I31,
                wasmparser::AbstractHeapType::None => Self::None,
                wasmparser::AbstractHeapType::NoExtern => Self::NoExtern,
                wasmparser::AbstractHeapType::NoFunc => Self::NoFunc,
                wasmparser::AbstractHeapType::Struct => Self::Struct,
                wasmparser::AbstractHeapType::Array => Self::Array,
                wasmparser::AbstractHeapType::Exn => Self::Exn,
                wasmparser::AbstractHeapType::NoExn => Self::NoExn,
                wasmparser::AbstractHeapType::Cont => Self::Cont,
                wasmparser::AbstractHeapType::NoCont => Self::NoCont,
            },
            wasmparser::HeapType::Abstract { shared: true, ty } => {
                anyhow::bail!("shared heap types are not supported: {ty:?}")
            }
            wasmparser::HeapType::Concrete(index) => {
                Self::Concrete(index.as_module_index().unwrap())
            }
            wasmparser::HeapType::Exact(_) => {
                anyhow::bail!("exact heap types are not yet supported")
            }
        })
    }
}

impl From<HeapType> for wasm_encoder::HeapType {
    /// Converts a `HeapType` into a `wasm_encoder::HeapType`.
    /// Shared types are not supported, so defaults to `false`
    /// for Abstract types.
    fn from(value: HeapType) -> Self {
        match value {
            HeapType::Concrete(index) => Self::Concrete(index),
            HeapType::Func => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Func,
            },
            HeapType::Extern => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Extern,
            },
            HeapType::Any => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Any,
            },
            HeapType::None => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::None,
            },
            HeapType::NoExtern => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::NoExtern,
            },
            HeapType::NoFunc => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::NoFunc,
            },
            HeapType::Eq => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Eq,
            },
            HeapType::Struct => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Struct,
            },
            HeapType::Array => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Array,
            },
            HeapType::I31 => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::I31,
            },
            HeapType::Exn => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Exn,
            },
            HeapType::NoExn => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::NoExn,
            },
            HeapType::Cont => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Cont,
            },
            HeapType::NoCont => Self::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::NoCont,
            },
        }
    }
}

/// Represents a core function type in a WebAssembly module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CoreFuncType {
    /// The parameters of the function.
    pub params: Vec<CoreType>,
    /// The results of the function.
    pub results: Vec<CoreType>,
}

impl fmt::Display for CoreFuncType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;

        for (i, ty) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }

            write!(f, "{}", ty)?;
        }

        write!(f, "] -> [")?;

        for (i, ty) in self.results.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }

            write!(f, "{}", ty)?;
        }

        write!(f, "]")
    }
}
