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
    /// `Buffer.prototype`: node's host prototype, not an ECMA-262 intrinsic
    /// (see [`BuiltinProto::realm_intrinsic`]).
    Buffer = 16,
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
        (BuiltinProto::Buffer, "Buffer"),
    ];

    /// Is this family's prototype an ECMA-262 intrinsic: built with the realm,
    /// sealed by the realm seal and under DEC 250's lock? `Buffer.prototype`
    /// is node's host prototype: it is built on first observation and sealed
    /// then (until it exists its slot names the pristine stand-in), and the
    /// program may patch it as in node, so a write only sets its shadow bit.
    pub const fn realm_intrinsic(self) -> bool {
        (self as usize) < Self::REALM_INTRINSICS
    }

    /// The ECMA-262 intrinsic families are the first `REALM_INTRINSICS` of
    /// [`BuiltinProto::ALL`]; the host families (`Buffer`) follow them.
    pub const REALM_INTRINSICS: usize = BuiltinProto::Buffer as usize;

    /// The families the realm seal seals: the prefix of [`BuiltinProto::ALL`]
    /// that [`BuiltinProto::realm_intrinsic`] admits.
    pub const REALM: [(BuiltinProto, &'static str); Self::REALM_INTRINSICS] = {
        let mut realm = [(BuiltinProto::Object, ""); Self::REALM_INTRINSICS];
        let mut i = 0;
        while i < Self::REALM_INTRINSICS {
            realm[i] = Self::ALL[i];
            i += 1;
        }
        realm
    };

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
