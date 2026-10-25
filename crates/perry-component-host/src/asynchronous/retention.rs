//! Native retention bookkeeping for an embedding's per-invocation context.
//! The registry contains IDs and counts only, never callbacks or JS roots.
use super::origin::InvocationId;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use wasmtime::component::GuestTaskId;

#[derive(Default)]
struct State {
    roots: BTreeMap<GuestTaskId, InvocationId>,
    captures: BTreeMap<InvocationId, usize>,
}
#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<State>>);
pub(super) struct Lease {
    origin: InvocationId,
    registry: Registry,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut state = self.registry.0.lock().unwrap();
        let count = state
            .captures
            .get_mut(&self.origin)
            .expect("live origin lease");
        *count -= 1;
        if *count == 0 {
            state.captures.remove(&self.origin);
        }
    }
}
impl Registry {
    pub fn register(&self, task: GuestTaskId, origin: InvocationId) {
        // IDs may be reused after task exit. Replacing a binding does not
        // discard leases retained by host callback futures or their cleanup.
        self.0.lock().unwrap().roots.insert(task, origin);
    }
    pub fn clear_roots(&self) {
        self.0.lock().unwrap().roots.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().roots.is_empty()
    }
    pub(super) fn capture(
        &self,
        ancestry: &[GuestTaskId],
        current: Option<InvocationId>,
    ) -> Option<(InvocationId, Lease)> {
        let mut state = self.0.lock().unwrap();
        let origin = ancestry
            .iter()
            .rev()
            .find_map(|task| state.roots.get(task).copied())
            .or_else(|| {
                current.filter(|id| {
                    state.captures.contains_key(id) || state.roots.values().any(|root| root == id)
                })
            })?;
        let count = state.captures.entry(origin).or_default();
        *count = count.checked_add(1).expect("origin lease count exhausted");
        Some((
            origin,
            Lease {
                origin,
                registry: self.clone(),
            },
        ))
    }
    pub fn snapshot(&self) -> BTreeSet<InvocationId> {
        let state = self.0.lock().unwrap();
        state
            .roots
            .values()
            .copied()
            .chain(state.captures.keys().copied())
            .collect()
    }
}
