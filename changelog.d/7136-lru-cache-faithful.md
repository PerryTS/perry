### Fixed

- **`lru-cache` native binding (`perry-ext-lru-cache`) is now faithful to the
  npm `lru-cache` API for real-world usage.** The previous wrapper only handled
  numeric (`f64`) keys and values with no TTL, so a cache keyed on strings with
  object/string values (e.g. Socket Firewall's `new LRUCache({ max, ttl,
  updateAgeOnGet })`) silently misbehaved. Two defects are fixed:

  - **Keys and values are treated as real JS values, not raw `f64` bit
    patterns.** String keys now hash and compare by **content** (the NaN-boxed
    `StringHeader` is materialized via `js_get_string_pointer_unified` and keyed
    on its bytes), so a `get("k")` after `set("k", …)` hits even when the two
    `"k"` strings are distinct allocations or an SSO short string vs a heap
    string. Number/boolean/null/undefined keys key by canonical value
    (SameValueZero: `+0`/`-0` and all `NaN`s unified).
  - **Cached heap values are GC roots for as long as they are cached.** A
    mutable root scanner (`gc_register_mutable_root_scanner_named`) visits every
    cached value slot each GC cycle, so stored objects/strings are marked and
    rewritten to their forwarded address under copying evacuation — fixing a
    use-after-free where a cached value was collected out from under the cache
    (the "value is not a function" class of bug).

- **Constructor honors the options object.** `new LRUCache({ max, ttl,
  updateAgeOnGet })` is parsed by the runtime from the NaN-boxed options object
  (codegen now forwards the whole object instead of statically extracting only
  `max`, so dynamic/variable options work). `ttl` gives per-entry expiry on the
  `performance.now()` clock (`get`/`has`/`peek` treat an expired entry as
  absent; `get` evicts it); `updateAgeOnGet` resets an entry's TTL clock on a
  live `get`. `peek` is now wired into method dispatch.

  Not yet implemented (unchanged ABI carries only `(key, value)`):
  `maxSize`/`sizeCalculation`, `dispose`/`disposeAfter`, `fetch`, `allowStale`,
  per-call option objects, and the iterator surface. Object-identity keys are
  supported by pointer identity but are not tracked across a GC relocation;
  primitive keys are the GC-safe path.

  Tracking: #466 (Phase 5 native bindings). PR #7136.
