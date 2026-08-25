//! GC-root registration and scanning for native net handles.

use super::*;

static NET_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

extern "C" {
    fn js_register_net_socket_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
}

unsafe extern "C" fn ext_net_socket_handle_probe(handle: i64) -> bool {
    is_net_socket_handle(handle)
}

/// Register the net GC root scanner exactly once. Safe to call from any
/// `js_net_*` entry point on the main thread.
pub(crate) fn ensure_gc_scanner_registered() {
    NET_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-net", scan_net_roots);
        unsafe {
            js_register_net_socket_handle_probe(ext_net_socket_handle_probe);
        }
        // #2154 — publish the raw-consumer vtable for perry-ext-http (runs on
        // the first net FFI entry, before http could reference a socket).
        raw_bridge::register();
    });
}

/// GC root scanner for net.Socket event listener closures.
///
/// Without this, any GC cycle between `.on()` and the next dispatch would
/// sweep the closure; the next `closure.call*()` would dereference freed
/// memory. Same pattern as perry-stdlib's net mod and perry-ext-events.
pub(crate) fn scan_net_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut listeners) = statics::listeners().lock() {
        for per_socket in listeners.values_mut() {
            for cb_vec in per_socket.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }
    // `once_flags()` keys membership by the closure's ADDRESS BITS. The
    // canonical copy in `listeners()` above keeps the closure alive and is
    // rewritten when the copying GC moves it — but a `HashSet<i64>` element
    // cannot be rewritten in place, so without this rebuild the set still
    // holds the OLD address after evacuation: the once-membership test in
    // `lifecycle.rs` then misses, the listener is never auto-removed, and a
    // "once" callback fires on every subsequent event. Drain, forward each
    // element through the visitor, and reinsert under the new identity.
    if let Ok(mut once) = statics::once_flags().lock() {
        for per_handle in once.values_mut() {
            for set in per_handle.values_mut() {
                let old: Vec<i64> = set.drain().collect();
                for mut cb in old {
                    visitor.visit_i64_slot(&mut cb);
                    set.insert(cb);
                }
            }
        }
    }
    if let Ok(mut completions) = crate::lifecycle::socket_completions().lock() {
        for (_, callback) in completions.values_mut() {
            visitor.visit_i64_slot(callback);
        }
    }
    // #8259 — the pump's in-flight dispatch frames (snapshotted callbacks +
    // parked payloads), which the table walks above cannot see.
    dispatch_custody::scan(visitor);
}
