//! `process.permission` model — flag parsing, scope/path checks, and the
//! `has`/`drop` method object. Split out of the `process` trunk. Pure code
//! move — no behavior change.

use super::*;
use crate::value::JSValue;

pub(crate) fn process_permission_enabled() -> bool {
    let mut enabled = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--permission" => enabled = true,
            "--no-permission" => enabled = false,
            _ => {}
        }
    }
    enabled
}

fn process_permission_flag_values(flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let prefix = format!("{flag}=");
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            values.extend(
                value
                    .split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.to_string()),
            );
        } else if arg == flag {
            if let Some(next) = args.peek() {
                if !next.starts_with("--") {
                    if let Some(value) = args.next() {
                        values.extend(
                            value
                                .split(',')
                                .filter(|part| !part.is_empty())
                                .map(|part| part.to_string()),
                        );
                    }
                } else {
                    values.push("*".to_string());
                }
            } else {
                values.push("*".to_string());
            }
        }
    }
    values
}

fn process_permission_has_flag(flag: &str) -> bool {
    std::env::args().skip(1).any(|arg| arg == flag)
}

fn permission_canonical_path(path: &str) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn permission_path_allowed(reference: &str, allowed: &[String]) -> bool {
    if allowed.iter().any(|entry| entry == "*") {
        return true;
    }
    let reference_path = permission_canonical_path(reference);
    for entry in allowed {
        if entry == reference {
            return true;
        }
        if let (Some(reference_path), Some(allowed_path)) =
            (reference_path.as_ref(), permission_canonical_path(entry))
        {
            if reference_path == &allowed_path || reference_path.starts_with(&allowed_path) {
                return true;
            }
        }
    }
    false
}

fn process_permission_is_dropped(scope: &str, reference: Option<&str>) -> bool {
    PROCESS_PERMISSION_DROPS.with(|drops| {
        drops.borrow().iter().any(|drop| {
            if drop.scope != scope {
                return false;
            }
            match (&drop.reference, reference) {
                (None, _) => true,
                (Some(drop_reference), Some(reference)) => {
                    permission_path_allowed(reference, std::slice::from_ref(drop_reference))
                }
                _ => false,
            }
        })
    })
}

fn process_permission_drop(scope: &str, reference: Option<String>) {
    PROCESS_PERMISSION_DROPS.with(|drops| {
        let mut drops = drops.borrow_mut();
        if reference.is_none() {
            drops.retain(|drop| drop.scope != scope);
        }
        drops.push(ProcessPermissionDrop {
            scope: scope.to_string(),
            reference,
        });
    });
}

fn process_permission_scope_allowed(scope: &str, reference: Option<&str>) -> bool {
    if process_permission_is_dropped(scope, reference) {
        return false;
    }
    match scope {
        "fs.read" => {
            let allowed = process_permission_flag_values("--allow-fs-read");
            match reference {
                Some(reference) => permission_path_allowed(reference, &allowed),
                None => allowed.iter().any(|entry| entry == "*"),
            }
        }
        "fs.write" => {
            let allowed = process_permission_flag_values("--allow-fs-write");
            match reference {
                Some(reference) => permission_path_allowed(reference, &allowed),
                None => allowed.iter().any(|entry| entry == "*"),
            }
        }
        "child" => process_permission_has_flag("--allow-child-process"),
        "worker" => process_permission_has_flag("--allow-worker"),
        "addon" => process_permission_has_flag("--allow-addons"),
        _ => false,
    }
}

fn throw_permission_arg_type(name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type string. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

extern "C" fn process_permission_has_thunk(
    _closure: *const crate::closure::ClosureHeader,
    scope_value: f64,
    reference_value: f64,
) -> f64 {
    let Some(scope) = module_value_to_string(scope_value) else {
        throw_permission_arg_type("scope", scope_value);
    };
    let reference_js = JSValue::from_bits(reference_value.to_bits());
    let reference = if reference_js.is_undefined() || reference_js.is_null() {
        None
    } else if let Some(reference) = module_value_to_string_or_buffer(reference_value) {
        Some(reference)
    } else {
        throw_permission_arg_type("reference", reference_value);
    };
    bool_value(process_permission_scope_allowed(
        &scope,
        reference.as_deref(),
    ))
}

extern "C" fn process_permission_drop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    scope_value: f64,
    reference_value: f64,
) -> f64 {
    let Some(scope) = module_value_to_string(scope_value) else {
        throw_permission_arg_type("scope", scope_value);
    };
    let reference_js = JSValue::from_bits(reference_value.to_bits());
    let reference = if reference_js.is_undefined() || reference_js.is_null() {
        None
    } else if let Some(reference) = module_value_to_string_or_buffer(reference_value) {
        Some(reference)
    } else {
        throw_permission_arg_type("reference", reference_value);
    };
    process_permission_drop(&scope, reference);
    undefined_value()
}

pub(crate) fn process_permission_value() -> Option<f64> {
    if !process_permission_enabled() {
        return None;
    }
    use std::cell::Cell;
    thread_local! {
        static CACHED_PERMISSION: Cell<f64> = const { Cell::new(0.0) };
    }

    let cached = CACHED_PERMISSION.with(|c| c.get());
    if cached != 0.0 {
        return Some(cached);
    }

    let obj = crate::object::js_object_alloc(0, 2);
    module_set_field(
        obj,
        "has",
        module_function2("has", process_permission_has_thunk, 2),
    );
    module_set_field(
        obj,
        "drop",
        module_function2("drop", process_permission_drop_thunk, 2),
    );
    let value = module_object_value(obj);
    CACHED_PERMISSION.with(|c| c.set(value));
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    Some(value)
}
