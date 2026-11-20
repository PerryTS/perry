//! THE single definition of the array-type predicates (`perry-abi` owns
//! `GC_TYPE_ARRAY`); `perry-codegen`, `perry-runtime` and `perry-stdlib` use
//! these. Each macro expands textually to the comparison it replaces, so a
//! migrated site compiles to the code it had before.
//!
//! This file is the only place allowed to name `GC_TYPE_ARRAY` in a
//! comparison (`scripts/check_object_header_access.sh`, column (e)). Births
//! (allocation arguments, header writes) still name the constant.

/// "Is an ordinary array head" (`ArrayHeader`) on an `obj_type` byte.
#[macro_export]
macro_rules! gc_type_is_array {
    ($t:expr) => {
        ($t) == $crate::GC_TYPE_ARRAY
    };
}
/// The negation of [`gc_type_is_array!`] for a site that compared with `!=`.
#[macro_export]
macro_rules! gc_type_is_not_array {
    ($t:expr) => {
        ($t) != $crate::GC_TYPE_ARRAY
    };
}
/// The array type as a `match` arm pattern, or as the type a kind-test helper
/// (`*_of_type(.., ty)`, `Some(ty)`) compares with.
#[macro_export]
macro_rules! gc_array_type_pat {
    () => {
        $crate::GC_TYPE_ARRAY
    };
}
