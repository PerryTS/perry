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

/// Sealed prototype metadata families beyond the agent's builtin slots.
/// Births and image bytes use these same existing encodings.
pub const RUNTIME_PROTOTYPE_FAMILY: usize = crate::INTRINSIC_FAMILIES + 1;
pub const ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 1;
pub const ARRAY_ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 2;
pub const GENERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 3;
pub const ASYNC_GENERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 4;
pub const MAP_ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 5;
pub const SET_ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 6;
pub const STRING_ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 7;
pub const REGEXP_STRING_ITERATOR_PROTOTYPE_FAMILY: usize = RUNTIME_PROTOTYPE_FAMILY + 8;

/// The five existing sealed iterator families after the common root.
/// This is the runtime birth's existing catalog, shared with byte emission.
pub const ITERATOR_PROTOTYPE_FAMILIES: [usize; 5] = [
    ARRAY_ITERATOR_PROTOTYPE_FAMILY,
    MAP_ITERATOR_PROTOTYPE_FAMILY,
    SET_ITERATOR_PROTOTYPE_FAMILY,
    STRING_ITERATOR_PROTOTYPE_FAMILY,
    REGEXP_STRING_ITERATOR_PROTOTYPE_FAMILY,
];

/// A source-declared iterator root's existing seal. Helper and wrapper
/// prototypes are not sealed; their serials confer identity only.
pub const fn iterator_sealed_family(serial: u64) -> Option<usize> {
    use crate::intrinsic_serials::*;
    if serial == INTRINSIC_SERIAL_ITERATOR_PROTOTYPE {
        Some(ITERATOR_PROTOTYPE_FAMILY)
    } else if serial >= INTRINSIC_SERIAL_ARRAY_ITERATOR_PROTOTYPE
        && serial <= INTRINSIC_SERIAL_REGEXP_STRING_ITERATOR_PROTOTYPE
    {
        Some(
            ITERATOR_PROTOTYPE_FAMILIES
                [(serial - INTRINSIC_SERIAL_ARRAY_ITERATOR_PROTOTYPE) as usize],
        )
    } else {
        None
    }
}

#[cfg(test)]
mod sealed_family_tests {
    use super::*;
    #[test]
    fn realm_metadata_seals_share_the_birth_catalog_and_exclude_unsealed_roots() {
        use crate::intrinsic_serials::*;
        assert_eq!(
            iterator_sealed_family(INTRINSIC_SERIAL_ITERATOR_PROTOTYPE),
            Some(ITERATOR_PROTOTYPE_FAMILY)
        );
        for (index, family) in ITERATOR_PROTOTYPE_FAMILIES.into_iter().enumerate() {
            assert_eq!(
                iterator_sealed_family(INTRINSIC_SERIAL_ARRAY_ITERATOR_PROTOTYPE + index as u64),
                Some(family)
            );
        }
        assert_eq!(
            iterator_sealed_family(INTRINSIC_SERIAL_ITERATOR_HELPER_PROTOTYPE),
            None
        );
        assert_eq!(
            iterator_sealed_family(INTRINSIC_SERIAL_WRAP_FOR_VALID_ITERATOR_PROTOTYPE),
            None
        );
        assert_eq!(iterator_sealed_family(0), None);
        assert_ne!(GENERATOR_PROTOTYPE_FAMILY, ASYNC_GENERATOR_PROTOTYPE_FAMILY);
    }
}
