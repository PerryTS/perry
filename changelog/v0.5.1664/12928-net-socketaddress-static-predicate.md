Fix missing receiverless static `SocketAddress.isSocketAddress` registration.
Direct calls and extracted method values share one boxed, lifecycle-checked predicate; the internal raw-handle ABI is unchanged.
Pinned Node 26.5.1 validation covers the 54-row regression and existing defaults RED/GREEN; independent verification preserves three controls byte-for-byte in raw output.
This repairs own-payload checks only: inherited branding, proxy symbol-get observability, and revoked-proxy behavior remain unresolved; the NET module is incomplete.
