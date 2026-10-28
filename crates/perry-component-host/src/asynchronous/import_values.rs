//! Prepare only borrowed capabilities while the canonical host-call scope is
//! active. No embedding hook runs here; JS/owned-resource lifting follows yield.
use crate::{transport, StoreData};
use anyhow::{ensure, Result};
use serde_json::Value;
use std::collections::VecDeque;
use wasmtime::{component::Val, AsContextMut, StoreContextMut};
pub(crate) fn prepare(
    mut store: StoreContextMut<'_, StoreData>,
    values: &[Val],
) -> Result<VecDeque<Value>> {
    fn visit(
        store: &mut StoreContextMut<'_, StoreData>,
        value: &Val,
        out: &mut VecDeque<Value>,
        depth: usize,
    ) -> Result<()> {
        ensure!(depth < 128, "component value nesting exceeds 128");
        match value {
            Val::Resource(resource) if !resource.owned() => {
                out.push_back(transport::lift_resource(store.as_context_mut(), *resource)?)
            }
            Val::List(values) => {
                // Canonical lists are homogeneous; primitive elements cannot
                // hide a borrow. Avoid walking large byte/scalar lists twice.
                if matches!(
                    values.first(),
                    Some(
                        Val::Bool(_)
                            | Val::U8(_)
                            | Val::U16(_)
                            | Val::U32(_)
                            | Val::U64(_)
                            | Val::S8(_)
                            | Val::S16(_)
                            | Val::S32(_)
                            | Val::S64(_)
                            | Val::Float32(_)
                            | Val::Float64(_)
                            | Val::Char(_)
                            | Val::String(_)
                            | Val::Enum(_)
                            | Val::Flags(_)
                    )
                ) {
                    return Ok(());
                }
                for value in values {
                    visit(store, value, out, depth + 1)?;
                }
            }
            Val::Tuple(values) => {
                for value in values {
                    visit(store, value, out, depth + 1)?;
                }
            }
            Val::Record(values) => {
                for (_, value) in values {
                    visit(store, value, out, depth + 1)?;
                }
            }
            Val::Option(Some(value)) | Val::Variant(_, Some(value)) => {
                visit(store, value, out, depth + 1)?
            }
            Val::Result(Ok(Some(value)) | Err(Some(value))) => visit(store, value, out, depth + 1)?,
            _ => {}
        }
        Ok(())
    }
    let mut prepared = VecDeque::new();
    for value in values {
        visit(&mut store, value, &mut prepared, 0)?;
    }
    Ok(prepared)
}
