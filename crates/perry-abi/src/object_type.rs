//! THE single definition of the object-type predicates (`perry-abi` owns
//! `GC_TYPE_OBJECT`); `perry-codegen` and `perry-runtime` both use these.
//! Each macro expands textually to the comparison it replaces, so a migrated
//! site compiles to the code it had before.
//!
//! This file is the only place allowed to name `GC_TYPE_OBJECT` in a
//! comparison (`scripts/check_object_header_access.sh`).

/// "Is an ordinary object", either header form. S0f meaning (A): `t >= 0xFE`.
#[macro_export]
macro_rules! gc_type_is_object {
    ($t:expr) => {
        ($t) == $crate::GC_TYPE_OBJECT
    };
}
/// "Might carry the meta word". S0f meaning (B): `t == 0xFF`.
#[macro_export]
macro_rules! object_has_meta_word {
    ($t:expr) => {
        ($t) == $crate::GC_TYPE_OBJECT
    };
}
/// The negation of [`object_has_meta_word!`]. S0f meaning (B): `t != 0xFF`.
#[macro_export]
macro_rules! object_lacks_meta_word {
    ($t:expr) => {
        ($t) != $crate::GC_TYPE_OBJECT
    };
}
/// The negation of [`gc_type_is_object!`] for a site that compared with `!=`.
#[macro_export]
macro_rules! gc_type_is_not_object {
    ($t:expr) => {
        ($t) != $crate::GC_TYPE_OBJECT
    };
}
/// The constant pattern of a `match` arm on an object type byte.
#[macro_export]
macro_rules! gc_object_type_pat {
    () => {
        $crate::GC_TYPE_OBJECT
    };
}
