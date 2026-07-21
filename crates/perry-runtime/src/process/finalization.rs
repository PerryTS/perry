//! `process.getReport`-adjacent finalization registry: the
//! `FinalizationRegistry`-shaped exit/beforeExit callback bookkeeping split out
//! of the `process` trunk. Pure code move — no behavior change.

use super::*;

extern "C" fn process_finalization_before_exit_listener(
    _closure: *const crate::closure::ClosureHeader,
    _code: f64,
) -> f64 {
    js_process_run_finalization_before_exit();
    undefined_value()
}

fn process_finalization_before_exit_listener_ptr() -> *const crate::closure::ClosureHeader {
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| {
        let existing = cell.get();
        if !existing.is_null() {
            return existing;
        }
        let func_ptr = process_finalization_before_exit_listener as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 1);
        crate::closure::js_register_closure_length(func_ptr, 1);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        crate::object::set_bound_native_closure_name(closure, "processFinalizationBeforeExit");
        crate::object::set_builtin_closure_length(closure as usize, 1);
        cell.set(closure);
        closure
    })
}

fn process_finalization_has_before_exit_entries() -> bool {
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .any(|entry| entry.kind == ProcessFinalizationKind::BeforeExit)
    })
}

fn ensure_process_finalization_before_exit_listener() {
    let callback = process_finalization_before_exit_listener_ptr();
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED.with(|installed| {
        if installed.replace(true) {
            return;
        }
        crate::os::add_internal_process_listener("beforeExit", callback);
    });
}

fn sync_process_finalization_before_exit_listener() {
    if process_finalization_has_before_exit_entries() {
        ensure_process_finalization_before_exit_listener();
        return;
    }
    let callback = PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| cell.get());
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER_INSTALLED.with(|installed| {
        if !installed.replace(false) {
            return;
        }
        crate::os::remove_internal_process_listener("beforeExit", callback);
    });
}

fn process_finalization_ref_is_valid(value: f64) -> bool {
    if is_function_value(value) {
        return true;
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return false;
    }
    module_object_ptr(value).is_some()
}

fn validate_process_finalization_ref(value: f64) {
    if process_finalization_ref_is_valid(value) {
        return;
    }
    let message = format!(
        "The \"obj\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn process_finalization_register(kind: ProcessFinalizationKind, obj: f64, callback: f64) -> f64 {
    validate_process_finalization_ref(obj);
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry.borrow_mut().push(ProcessFinalizationEntry {
            obj,
            callback,
            kind,
        });
    });
    if kind == ProcessFinalizationKind::BeforeExit {
        ensure_process_finalization_before_exit_listener();
    }
    undefined_value()
}

fn process_finalization_unregister(obj: f64) -> f64 {
    let obj_bits = obj.to_bits();
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .retain(|entry| entry.obj.to_bits() != obj_bits);
    });
    sync_process_finalization_before_exit_listener();
    undefined_value()
}

fn process_finalization_mark_ran(kind: ProcessFinalizationKind) -> bool {
    match kind {
        ProcessFinalizationKind::BeforeExit => {
            PROCESS_FINALIZATION_BEFORE_EXIT_RAN.with(|ran| ran.replace(true))
        }
        ProcessFinalizationKind::Exit => {
            PROCESS_FINALIZATION_EXIT_RAN.with(|ran| ran.replace(true))
        }
    }
}

fn process_finalization_event_name(kind: ProcessFinalizationKind) -> &'static str {
    match kind {
        ProcessFinalizationKind::BeforeExit => "beforeExit",
        ProcessFinalizationKind::Exit => "exit",
    }
}

fn run_process_finalization_callbacks(kind: ProcessFinalizationKind) {
    if process_finalization_mark_ran(kind) {
        return;
    }
    let entries = PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        registry
            .borrow()
            .iter()
            .filter(|entry| entry.kind == kind)
            .copied()
            .collect::<Vec<_>>()
    });
    if entries.is_empty() {
        return;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let event_handle =
        scope.root_nanbox_f64(module_string_value(process_finalization_event_name(kind)));
    let handles = entries
        .iter()
        .map(|entry| {
            (
                scope.root_nanbox_f64(entry.obj),
                scope.root_nanbox_f64(entry.callback),
            )
        })
        .collect::<Vec<_>>();

    for (obj_handle, callback_handle) in handles {
        let callback = callback_handle.get_nanbox_f64();
        if !is_function_value(callback) {
            crate::closure::throw_not_callable();
        }
        let args = [obj_handle.get_nanbox_f64(), event_handle.get_nanbox_f64()];
        unsafe {
            crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
        }
    }
}

pub fn scan_process_finalization_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROCESS_FINALIZATION_OBJECT.with(|cell| {
        let mut value = cell.get();
        if value != 0.0 && visitor.visit_nanbox_f64_slot(&mut value) {
            cell.set(value);
        }
    });
    PROCESS_FINALIZATION_REGISTRY.with(|registry| {
        for entry in registry.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut entry.obj);
            visitor.visit_nanbox_f64_slot(&mut entry.callback);
        }
    });
    PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER.with(|cell| {
        let mut callback = cell.get();
        if !callback.is_null() && visitor.visit_raw_const_ptr_slot(&mut callback) {
            cell.set(callback);
        }
    });
}

extern "C" fn process_finalization_register_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    callback: f64,
) -> f64 {
    process_finalization_register(ProcessFinalizationKind::Exit, obj, callback)
}

extern "C" fn process_finalization_register_before_exit_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
    callback: f64,
) -> f64 {
    process_finalization_register(ProcessFinalizationKind::BeforeExit, obj, callback)
}

extern "C" fn process_finalization_unregister_function(
    _closure: *const crate::closure::ClosureHeader,
    obj: f64,
) -> f64 {
    process_finalization_unregister(obj)
}

#[no_mangle]
pub extern "C" fn js_process_run_finalization_before_exit() {
    run_process_finalization_callbacks(ProcessFinalizationKind::BeforeExit);
}

#[no_mangle]
pub extern "C" fn js_process_run_finalization_exit() {
    run_process_finalization_callbacks(ProcessFinalizationKind::Exit);
}

pub(crate) fn process_finalization_value() -> f64 {
    let cached = PROCESS_FINALIZATION_OBJECT.with(|c| c.get());
    if cached != 0.0 {
        return cached;
    }

    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(
        obj,
        "register",
        module_function2("register", process_finalization_register_function, 2),
    );
    module_set_field(
        obj,
        "registerBeforeExit",
        module_function2(
            "registerBeforeExit",
            process_finalization_register_before_exit_function,
            2,
        ),
    );
    module_set_field(
        obj,
        "unregister",
        module_function1("unregister", process_finalization_unregister_function, 1),
    );
    let value = module_object_value(obj);
    PROCESS_FINALIZATION_OBJECT.with(|c| c.set(value));
    value
}
