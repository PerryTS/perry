//! Active AST pins are true interpreter frames, released by root-stack
//! truncation on ordinary return and on throws. There is no owner registry.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
mod active_functions_storage {
    use super::*;
    type Value = RefCell<Vec<(usize, Rc<super::super::InterpFn>)>>;
    type Storage = crate::tls_hot::HotCell<Value, { std::mem::needs_drop::<Value>() as usize }>;
    crate::__perry_thread_local_storage!(Storage, expr(RefCell::new(Vec::new())));
    pub(super) fn resolve(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
        STORAGE.try_with(|cell| {
            cell.arm_guard(idx);
            cell.value_addr()
        })
    }
    pub(super) static KEY: crate::tls_hot::HotKey<Value> = crate::tls_hot::HotKey::new(
        crate::tls_hot::SlotId::named(concat!(
            module_path!(),
            "::ACTIVE_FUNCTIONS@",
            line!(),
            ":",
            column!()
        )),
        crate::dyn_eval_hook_roots::resolve_active_functions_storage,
    );
}
use active_functions_storage::KEY as ACTIVE_FUNCTIONS;
mod pin_top_storage {
    use super::*;
    type Value = Cell<usize>;
    type Storage = crate::tls_hot::HotCell<Value, { std::mem::needs_drop::<Value>() as usize }>;
    crate::__perry_thread_local_storage!(Storage, const { Cell::new(0) });
    pub(super) fn resolve(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
        STORAGE.try_with(|cell| {
            cell.arm_guard(idx);
            cell.value_addr()
        })
    }
    pub(super) static KEY: crate::tls_hot::HotKey<Value> = crate::tls_hot::HotKey::new(
        crate::tls_hot::SlotId::named(concat!(
            module_path!(),
            "::PIN_TOP@",
            line!(),
            ":",
            column!()
        )),
        crate::dyn_eval_hook_roots::resolve_pin_top_storage,
    );
}
use pin_top_storage::KEY as PIN_TOP;

/// A borrowed AST backed by an explicit root-stack pin. Ordinary returns
/// truncate through Drop; exception restore truncates the same stack directly.
pub(super) struct FunctionPin {
    function: *const super::InterpFn,
    root: usize,
}

impl std::ops::Deref for FunctionPin {
    type Target = super::InterpFn;

    fn deref(&self) -> &Self::Target {
        // SAFETY: ACTIVE_FUNCTIONS owns an Rc until this pin's root is
        // truncated, and the collecting cell owns another Rc while its root is live. Callers finish all AST reads before truncating that
        // root; a throw that truncates it never returns to the abandoned
        // caller.
        unsafe { &*self.function }
    }
}

impl Drop for FunctionPin {
    fn drop(&mut self) {
        super::roots_truncate(self.root);
    }
}

pub(super) fn pin_function(id: f64) -> Option<FunctionPin> {
    let function = super::lookup_fn(id)?;
    let ptr = Rc::as_ptr(&function);
    let root = super::root_push(id);
    ACTIVE_FUNCTIONS.with(|pins| pins.borrow_mut().push((root, function)));
    PIN_TOP.with(|top| top.set(root + 1));
    Some(FunctionPin {
        function: ptr,
        root,
    })
}

#[inline]
pub(super) fn release_function_pins(root_len: usize) {
    if PIN_TOP.with(Cell::get) <= root_len {
        return;
    }
    ACTIVE_FUNCTIONS.with(|pins| {
        let mut pins = pins.borrow_mut();
        while pins.last().is_some_and(|(root, _)| *root >= root_len) {
            pins.pop();
        }
        PIN_TOP.with(|top| top.set(pins.last().map_or(0, |(root, _)| root + 1)));
    });
}

/// A parent's own AST memo dies with that parent. Children have independent
/// Rc ownership, so a live nested closure does not retain a dead parent's AST.
pub(super) fn node_fn_id(parent: f64, addr: usize, build: impl FnOnce() -> super::InterpFn) -> f64 {
    let parent = super::lookup_fn(parent).expect("executing function owns its AST");
    let cached = parent.node_functions.borrow().get(&addr).cloned();
    let function = match cached {
        Some(function) => function,
        None => {
            let function = Rc::new(build());
            parent
                .node_functions
                .borrow_mut()
                .insert(addr, function.clone());
            function
        }
    };
    super::function_cell::new(function)
}
#[cfg(test)]
#[path = "function_owner_tests.rs"]
mod tests;

pub(super) fn install_storage_resolvers() {
    crate::dyn_eval_hook_roots::set_active_functions_storage(active_functions_storage::resolve);
    crate::dyn_eval_hook_roots::set_pin_top_storage(pin_top_storage::resolve);
}
