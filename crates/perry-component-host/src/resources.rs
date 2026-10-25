use crate::ResourceRef;
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
    entries: BTreeMap<String, Entry>,
}

struct Entry {
    value: ResourceAny,
    serial: u64,
    host: Option<ResourceRef>,
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
        self.entries.insert(
            id.clone(),
            Entry {
                value: resource,
                serial: self.next - 1,
                host: None,
            },
        );
        id
    }

    pub fn get(
        &self,
        id: &str,
        ty: ResourceType,
        owned: bool,
        transfers: &mut BTreeSet<String>,
    ) -> Result<ResourceAny> {
        let value = self
            .entries
            .get(id)
            .context("resource is disposed, transferred, or belongs to another component")?
            .value;
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
            .map(|entry| entry.value)
            .context("resource is disposed, transferred, or belongs to another component")
    }

    pub fn owned(&self, id: &str) -> Result<ResourceAny> {
        let resource = self
            .entries
            .get(id)
            .context("resource is disposed, transferred, or belongs to another component")?
            .value;
        anyhow::ensure!(resource.owned(), "cannot dispose a borrowed resource");
        Ok(resource)
    }

    pub fn owns(&self, id: &str) -> bool {
        self.entries
            .get(id)
            .is_some_and(|entry| entry.value.owned())
    }

    pub fn transferred(&mut self, ids: &BTreeSet<String>) {
        for id in ids {
            self.entries.remove(id);
        }
    }

    pub fn drain(&mut self) -> Vec<ResourceAny> {
        std::mem::take(&mut self.entries)
            .into_values()
            .map(|entry| entry.value)
            .collect()
    }

    pub fn checkpoint(&self) -> u64 {
        self.next
    }
    pub fn borrowed_since(&self, checkpoint: u64) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, e)| e.serial >= checkpoint && !e.value.owned())
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub fn take_borrows(&mut self, ids: &[String]) -> Vec<ResourceAny> {
        ids.iter()
            .filter_map(|id| self.entries.remove(id))
            .map(|entry| entry.value)
            .collect()
    }
    pub fn insert_host_borrow(&mut self, value: ResourceAny, reference: ResourceRef) -> String {
        let id = self.insert(value);
        self.entries.get_mut(&id).unwrap().host = Some(reference);
        id
    }
    pub fn host_borrow(
        &self,
        id: &str,
        reference: &ResourceRef,
        ty: ResourceType,
    ) -> Result<ResourceAny> {
        let entry = self
            .entries
            .get(id)
            .context("host resource borrow expired or belongs to another component")?;
        anyhow::ensure!(
            entry.host.as_ref() == Some(reference)
                && entry.value.ty() == ty
                && !entry.value.owned(),
            "host resource borrow identity mismatch"
        );
        Ok(entry.value)
    }
}
