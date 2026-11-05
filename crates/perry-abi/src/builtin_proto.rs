//! Intrinsic family identities shared by runtime births and image emission.
/// A family of intrinsic prototypes whose methods compiled code calls by
/// name. The discriminant is the family's agent slot offset
/// (`AGENT_PTR_INTRINSIC_PROTO + k`). Runtime and codegen use this shared enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BuiltinProto {
    Object = 0,
    Function = 1,
    Array = 2,
    String = 3,
    Number = 4,
    Boolean = 5,
    Symbol = 6,
    BigInt = 7,
    Map = 8,
    Set = 9,
    Date = 10,
    RegExp = 11,
    Promise = 12,
    Error = 13,
    WeakMap = 14,
    WeakSet = 15,
}

impl BuiltinProto {
    /// Every family, with the global constructor whose `prototype` it is.
    pub const ALL: [(BuiltinProto, &'static str); crate::BUILTIN_PROTO_FAMILIES] = [
        (BuiltinProto::Object, "Object"),
        (BuiltinProto::Function, "Function"),
        (BuiltinProto::Array, "Array"),
        (BuiltinProto::String, "String"),
        (BuiltinProto::Number, "Number"),
        (BuiltinProto::Boolean, "Boolean"),
        (BuiltinProto::Symbol, "Symbol"),
        (BuiltinProto::BigInt, "BigInt"),
        (BuiltinProto::Map, "Map"),
        (BuiltinProto::Set, "Set"),
        (BuiltinProto::Date, "Date"),
        (BuiltinProto::RegExp, "RegExp"),
        (BuiltinProto::Promise, "Promise"),
        (BuiltinProto::Error, "Error"),
        (BuiltinProto::WeakMap, "WeakMap"),
        (BuiltinProto::WeakSet, "WeakSet"),
    ];

    pub fn from_index(i: usize) -> Option<BuiltinProto> {
        Self::ALL.get(i).map(|(f, _)| *f)
    }
}

/// Intrinsic prototype selected by a function body's ABI flags. Runtime
/// closure births and compiler image state use the same priority rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FunctionProtoKind {
    Function = 0,
    AsyncFunction = 1,
    Generator = 2,
    AsyncGenerator = 3,
}

impl FunctionProtoKind {
    pub const fn of_flags(flags: u32) -> Self {
        if flags & crate::FN_ASYNC_GENERATOR != 0 {
            Self::AsyncGenerator
        } else if flags & crate::FN_GENERATOR != 0 {
            Self::Generator
        } else if flags & crate::FN_ASYNC != 0 {
            Self::AsyncFunction
        } else {
            Self::Function
        }
    }

    pub const fn prototype_name(self) -> &'static str {
        match self {
            Self::Function => "Function.prototype",
            Self::AsyncFunction => "%AsyncFunctionPrototype%",
            Self::Generator => "%GeneratorFunctionPrototype%",
            Self::AsyncGenerator => "%AsyncGeneratorFunctionPrototype%",
        }
    }
}
