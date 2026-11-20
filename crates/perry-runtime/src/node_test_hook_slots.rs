//! The test module installs the probes reached by ordinary object dispatch.
use crate::feature_hooks::Hook;
static PROPERTY: Hook<fn(&str) -> Option<f64>> = Hook::empty();
static DISPATCH: Hook<fn(u32, &str) -> Option<f64>> = Hook::empty();
pub(crate) fn property(name: &str) -> Option<f64> {
    PROPERTY.get().and_then(|run| run(name))
}
pub(crate) fn dispatch_object_method(class_id: u32, name: &str) -> Option<f64> {
    DISPATCH.get().and_then(|run| run(class_id, name))
}
pub(crate) fn install() {
    PROPERTY.set(crate::node_test::property);
    DISPATCH.set(crate::node_test::dispatch_object_method);
}
