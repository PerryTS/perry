### Changed

- Appending to and popping from a `class X extends Array` instance no longer re-classifies its own elements store: an in-capacity append and a non-hole tail pop run without a handle scope, a proxy probe, forwarding-stub cleaning or flag resolution, keeping only the element bookkeeping the read tiers and the loop guard consume. Growth, holes, an empty store and every exotic flag keep the complete runtime entry.
- `sub.pop()` on such an instance now pops inline as well: the codegen tier resolves the payload through the meta record and runs the same length/read/take blocks on it, instead of calling the runtime entry that only re-derives the store.
