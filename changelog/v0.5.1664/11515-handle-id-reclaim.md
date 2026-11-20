**Long-running servers no longer die with `native handle registration exhausted: IdExhausted` (#11453).** A mixed http + fetch + crypto + zlib + timers backend panicked after about 262k requests in one process.

Root cause: a true leak, not a reuse policy. Instrumenting the shared id pool per payload type showed that each request registered one `ServerResponse` and one `IncomingMessage`, both retired and recycled by perry-ffi's tick quarantine, and one `crypto.createHash` **`HashHandle`, which nothing ever dropped**. The stdlib common registry had no path that released a digest, HMAC, cipher, sign/verify or `StringDecoder`. Their only owners are JS values, so they kept both their payload and their id in the shared 262k band forever. Explicitly dropped common ids were also tombstoned forever since #11225.

Fix:

- Common ids are now owned weakly by the collector. `register_reclaimable_handle` parks an id, and a full heap trace that finds no word naming it (heap slot, stack root, handle scope or registered native root) drops the payload and returns the id to the pool. Explicitly dropped ids are handled the same way instead of being tombstoned.
- Stale-id safety holds by construction: an id is reissued only after the collector has proved that no JS value still holds it. Ids stay inside the existing `[1, 0x40000)` band, so NaN-box handle classification is unchanged.
- The runtime exposes a second weak-owner provider, `perry_ffi_gc_register_pool_handle_trace`, beside the Fetch band's. It is armed only while something is parked.
- Pacing: the collector's own full traces decide parked ids at no extra cost. A trace is *requested* only when the parked count reaches its trigger *and* the mutator has run 32 times as long as the previous requested trace took. 64k parked ids or low band headroom override the time budget, so memory stays bounded. Cost: +2.6% instructions on the #11451 backend probe, +9.8% on a loop that does nothing but `createHash`; −43% server RSS.
- A hash or HMAC used as a stream (`on`/`pipe`/`write`/`end`) becomes strongly owned, because its listeners and queued events are keyed by id. The receiver of every common method dispatch is rooted for the duration of the call.
- The shared pool's freelist is now FIFO (it was LIFO), so a recycled perry-ffi id waits behind the whole free population.
- `finish_retirement_reusable` and `available_ids` are new on the pool.
- True exhaustion is now a catchable `Error` with code `ERR_PERRY_HANDLE_IDS_EXHAUSTED` from both registries, not a process abort.
