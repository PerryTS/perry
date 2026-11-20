perf(runtime): implicit `ToPrimitive` on objects and Dates is much cheaper (#10510).
`(a < b) + (a - b)` on objects with a prototype `valueOf` is ~4× faster
(15.3 s → ~3.9 s per 1M iterations on the issue's benchmark, now cheaper than
the explicit `.valueOf()` calls), and `+date` / `date - date` are ~15–20%
faster.

- The #5437 `_req` symbol fallback (`symbol/get.rs`) ran on every symbol read
  that missed the receiver's own table, interning `"_req"` and doing a by-name
  prototype-chain `[[Get]]` each time. It can only ever return what a small
  native handle holds in the symbol side tables, so it is now latched off
  until such an install has happened (`SMALL_HANDLE_SYMBOL_OWNER_EVER`, set
  in both install funnels before the insert).
- `well_known_symbol` has a lock-free fast path for the names the runtime
  resolves on hot paths (it took a mutex and hashed a `String` every call).
- `Date.prototype[Symbol.toPrimitive]` and `Date.prototype.valueOf` are
  dispatched directly when the resolved method is the builtin (compared by
  code address), so overrides are still observed.
- `OrdinaryToPrimitive` looks `valueOf`/`toString` up with interned keys
  instead of allocating a string per conversion.

Remaining Date cost is resolving `Date.prototype` via globalThis twice per
conversion; the issue's Date target is not yet met.
