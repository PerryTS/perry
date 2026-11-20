//! Exception-safe entry and cleanup for async-resource execution scopes.

use super::{after, before, destroy, init_resource, AsyncResourceIds};

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
    let result = match outcome {
        Ok(value) => Ok(scope.root_nanbox_f64(value)),
        Err(error) => Err(error.root(&scope)),
    };
    let destroy_outcome = crate::exception::catch_thrown(|| {
        destroy(ids.async_id);
        TAG_UNDEFINED_F64
    });
    let destroy_error = match destroy_outcome {
        Ok(_) => None,
        Err(error) => Some(error.root(&scope)),
    };
    if result.is_err() {
        crate::exception::rethrow_caught(result.as_ref().err().unwrap().caught());
    }
    if let Some(error) = destroy_error {
        crate::exception::rethrow_caught(error.caught());
    }
    result.ok().unwrap().get_nanbox_f64()
}

/// Enter an existing provider's captured AsyncLocalStorage and execution-id
/// scope for one native callback phase.
///
/// The scope's restore guards belong to the calling frame (or, called from a
/// helper the owner calls directly, the owner's: `crate::exception::frame_mark`).
#[inline(always)]
pub fn try_enter_resource_scope(ids: AsyncResourceIds) -> Result<(), crate::exception::Caught> {
    try_enter_resource_scope_at(crate::exception::frame_mark(), ids)
}

/// [`try_enter_resource_scope`] for the scope owned by the frame `mark` names.
pub fn try_enter_resource_scope_at(
    mark: usize,
    ids: AsyncResourceIds,
) -> Result<(), crate::exception::Caught> {
    let context = super::resource_state::get(ids.async_id)
        .map(|meta| unsafe { (*meta).context.clone() })
        .unwrap_or_default();
    try_enter_scope_at(mark, ids, context, false)
}

fn try_enter_scope_at(
    mark: usize,
    ids: AsyncResourceIds,
    context: crate::async_context::AsyncContextSnapshot,
    promise: bool,
) -> Result<(), crate::exception::Caught> {
    let previous = crate::async_context::enter_context(&context);
    drop(context);
    crate::async_context::push_context_guard_at(
        mark,
        crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
    );
    crate::async_context::push_context_guard_at(
        mark,
        crate::async_context::ContextGuardAction::RestoreExecutionIds,
    );
    let outcome = crate::exception::catch_thrown(|| {
        if promise {
            super::before_promise(ids.async_id, ids.trigger_async_id);
        } else {
            before(ids.async_id, ids.trigger_async_id);
        }
        TAG_UNDEFINED_F64
    });
    if let Err(error) = outcome {
        let scope = crate::gc::RuntimeHandleScope::new();
        let error = error.root(&scope);
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
        return Err(error.caught());
    }
    Ok(())
}

#[inline(always)]
pub fn enter_resource_scope(ids: AsyncResourceIds) {
    if let Err(error) = try_enter_resource_scope(ids) {
        crate::exception::rethrow_caught(error);
    }
}

/// Leave a provider scope entered by [`enter_resource_scope`].
pub fn try_leave_resource_scope(async_id: u64) -> Result<(), crate::exception::Caught> {
    try_leave_scope(async_id, false)
}

fn try_leave_scope(async_id: u64, promise: bool) -> Result<(), crate::exception::Caught> {
    let outcome = crate::exception::catch_thrown(|| {
        if promise {
            super::after_promise(async_id);
        } else {
            after(async_id);
        }
        TAG_UNDEFINED_F64
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = match outcome {
        Ok(value) => Ok(scope.root_nanbox_f64(value)),
        Err(error) => Err(error.root(&scope)),
    };
    if let Some(action) = crate::async_context::pop_context_guard() {
        if result.is_err() {
            crate::async_context::apply_context_guard(action);
        }
    }
    if let Some(action) = crate::async_context::pop_context_guard() {
        crate::async_context::apply_context_guard(action);
    }
    if result.is_err() {
        Err(result.as_ref().err().unwrap().caught())
    } else {
        Ok(())
    }
}

pub fn leave_resource_scope(async_id: u64) {
    if let Err(error) = try_leave_resource_scope(async_id) {
        crate::exception::rethrow_caught(error);
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
) -> Result<f64, crate::exception::Caught> {
    try_enter_resource_scope(ids)?;
    try_run_entered_scope(ids, false, completion)
}

/// Run a native Promise producer in its submission context, including when
/// async hooks were disabled at creation (and no resource metadata exists).
/// The caller must keep the Promise rooted throughout this call.
pub(crate) unsafe fn try_run_promise_scope(
    promise: *mut crate::promise::Promise,
    completion: impl FnOnce() -> f64,
) -> Result<f64, crate::exception::Caught> {
    let ids = AsyncResourceIds {
        async_id: (*promise).async_id,
        trigger_async_id: (*promise).trigger_async_id,
    };
    let context = crate::promise::context_for_promise(promise);
    crate::async_hooks::try_run_captured_promise_scope(ids, context, completion)
}

/// Continue an invocation after its public Promise has been released. The
/// caller owns the captured snapshot in traced state and supplies its native
/// execution IDs; the execution scope does not retain or resurrect the Promise.
/// Observed IDs may be opaque resource-state cell tokens: callers must retain
/// those cells in traced state across polls, or capture public serial IDs instead.
/// A resource-state cell holds only a weak edge to its Promise.
pub(crate) fn try_run_captured_promise_scope(
    ids: AsyncResourceIds,
    context: crate::async_context::AsyncContextSnapshot,
    completion: impl FnOnce() -> f64,
) -> Result<f64, crate::exception::Caught> {
    try_enter_scope_at(crate::exception::frame_mark(), ids, context, true)?;
    try_run_entered_scope(ids, true, completion)
}

#[cfg(test)]
mod tests;

fn try_run_entered_scope(
    ids: AsyncResourceIds,
    promise: bool,
    completion: impl FnOnce() -> f64,
) -> Result<f64, crate::exception::Caught> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let outcome = crate::exception::catch_thrown(completion);
    let result = match outcome {
        Ok(value) => Ok(scope.root_nanbox_f64(value)),
        Err(error) => Err(error.root(&scope)),
    };
    if let Err(error) = try_leave_scope(ids.async_id, promise) {
        if result.is_err() {
            return Err(result.as_ref().err().unwrap().caught());
        }
        let error = error.root(&scope);
        return Err(error.caught());
    }
    if result.is_err() {
        Err(result.as_ref().err().unwrap().caught())
    } else {
        Ok(result.ok().unwrap().get_nanbox_f64())
    }
}

pub fn run_resource_scope_catching(ids: AsyncResourceIds, completion: impl FnOnce() -> f64) -> f64 {
    match try_run_resource_scope(ids, completion) {
        Ok(value) => value,
        Err(error) => crate::exception::rethrow_caught(error),
    }
}
