//! Borrow guards last until the exported result: Wasmtime task_complete returns
//! canonical resource borrows before publishing that result. Background work
//! needs no argument roots after that boundary. No JS values live here.
use crate::InvocationId;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
#[derive(Default)]
struct State {
    counts: BTreeMap<String, usize>,
    waiters: BTreeMap<InvocationId, Waker>,
}
#[derive(Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<State>>);
pub(crate) struct Guard {
    registry: Registry,
    keys: Vec<String>,
}
impl Registry {
    pub fn contains(&self, key: &str) -> bool {
        self.0.lock().unwrap().counts.contains_key(key)
    }
    pub fn acquire(&self, keys: Vec<String>) -> Guard {
        let mut state = self.0.lock().unwrap();
        for key in &keys {
            let count = state.counts.entry(key.clone()).or_default();
            *count = count
                .checked_add(1)
                .expect("resource borrow count exhausted");
        }
        Guard {
            registry: self.clone(),
            keys,
        }
    }
    pub fn wait(&self, key: String, invocation: InvocationId) -> Wait {
        Wait {
            registry: self.clone(),
            key,
            invocation,
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.keys.is_empty() {
            return;
        }
        let waiters = {
            let mut state = self.registry.0.lock().unwrap();
            for key in &self.keys {
                let count = state.counts.get_mut(key).expect("live resource borrow");
                *count -= 1;
                if *count == 0 {
                    state.counts.remove(key);
                }
            }
            std::mem::take(&mut state.waiters)
        };
        for wake in waiters.into_values() {
            wake.wake();
        }
    }
}
pub(crate) struct Wait {
    registry: Registry,
    key: String,
    invocation: InvocationId,
}
impl Future for Wait {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.registry.0.lock().unwrap();
        if !state.counts.contains_key(&self.key) {
            state.waiters.remove(&self.invocation);
            return Poll::Ready(());
        }
        state.waiters.insert(self.invocation, cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .unwrap()
            .waiters
            .remove(&self.invocation);
    }
}
