//! K3 C3: the bodies of this module whose receiver family is live at the
//! whole-program fixpoint (`ProgramFactsView::recv_family_live`). Their
//! `JsFunctionInfo` carries the home-key record the runtime's receiver
//! check reads (`fn_info::FnInfoFacts::family`). Set per module thread at
//! the start of `compile_module`, as `verify_facts::bodies` is.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

type Live = Arc<BTreeMap<(u32, String), Vec<String>>>;

thread_local! {
    static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
}

pub(crate) fn install(view: &crate::codegen::ProgramFactsView) {
    LIVE.with(|l| *l.borrow_mut() = view.recv_family_live.clone());
}

/// The home keys of body `id` (HIR function id) when its family is live.
pub(crate) fn keys(id: u32) -> Option<Vec<String>> {
    LIVE.with(|l| l.borrow().as_ref()?.get(&(id, String::new())).cloned())
}

/// Every live body of this module, by HIR function id.
pub(crate) fn all() -> Vec<(u32, Vec<String>)> {
    LIVE.with(|l| {
        l.borrow()
            .iter()
            .flat_map(|m| m.iter())
            .filter(|((_, name), _)| name.is_empty())
            .map(|((id, _), k)| (*id, k.clone()))
            .collect()
    })
}
