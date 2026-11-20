//! Host resource identities live in one Store. Payloads stay in the embedding;
//! callbacks carry typed identities, never addresses into its managed heap.
use crate::{Options, StoreData};
use anyhow::{ensure, Context, Result};
use perry_wit::World;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use wasmtime::component::{ResourceAny, ResourceType};
use wasmtime::{AsContextMut, StoreContextMut};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceRef {
    pub token: String,
    #[serde(rename = "type")]
    pub type_key: String,
    pub generation: u32,
}

/// Lower validates every identity before changing any ownership. It must not
/// execute application code. Lift renews ownership after a guest returns it;
/// Drop releases the payload before invoking the application's destructor.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResourceEvent {
    /// After an import returns (including failure), retire owned arguments
    /// disposed by that callback without reentering the active Store.
    Drain,
    Lower {
        created: Vec<ResourceRef>,
        transferred: Vec<ResourceRef>,
    },
    Lift {
        resource: ResourceRef,
    },
    Drop {
        resource: ResourceRef,
    },
}
pub type ResourceHook = Arc<dyn Fn(ResourceEvent) -> Result<Vec<ResourceRef>> + Send + Sync>;

pub(crate) struct Entry {
    pub reference: ResourceRef,
    pub rep: u32,
    pub ty: u32,
    pub handle: Option<ResourceAny>,
}

pub(crate) struct HostResources {
    pub types: BTreeMap<String, u32>,
    pub entries: BTreeMap<String, Entry>,
    pub reps: BTreeMap<u32, String>,
    pub hook: ResourceHook,
    next_rep: u32,
}

impl HostResources {
    pub fn new(world: &World, options: &Options, hook: ResourceHook) -> Result<Self> {
        let mut types = BTreeMap::new();
        for interface in &world.imports {
            if options.wasi.is_some() && interface.name.starts_with("wasi:") {
                continue;
            }
            for key in interface.resources.values() {
                let next = u32::try_from(types.len()).context("too many host resource types")?;
                types.entry(key.clone()).or_insert(next);
            }
        }
        Ok(Self {
            types,
            entries: BTreeMap::new(),
            reps: BTreeMap::new(),
            hook,
            next_rep: 1,
        })
    }

    pub fn is_host(&self, ty: ResourceType) -> bool {
        self.types
            .values()
            .any(|id| ty == ResourceType::host_dynamic(*id))
    }

    pub fn next_rep(&mut self) -> Result<u32> {
        let rep = self.next_rep;
        self.next_rep = rep
            .checked_add(1)
            .context("host resource representation space exhausted")?;
        Ok(rep)
    }

    pub fn remove(&mut self, rep: u32, ty: u32) -> Result<ResourceRef> {
        let token = self
            .reps
            .get(&rep)
            .context("unknown host resource representation")?;
        ensure!(
            self.entries[token].ty == ty,
            "host resource destructor type mismatch"
        );
        let token = self.reps.remove(&rep).unwrap();
        Ok(self.entries.remove(&token).unwrap().reference)
    }
}

pub(crate) fn destroy(mut store: StoreContextMut<'_, StoreData>, rep: u32, ty: u32) -> Result<()> {
    let token = store
        .data()
        .host_resources
        .reps
        .get(&rep)
        .context("unknown host resource representation")?;
    ensure!(
        !store.data().borrowed.contains(&format!("host:{token}")),
        "host resource is borrowed by an active component call"
    );
    let state = &mut store.data_mut().host_resources;
    let reference = state.remove(rep, ty)?;
    let hook = state.hook.clone();
    // Remove first: a failing destructor must not be retried on teardown.
    hook(ResourceEvent::Drop {
        resource: reference,
    })
    .map(|_| ())
}

/// Handles recreated from ResourceDynamic have no canonical destructor.
/// Release that table entry first, then explicitly destroy the host payload.
/// A rejected release preserves ownership; a throwing payload destructor does
/// not revive it. Guest-side resource.drop uses the linker's destructor instead.
pub(crate) fn drop_owned(
    mut store: StoreContextMut<'_, StoreData>,
    token: &str,
    released: &mut bool,
) -> Result<()> {
    let entry = store
        .data()
        .host_resources
        .entries
        .get(token)
        .context("unknown host resource")?;
    let handle = entry
        .handle
        .context("host resource is owned by the guest")?;
    let (rep, ty) = (entry.rep, entry.ty);
    release(handle, store.as_context_mut())?;
    *released = true;
    destroy(store, rep, ty)
}

/// Remove a host-defined handle's table entry. Its type has no canonical
/// destructor, so no guest code runs; unlike resource_drop this is valid in an
/// async-configured Store such as the persistent driver's.
fn release(handle: ResourceAny, store: StoreContextMut<'_, StoreData>) -> Result<()> {
    handle
        .try_into_resource_dynamic(store)
        .map(drop)
        .map_err(anyhow::Error::from)
}

pub(crate) fn drain(mut store: StoreContextMut<'_, StoreData>) -> Result<()> {
    let hook = store.data().host_resources.hook.clone();
    let pending = hook(ResourceEvent::Drain)?;
    let mut first = None;
    for reference in pending {
        let result = (|| -> Result<()> {
            let entry = store
                .data()
                .host_resources
                .entries
                .get(&reference.token)
                .context("unknown disposed host resource")?;
            ensure!(entry.reference == reference, "stale disposed host resource");
            drop_owned(store.as_context_mut(), &reference.token, &mut false)
        })();
        if let Err(error) = result {
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}

/// Teardown also releases host payloads held by the guest, whose canonical
/// handles are not visible to the embedding. Wasmtime Store drop does not run
/// their destructors. A trapping destructor does not stop the remaining drops.
pub(crate) fn dispose(mut store: StoreContextMut<'_, StoreData>) -> Result<()> {
    let handles: Vec<_> = store
        .data_mut()
        .host_resources
        .entries
        .values_mut()
        .filter_map(|entry| entry.handle.take())
        .collect();
    let mut first = None;
    for handle in handles {
        if let Err(error) = release(handle, store.as_context_mut()) {
            first.get_or_insert(error);
        }
    }
    let state = &mut store.data_mut().host_resources;
    let remaining = std::mem::take(&mut state.entries);
    state.reps.clear();
    let hook = state.hook.clone();
    for entry in remaining.into_values() {
        if let Err(error) = hook(ResourceEvent::Drop {
            resource: entry.reference,
        }) {
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}
