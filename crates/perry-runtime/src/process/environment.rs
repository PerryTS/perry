//! Agent-owned native environment state; no managed values cross threads.
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

type Entries = Vec<(String, String)>;

#[derive(Clone)]
pub(super) enum Environment {
    Os,
    Private(Arc<Mutex<Entries>>),
}

thread_local! {
    static CURRENT: RefCell<Environment> = const { RefCell::new(Environment::Os) };
}

impl Environment {
    pub(super) fn copied(entries: Entries) -> Self {
        Self::Private(Arc::new(Mutex::new(entries)))
    }

    pub(super) fn entries(&self) -> Entries {
        match self {
            Self::Os => std::env::vars().collect(),
            Self::Private(entries) => entries.lock().unwrap().clone(),
        }
    }

    pub(super) fn install(self) {
        CURRENT.with(|current| *current.borrow_mut() = self);
    }
}

pub(super) fn current() -> Environment {
    CURRENT.with(|current| current.borrow().clone())
}

pub(super) fn get(name: &str) -> Option<String> {
    match current() {
        Environment::Os => std::env::var(name).ok(),
        Environment::Private(entries) => entries
            .lock()
            .unwrap()
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone()),
    }
}

pub(super) fn contains(name: &str) -> bool {
    match current() {
        Environment::Os => std::env::var_os(name).is_some(),
        Environment::Private(entries) => entries.lock().unwrap().iter().any(|(key, _)| key == name),
    }
}

pub(super) fn set(name: &str, value: &str) {
    match current() {
        Environment::Os => std::env::set_var(name, value),
        Environment::Private(entries) => {
            let mut entries = entries.lock().unwrap();
            if let Some((_, previous)) = entries.iter_mut().find(|(key, _)| key == name) {
                *previous = value.into();
            } else {
                entries.push((name.into(), value.into()));
            }
        }
    }
}

pub(super) fn remove(name: &str) {
    match current() {
        Environment::Os => std::env::remove_var(name),
        Environment::Private(entries) => entries.lock().unwrap().retain(|(key, _)| key != name),
    }
}
