//! Private callback EXIT notifications. A candidate becomes observable only
//! after the enclosing VM poll succeeds, including its exit validation.
mod instrument;
use crate::{asynchronous::retention::Registry as Origins, InvocationId, StoreData};
use anyhow::Result;
use futures_channel::oneshot;
use perry_wit::World;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use wasmtime::component::{GuestTaskId, Linker};

#[derive(Default)]
struct State {
    candidates: BTreeSet<(GuestTaskId, InvocationId)>,
    waiters: BTreeMap<InvocationId, oneshot::Sender<()>>,
}
#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<State>>);
impl Registry {
    pub fn wait(&self, origin: InvocationId) -> oneshot::Receiver<()> {
        let (send, receive) = oneshot::channel();
        assert!(self
            .0
            .lock()
            .unwrap()
            .waiters
            .insert(origin, send)
            .is_none());
        receive
    }
    pub fn candidate(&self, task: GuestTaskId, origin: InvocationId) {
        self.0.lock().unwrap().candidates.insert((task, origin));
    }
    pub fn acknowledge(&self, origins: &Origins) -> bool {
        let (candidates, sends) = {
            let mut state = self.0.lock().unwrap();
            let candidates = std::mem::take(&mut state.candidates);
            let sends = candidates
                .iter()
                .filter_map(|(_, origin)| state.waiters.remove(origin))
                .collect::<Vec<_>>();
            (candidates, sends)
        };
        for (task, origin) in &candidates {
            origins.remove(*task, *origin);
        }
        for send in sends {
            let _ = send.send(());
        }
        !candidates.is_empty()
    }
    pub fn fail(&self) {
        let state = std::mem::take(&mut *self.0.lock().unwrap());
        drop(state);
    }
}

pub(crate) struct Prepared {
    pub bytes: Vec<u8>,
    hooks: Vec<String>,
    pub targets: BTreeSet<(String, String)>,
}
impl Prepared {
    pub fn new(bytes: &[u8], world: &World, enabled: bool) -> Result<Self> {
        let mut prepared = Self {
            bytes: bytes.to_vec(),
            hooks: Vec::new(),
            targets: BTreeSet::new(),
        };
        if enabled {
            for interface in &world.exports {
                for function in &interface.functions {
                    if !function.async_ {
                        continue;
                    }
                    if let Some(output) =
                        instrument::instrument(&prepared.bytes, &interface.name, &function.name)?
                    {
                        prepared.bytes = output.bytes;
                        prepared.hooks.push(output.hook);
                        prepared
                            .targets
                            .insert((interface.name.clone(), function.name.clone()));
                    }
                }
            }
        }
        Ok(prepared)
    }
    pub fn link(&self, linker: &mut Linker<StoreData>) -> Result<()> {
        for hook in &self.hooks {
            linker
                .root()
                .func_new(hook, |mut store, _, params, results| {
                    debug_assert!(params.is_empty() && results.is_empty());
                    let task = store.async_call_stack()?.next();
                    if let Some(task) = task {
                        if let Some(origin) = store.data().origins.registry.exact(task) {
                            store.data().task_exits.candidate(task, origin);
                        }
                    }
                    Ok(())
                })?;
        }
        Ok(())
    }
}
