//! Exception-safe entry and cleanup for async-resource execution scopes.

use super::{after, before, destroy, init_resource, AsyncResourceIds, RESOURCES};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

/// Run a synchronous native completion as an observable async-hooks provider.
/// The returned value stays rooted while arbitrary JavaScript hooks run.
pub fn run_provider_completion(type_name: &'static str, completion: impl FnOnce() -> f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = crate::object::js_object_alloc_null_proto(0, 0);
    let resource_handle = scope.root_raw_mut_ptr(resource);
    let ids = resource_handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|resource| {
        init_resource(
            type_name,
            crate::value::js_nanbox_pointer(resource as i64),
            true,
        )
    });
    let outcome = try_run_resource_scope(ids, completion);
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let destroy_outcome = crate::exception::js_call_catching(|| {
        destroy(ids.async_id);
        TAG_UNDEFINED_F64
    });
    let destroy_error = destroy_outcome
        .err()
        .map(|error| scope.root_nanbox_f64(error));
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    if let Some(error) = destroy_error {
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}

/// Enter an existing provider's captured AsyncLocalStorage and execution-id
/// scope for one native callback phase.
pub fn try_enter_resource_scope(ids: AsyncResourceIds) -> Result<(), f64> {
    let context = RESOURCES
        .lock()
        .unwrap()
        .get(&ids.async_id)
        .map(|meta| meta.context.clone())
        .unwrap_or_default();
    let previous = crate::async_context::enter_context(&context);
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
    );
    crate::async_context::push_context_guard(
        crate::async_context::ContextGuardAction::RestoreExecutionIds,
    );
    let outcome = crate::exception::js_call_catching(|| {
        before(ids.async_id, ids.trigger_async_id);
        TAG_UNDEFINED_F64
    });
    if let Err(error) = outcome {
        let scope = crate::gc::RuntimeHandleScope::new();
        let error = scope.root_nanbox_f64(error);
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        return Err(error.get_nanbox_f64());
    }
    Ok(())
}

pub fn enter_resource_scope(ids: AsyncResourceIds) {
    if let Err(error) = try_enter_resource_scope(ids) {
        crate::exception::js_throw(error);
    }
}

/// Leave a provider scope entered by [`enter_resource_scope`].
pub fn try_leave_resource_scope(async_id: u64) -> Result<(), f64> {
    let outcome = crate::exception::js_call_catching(|| {
        after(async_id);
        TAG_UNDEFINED_F64
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    if let Some(action) = crate::async_context::pop_context_guard() {
        if threw {
            crate::async_context::apply_context_guard(action);
        }
    }
    if let Some(action) = crate::async_context::pop_context_guard() {
        crate::async_context::apply_context_guard(action);
    }
    if threw {
        Err(result.get_nanbox_f64())
    } else {
        Ok(())
    }
}

pub fn leave_resource_scope(async_id: u64) {
    if let Err(error) = try_leave_resource_scope(async_id) {
        crate::exception::js_throw(error);
    }
}

pub fn run_resource_scope(ids: AsyncResourceIds, completion: impl FnOnce()) {
    let _ = run_resource_scope_catching(ids, || {
        completion();
        TAG_UNDEFINED_F64
    });
}

/// Execute user code inside an existing provider and return its exception only
/// after the provider context and execution-id stacks have been restored.
pub fn try_run_resource_scope(
    ids: AsyncResourceIds,
    completion: impl FnOnce() -> f64,
) -> Result<f64, f64> {
    try_enter_resource_scope(ids)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let outcome = crate::exception::js_call_catching(completion);
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    if let Err(error) = try_leave_resource_scope(ids.async_id) {
        if threw {
            return Err(result.get_nanbox_f64());
        }
        let error = scope.root_nanbox_f64(error);
        return Err(error.get_nanbox_f64());
    }
    if threw {
        Err(result.get_nanbox_f64())
    } else {
        Ok(result.get_nanbox_f64())
    }
}

pub fn run_resource_scope_catching(ids: AsyncResourceIds, completion: impl FnOnce() -> f64) -> f64 {
    match try_run_resource_scope(ids, completion) {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}
