use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{ResourceAny, ResourceType};

// A nonce prevents a token from a different component selecting a same-numbered
// resource. The counter contains no capability or heap pointer; ownership and
// every live resource stay inside their component's StoreData.
static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Resources {
    nonce: u64,
    next: u64,
    entries: BTreeMap<String, ResourceAny>,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            nonce: NEXT_NONCE.fetch_add(1, Ordering::Relaxed),
            next: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl Resources {
    pub fn insert(&mut self, resource: ResourceAny) -> String {
        let id = format!("{}:{}", self.nonce, self.next);
        self.next = self
            .next
            .checked_add(1)
            .expect("component resource token space exhausted");
        self.entries.insert(id.clone(), resource);
        id
    }

    pub fn get(
        &self,
        id: &str,
        ty: ResourceType,
        owned: bool,
        transfers: &mut BTreeSet<String>,
    ) -> Result<ResourceAny> {
        let value = *self
            .entries
            .get(id)
            .context("resource is disposed, transferred, or belongs to another component")?;
        if value.ty() != ty {
            bail!("resource type mismatch");
        }
        if owned && (!value.owned() || !transfers.insert(id.to_string())) {
            bail!("an owned resource can be transferred only once");
        }
        Ok(value)
    }

    pub fn remove(&mut self, id: &str) -> Result<ResourceAny> {
        self.entries
            .remove(id)
            .context("resource is disposed, transferred, or belongs to another component")
    }

    pub fn owned(&self, id: &str) -> Result<ResourceAny> {
        let resource = *self
            .entries
            .get(id)
            .context("resource is disposed, transferred, or belongs to another component")?;
        anyhow::ensure!(resource.owned(), "cannot dispose a borrowed resource");
        Ok(resource)
    }

    pub fn owns(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(ResourceAny::owned)
    }

    pub fn transferred(&mut self, ids: &BTreeSet<String>) {
        for id in ids {
            self.entries.remove(id);
        }
    }

    pub fn drain(&mut self) -> Vec<ResourceAny> {
        std::mem::take(&mut self.entries).into_values().collect()
    }

    pub fn take_borrows(&mut self) -> Vec<ResourceAny> {
        let ids: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, r)| !r.owned())
            .map(|(id, _)| id.clone())
            .collect();
        ids.into_iter()
            .filter_map(|id| self.entries.remove(&id))
            .collect()
    }
}
