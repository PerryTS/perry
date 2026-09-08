//! Socket / server / block-list handle-id allocation.
//!
//! Split out of `lib.rs` to keep that file under the file-size gate. These
//! helpers wrap perry-ffi's SHARED handle-id allocator and add the
//! exhaustion-degradation policy (#6441).

/// Socket / server / block-list ids come from perry-ffi's SHARED handle-id
/// allocator, not a private counter.
///
/// Every ext staticlib that mints ids privately from 1 aliases the others
/// inside the shared `[1, 0x40000)` band, and the composite handle-method
/// dispatch (`class_handles.rs::composite_handle_method_dispatch`) asks each
/// registered extension "is this handle yours?" — so the FIRST extension whose
/// private counter reached that number claims the call. Next.js's HTTP server
/// (perry-ext-http handle 1, via `register_handle`) therefore claimed
/// `socket.on('data', …)` on this crate's socket 1: the listener landed on the
/// HTTP server, the reader delivered the MySQL greeting to an empty listener
/// list, and mysql2's handshake hung to ETIMEDOUT.
///
/// `reserve_handle_id` consumes an id from the same counter `register_handle`
/// uses, so ids stay globally unique across every ext library while this
/// crate keeps its own object map.
///
/// Returns [`perry_ffi::INVALID_HANDLE`] (`0`) when that shared band is
/// exhausted rather than aborting the process (#6441). Reserved ids currently
/// leak — a `net.Socket`'s id lives as long as the JS object, not the TCP
/// connection, so freeing on `'close'` would let a stale reference alias a
/// recycled socket (the aliasing class #6407 fixes) — so a long-running server
/// will eventually drain the band. Callers on a synchronous FFI path must route
/// the `0` sentinel through [`next_id_or_throw`]; background callers must guard
/// it explicitly (never register an object under `0`).
pub(crate) fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(net_registry_domain())
}

/// One authoritative domain for net's private payload maps, sharing only the
/// numeric allocation pool with FFI's ordinary payload registry.
pub(crate) fn net_registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: std::sync::LazyLock<perry_ffi::NativeRegistryDomain> =
        std::sync::LazyLock::new(|| {
            perry_ffi::NativeRegistryDomain::new().expect("net native registry domains exhausted")
        });
    *DOMAIN
}

/// [`next_id`] for the synchronous FFI entry points (`new net.Socket()`,
/// `net.createServer`, `net.connect`, `new net.BlockList()`,
/// `new net.SocketAddress()`): on band exhaustion, throw a recoverable,
/// JS-visible `EMFILE`-coded error instead of returning the `0` sentinel that
/// would otherwise be registered as a phantom socket. Node surfaces file-handle
/// exhaustion the same way, so `try/catch` and `'error'` handlers can react.
///
/// Diverges on exhaustion (via `perry_ffi::throw_with_code`), exactly like this
/// crate's Node argument validators — so it must be called BEFORE acquiring any
/// `statics::*` lock, which every alloc site already does (`let id = ...` first).
pub(crate) fn next_id_or_throw() -> i64 {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        perry_ffi::throw_with_code(
            "too many open handles: net handle-id space exhausted",
            "EMFILE",
            perry_ffi::ErrorKind::Error,
        );
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_net_id_names_the_private_domain() {
        let id = next_id();
        assert_ne!(id, perry_ffi::INVALID_HANDLE);
        let identity = perry_ffi::handle_registration(id).unwrap();
        assert_eq!(identity.domain(), net_registry_domain());
        assert_ne!(identity.domain(), perry_ffi::handle_registry_domain());
        let lease =
            perry_ffi::acquire_handle_registration(identity, perry_ffi::NativeLeaseKind::Operation)
                .unwrap();
        perry_ffi::free_handle_id(id);
        perry_ffi::drain_quarantined_handles();
        assert!(perry_ffi::handle_registration(id).is_none());
        drop(lease);
    }
}
