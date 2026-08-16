**Proxy: the 65,536th `new Proxy()` in a thread no longer returns a wild pointer.**

`js_proxy_new` took its id straight from `PROXIES.len()` with no bound check on the band it encodes into. The revocable-Proxy band is `[PROXY_ID_BAND_START, HANDLE_BAND_MAX)` = `[0xF0000, 0x100000)` — exactly 65,536 ids — so id 65,536 encoded to `HANDLE_BAND_MAX` itself: a payload `addr_class::is_proxy_id_band` rejects and `addr_class::is_above_handle_band` **accepts**, i.e. one every classifier in the tree reads as a dereferenceable heap address. The value came back to user code as a wild pointer and the next property read on it segfaulted (65,600 proxies created then read back: SIGSEGV, exit 139, at exactly the first out-of-band id).

This is reachable by a server that merely stays up. `PROXIES` is append-only — `js_proxy_new` pushes and the only `slot.take()` is `#[cfg(test)]` — so the count is cumulative over process lifetime, not a live-proxy count. #8213 measured a warm Next.js App Route creating ~4 proxies per request (10,037 entries after 2,478 requests), which puts the ceiling at roughly 16k requests.

Every other handle band already refuses to allocate past its end (`common/handle.rs`, `fetch/mod.rs`). The Proxy band was the one without a guard, and the only one whose ids are minted directly by user code with no matching free/close call.

- `reserve_proxy_id(len)` returns `None` at the band edge, and `js_proxy_new` throws a catchable `RangeError` instead of minting — the same trade `error::throw_allocation_failed` makes for #5067. The reservation happens **before** the registry borrow is taken: the throw allocates a JS error, which can collect, and `scan_proxy_roots_mut` borrows `PROXIES` mutably, so throwing under an open borrow would panic the collector (and a caught throw would leave the registry borrowed for the life of the thread).
- `decode_proxy_id` now rejects payloads at or past the band end, so it and `addr_class::is_proxy_id_band` agree about what a proxy id is. They did not before — `lookup` accepted anything below 4 GiB, so an out-of-band id was simultaneously a live proxy there and a heap address everywhere else.

Four sabotage-verified tests: all 65,536 reservable ids encode in-band while the first refused one is `is_above_handle_band` (which is *why* refusing it is a memory-safety fix, with `PROXY_ID_BAND_LEN == 0x10000` pinned as a vacuity guard); the decoder's boundary; the live `js_proxy_new` path against a test-only shrunk band; and a subprocess witness asserting the child exits `Some(1)` with a `RangeError` — a signal death has no exit code at all, which is exactly the before/after difference.

This makes running out of the band safe and named; it does not remove the ceiling. The ceiling exists because the registry never reclaims a slot (#8213 mechanism (b)), which needs the collector to observe proxy-band payloads in traced slots — a proxy handle is not a heap object, so there is no death signal today. Raising the band instead is not a cheap alternative: it is boxed in by zlib below and by `HANDLE_BAND_MAX` above, whose own doc requires auditing every `is_handle_band` caller before it moves.

Closes #8218. Refs #8213.
