Closed the remaining #4975 review findings on the HTTP/HTTPS parity work:
exact mixed on/once ClientRequest listener ordering and removal, decoder state
retained across streamed base64/UTF-16 chunks, non-panicking TLS ticket-key RNG
with `sessionTimeout` expiry enforced, zero-byte writes reported as `WriteZero`,
port-aware TLS cache invalidation, and locale-independent HTTP-date validation.

`scripts/unrooted_local_shape.py` also got stricter. `RuntimeHandleScope` was a
blanket `ROOTED_MARKER`, so *any* function merely mentioning it was exempt from
the whole analysis. It is now tracked as a lexically-scoped exemption that
expires when its block closes, and the self-test pins both directions: a new
required probe (`planted_after_runtime_scope` — a stale pointer used after the
scope block ends, which the blanket exemption structurally could not catch) and
a new clean control (`clean_active_runtime_scope`). The event-helper roots the
stricter analysis exposed are fixed in the same change.
