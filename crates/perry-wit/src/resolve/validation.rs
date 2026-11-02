//! Canonical endpoint payloads cannot carry call-scoped resource borrows.
use crate::{Definition, Type};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, HashSet};

pub(super) fn endpoints(types: &BTreeMap<String, Definition>) -> Result<()> {
    for definition in types.values() {
        if let Type::Future(Some(payload)) | Type::Stream(Some(payload)) = &definition.ty {
            ensure!(
                !contains_borrow(payload, types, &mut HashSet::new())?,
                "type {}: async endpoint payloads cannot contain borrow<T>",
                definition.name
            );
        }
    }
    Ok(())
}

fn any<'a>(
    values: impl IntoIterator<Item = &'a Type>,
    types: &BTreeMap<String, Definition>,
    seen: &mut HashSet<String>,
) -> Result<bool> {
    for value in values {
        if contains_borrow(value, types, seen)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn contains_borrow(
    ty: &Type,
    types: &BTreeMap<String, Definition>,
    seen: &mut HashSet<String>,
) -> Result<bool> {
    match ty {
        Type::Borrow(_) => Ok(true),
        Type::Ref(key) => {
            if !seen.insert(key.clone()) {
                return Ok(false);
            }
            contains_borrow(
                &types.get(key).context("Missing endpoint payload type")?.ty,
                types,
                seen,
            )
        }
        Type::Record(fields) => any(fields.iter().map(|field| &field.ty), types, seen),
        Type::Tuple(values) => any(values, types, seen),
        Type::Variant(cases) => any(
            cases.iter().filter_map(|case| case.ty.as_ref()),
            types,
            seen,
        ),
        Type::Option(value) | Type::List(value) => contains_borrow(value, types, seen),
        Type::Result { ok, err } => any(
            ok.iter().chain(err).map(|value| value.as_ref()),
            types,
            seen,
        ),
        Type::Future(value) | Type::Stream(value) => any(value.as_deref(), types, seen),
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String
        | Type::Flags(_)
        | Type::Enum(_)
        | Type::Resource
        | Type::Own(_) => Ok(false),
    }
}
