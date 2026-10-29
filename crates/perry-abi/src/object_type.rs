//! The object-type predicates for crates below `perry-runtime` (this crate,
//! `perry-codegen`). Each macro expands textually to the comparison it
//! replaces, so a migrated site compiles to the code it had before. The
//! runtime's own copies live in `perry-runtime/src/gc/types.rs`.
//!
//! This file and `gc/types.rs` are the only places allowed to name
//! `GC_TYPE_OBJECT` in a comparison (`scripts/check_object_header_access.sh`).

/// "Is an ordinary object", either header form.
#[macro_export]
macro_rules! gc_type_is_object {
    ($t:expr) => {
        ($t) == $crate::GC_TYPE_OBJECT
    };
}
/// "Might carry the meta word".
#[macro_export]
macro_rules! object_has_meta_word {
    ($t:expr) => {
        ($t) == $crate::GC_TYPE_OBJECT
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
