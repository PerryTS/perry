//! Transactional canonical resource conversion. Newly allocated host handles
//! are rolled back if any later aggregate member fails validation.
use crate::host_resources::{Entry, ResourceEvent, ResourceRef};
use crate::{wire, StoreData};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wasmtime::component::{ResourceAny, ResourceDynamic, ResourceType, Type, Val};
use wasmtime::{AsContextMut, StoreContextMut};

pub(crate) struct Lowering<'a> {
    store: StoreContextMut<'a, StoreData>,
    pending: BTreeMap<String, Entry>,
    requests: BTreeMap<String, bool>,
    guest_transfers: BTreeSet<String>,
    host_transfers: BTreeSet<String>,
    async_transfers: BTreeSet<String>,
}

impl<'a> Lowering<'a> {
    pub fn new(store: StoreContextMut<'a, StoreData>) -> Self {
        Self {
            store,
            pending: BTreeMap::new(),
            requests: BTreeMap::new(),
            guest_transfers: BTreeSet::new(),
            host_transfers: BTreeSet::new(),
            async_transfers: BTreeSet::new(),
        }
    }

    pub fn value(&mut self, value: &Value, ty: &Type) -> Result<Val> {
        wire::lower_typed(value, ty, &mut |value, ty| match ty {
            Type::Own(r) | Type::Borrow(r) => self
                .resource(value, *r, matches!(ty, Type::Own(_)))
                .map(Val::Resource),
            Type::Future(_) | Type::Stream(_) => {
                self.store
                    .data()
                    .async_values
                    .get(value, ty, &mut self.async_transfers)
            }
            _ => anyhow::bail!("unsupported managed value"),
        })
    }

    pub fn borrowed_keys(&self) -> Vec<String> {
        self.requests
            .iter()
            .filter(|(_, owned)| !**owned)
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn reserve(&mut self, token: String, owned: bool) -> Result<()> {
        ensure!(
            !owned || !self.store.data().borrowed.contains(&token),
            "resource is borrowed by an active component call"
        );
        if let Some(previous) = self.requests.get(&token) {
            ensure!(
                !owned && !previous,
                "resource is transferred twice or borrowed during transfer"
            );
        }
        self.requests.insert(token, owned);
        Ok(())
    }

    fn resource(&mut self, value: &Value, ty: ResourceType, owned: bool) -> Result<ResourceAny> {
        if !self.store.data().host_resources.is_host(ty) {
            let token = value["$resource"]
                .as_str()
                .context("expected guest resource")?;
            self.reserve(format!("guest:{token}"), owned)?;
            return self
                .store
                .data()
                .resources
                .get(token, ty, owned, &mut self.guest_transfers);
        }
        let token = value["$hostResource"]
            .as_str()
            .context("expected host resource")?;
        ensure!(
            !token.is_empty() && token.len() <= 128,
            "invalid host resource token"
        );
        self.reserve(format!("host:{token}"), owned)?;
        let reference = ResourceRef {
            token: token.to_string(),
            type_key: value["type"]
                .as_str()
                .context("missing host resource type")?
                .to_string(),
            generation: u32::try_from(
                value["generation"]
                    .as_u64()
                    .context("invalid resource generation")?,
            )?,
        };
        let id = *self
            .store
            .data()
            .host_resources
            .types
            .get(&reference.type_key)
            .context("resource type is not provided by this host")?;
        ensure!(
            ty == ResourceType::host_dynamic(id),
            "host resource type mismatch"
        );
        let handle = if let Some(entry) = self.pending.get(token) {
            ensure!(
                entry.reference == reference,
                "host resource identity mismatch"
            );
            entry.handle.unwrap()
        } else if let Some(entry) = self.store.data().host_resources.entries.get(token) {
            ensure!(value["new"] != true, "host resource was already registered");
            ensure!(
                entry.reference == reference,
                "host resource identity is stale or has the wrong type"
            );
            entry
                .handle
                .context("host resource was transferred to the guest")?
        } else {
            ensure!(
                value["new"] == true && reference.generation == 1,
                "host resource is unknown, disposed, or belongs to another component"
            );
            let rep = self.store.data_mut().host_resources.next_rep()?;
            let handle = ResourceDynamic::new_own(rep, id)
                .try_into_resource_any(self.store.as_context_mut())?;
            self.pending.insert(
                token.to_string(),
                Entry {
                    reference,
                    rep,
                    ty: id,
                    handle: Some(handle),
                },
            );
            handle
        };
        if owned {
            self.host_transfers.insert(token.to_string());
        }
        Ok(handle)
    }

    /// No handle is consumed until every argument/result has been converted.
    /// The hook validates private embedding identities before updating them.
    pub fn commit(mut self) -> Result<()> {
        let state = &self.store.data().host_resources;
        let created = self
            .pending
            .values()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        let transferred = self
            .host_transfers
            .iter()
            .map(|token| {
                self.pending
                    .get(token)
                    .or_else(|| state.entries.get(token))
                    .unwrap()
                    .reference
                    .clone()
            })
            .collect::<Vec<_>>();
        if !created.is_empty() || !transferred.is_empty() {
            (state.hook)(ResourceEvent::Lower {
                created,
                transferred,
            })?;
        }
        let state = &mut self.store.data_mut().host_resources;
        for (token, entry) in std::mem::take(&mut self.pending) {
            state.reps.insert(entry.rep, token.clone());
            state.entries.insert(token, entry);
        }
        for token in &self.host_transfers {
            state.entries.get_mut(token).unwrap().handle = None;
        }
        self.store
            .data_mut()
            .resources
            .transferred(&self.guest_transfers);
        self.store
            .data_mut()
            .async_values
            .transferred(&self.async_transfers);
        Ok(())
    }
}

impl Drop for Lowering<'_> {
    fn drop(&mut self) {
        for entry in std::mem::take(&mut self.pending).into_values() {
            if let Some(handle) = entry.handle {
                // These handles never entered canonical lowering. Recover the
                // representation without invoking the application's destructor;
                // its original owned wrapper remains valid after rollback.
                let _ = handle.try_into_resource_dynamic(self.store.as_context_mut());
            }
        }
    }
}

pub(crate) fn lift(
    mut store: StoreContextMut<'_, StoreData>,
    value: &Val,
    ty: &Type,
) -> Result<Value> {
    wire::lift_typed(value, ty, &mut |value, ty| match value {
        Val::Resource(resource) => lift_resource(store.as_context_mut(), *resource),
        Val::Future(_) | Val::Stream(_) => store.data_mut().async_values.insert(value, ty),
        _ => anyhow::bail!("unsupported managed value"),
    })
}

fn lift_resource(
    mut store: StoreContextMut<'_, StoreData>,
    resource: ResourceAny,
) -> Result<Value> {
    if !store.data().host_resources.is_host(resource.ty()) {
        return Ok(
            json!({"$resource":store.data_mut().resources.insert(resource),"owned":resource.owned()}),
        );
    }
    let dynamic = resource.try_into_resource_dynamic(store.as_context_mut())?;
    let state = &store.data().host_resources;
    let token = state
        .reps
        .get(&dynamic.rep())
        .context("guest returned an unknown host resource")?
        .clone();
    let entry = &state.entries[&token];
    ensure!(
        entry.ty == dynamic.ty(),
        "guest returned the wrong host resource type"
    );
    let mut reference = entry.reference.clone();
    let owned = dynamic.owned();
    if owned {
        ensure!(
            entry.handle.is_none(),
            "guest returned an already-owned host resource"
        );
        reference.generation = reference
            .generation
            .checked_add(1)
            .context("host resource generation exhausted")?;
        let handle = dynamic.try_into_resource_any(store.as_context_mut())?;
        let hook = store.data().host_resources.hook.clone();
        if let Err(error) = hook(ResourceEvent::Lift {
            resource: reference.clone(),
        }) {
            let _ = handle.try_into_resource_dynamic(store.as_context_mut());
            return Err(error);
        }
        let entry = store
            .data_mut()
            .host_resources
            .entries
            .get_mut(&token)
            .unwrap();
        entry.reference = reference.clone();
        entry.handle = Some(handle);
    }
    Ok(
        json!({"$hostResource":reference.token,"type":reference.type_key,
        "generation":reference.generation,"owned":owned,"new":false}),
    )
}
