//! Host validation and guest/async ownership receipts share one transactional
//! callback. The persistent codec can release result roots at actual commit.
use super::*;
use crate::{ResourceEvent, ResourceRef};
use serde_json::Value;

type Wire = Arc<dyn Fn(Value) -> Result<Vec<ResourceRef>> + Send + Sync>;

pub(super) fn hooks(
    callback: Option<LifecycleCallback>,
    free: Option<FreeCallback>,
    current: Arc<AtomicU64>,
    failures: Arc<async_calls::Replies>,
) -> (ResourceHook, crate::transport::LoweringHook) {
    let wire: Wire = Arc::new(move |event| {
        // Native-only clients need not register a lifecycle callback for
        // guest/future transfers. Host payload identities still require it.
        if callback.is_none()
            && (event["kind"] == "drain"
                || (event["kind"] == "lower"
                    && event["created"].as_array().is_some_and(Vec::is_empty)
                    && event["transferred"].as_array().is_some_and(Vec::is_empty)))
        {
            return Ok(Vec::new());
        }
        let callback = callback.context("host resources require a resource callback")?;
        let free = free.context("host resources require a reply deallocator")?;
        let context = current.load(Ordering::Acquire);
        ensure!(context != 0, "resource event outside a live embedding call");
        let event = serde_json::to_vec(&event)?;
        let mut reply = Reply(Buffer::default(), free);
        let status = unsafe { callback.call(context, &event, &mut reply.0) };
        failures.callback_status(status);
        if status != OK {
            bail!(
                "component resource event: {}",
                String::from_utf8_lossy(unsafe { bytes(reply.0.data, reply.0.len)? })
            );
        }
        Ok(serde_json::from_slice(unsafe {
            bytes(reply.0.data, reply.0.len)?
        })?)
    });
    let lifecycle = wire.clone();
    let resource: ResourceHook = Arc::new(move |event| lifecycle(serde_json::to_value(event)?));
    let lowering: crate::transport::LoweringHook = Arc::new(move |event, guest, asynchronous| {
        debug_assert!(matches!(event, ResourceEvent::Lower { .. }));
        let mut event = serde_json::to_value(event)?;
        if !guest.is_empty() {
            event["guestTransferred"] = serde_json::to_value(guest)?;
        }
        if !asynchronous.is_empty() {
            event["asyncTransferred"] = serde_json::to_value(asynchronous)?;
        }
        wire(event)?;
        Ok(())
    });
    (resource, lowering)
}
