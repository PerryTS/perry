//! Synchronous stack-borrowed callback adapters for the lifecycle slots.
use super::AsyncResourceIds;
use crate::closure::ClosureHeader;
type Callback = unsafe fn(*mut ()) -> f64;
pub(crate) unsafe fn owned_provider_scope(
    resource: f64,
    context: *mut (),
    callback: Callback,
) -> Result<f64, f64> {
    super::owned_provider_scope(resource, || callback(context))
}
pub(crate) unsafe fn owned_provider_scope_rooted(
    scope: &crate::gc::RuntimeHandleScope,
    owner: &crate::gc::RuntimeHandle<'_>,
    context: *mut (),
    callback: Callback,
) -> Result<f64, f64> {
    super::owned_provider_scope_rooted(scope, owner, || callback(context))
}
pub(crate) unsafe fn run_provider_completion(
    type_name: &'static str,
    context: *mut (),
    callback: Callback,
) -> f64 {
    super::run_provider_completion(type_name, || callback(context))
}
pub(crate) unsafe fn run_resource_scope(
    ids: AsyncResourceIds,
    context: *mut (),
    callback: Callback,
) -> () {
    super::run_resource_scope(ids, || {
        callback(context);
    })
}
pub(crate) unsafe fn run_resource_scope_catching(
    ids: AsyncResourceIds,
    context: *mut (),
    callback: Callback,
) -> f64 {
    super::run_resource_scope_catching(ids, || callback(context))
}
pub(crate) unsafe fn run_with_context_snapshot(
    closure: *const ClosureHeader,
    context: *mut (),
    callback: Callback,
) -> f64 {
    super::run_with_context_snapshot(closure, || callback(context))
}
