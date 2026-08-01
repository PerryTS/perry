### Fixed

- **`lru-cache` native binding (`perry-ext-lru-cache`) is now faithful to the
  npm `lru-cache` API for real-world usage.** The previous wrapper only handled
  numeric (`f64`) keys and values with no TTL, so a cache keyed on strings with
  object/string values (e.g. a typical caller's `new LRUCache({ max, ttl,
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

- **Constructor options are validated exactly as npm validates them, so a
  bad `max` throws instead of aborting the process.** `option_number`
  accepted any finite number and `n as usize` saturated, handing the backing
  map a capacity request it could not satisfy: `new LRUCache({ max: 1e12 })`
  reserved a 10^12-bucket table and killed the process with no JS-visible
  error. Rather than invent a bound, every case was measured against
  `lru-cache@11.5.2` on the pinned oracle (Node 26.5.1) and reproduced
  message for message:

  | `new LRUCache(…)` | throws |
  |---|---|
  | `()` | `TypeError: Cannot read properties of undefined (reading 'max')` |
  | `(null)` | `TypeError: Cannot read properties of null (reading 'max')` |
  | `(5)`, `("x")`, `({})`, `({ max: 0 })`, `({ max: -0 })` | `TypeError: At least one of max, maxSize, or ttl is required` |
  | `({ max: -1 \| 1.5 \| Infinity \| NaN \| "3" \| true \| null })` | `TypeError: max option must be a nonnegative integer` |
  | `({ max: 2**32 })` … `({ max: MAX_SAFE_INTEGER })` | `RangeError: Invalid array length` |
  | `({ max: 2**53 })`, `({ max: 1e300 })` | `Error: invalid max value: <n>` |
  | `({ max: 3, ttl: -5 \| 1.5 \| Infinity \| "5" })` | `TypeError: ttl must be a positive integer if specified` |

  The two upper bounds are npm's own: it builds its index arrays with
  `Array.from({ length: max })` (past the JS array-length limit that is a
  `RangeError`) after a `getUintArray(max)` lookup that returns `null` past
  `Number.MAX_SAFE_INTEGER` (a plain `Error`). **This is a behavior change
  for `new LRUCache()` and `new LRUCache(100)`**, which previously fell
  through to a silent `max=100`; npm throws for both, and so does Perry now.
  `max: 0` together with a `ttl` is npm's legal unbounded cache and is
  supported.

  Where Perry deliberately differs: the backing map grows lazily instead of
  reserving `max` buckets up front, so a large-but-legal `max` (`1e8`, say)
  constructs instantly here where npm OOMs Node. Nothing observes that
  except by not running out of memory.

  Not yet implemented (unchanged ABI carries only `(key, value)`):
  `maxSize`/`sizeCalculation`, `dispose`/`disposeAfter`, `fetch`, `allowStale`,
  per-call option objects, and the iterator surface. Because `maxSize` is
  unimplemented it also does not satisfy npm's "at least one of max, maxSize,
  or ttl" requirement — a `maxSize`-only cache constructs on npm but throws
  here, which fails loudly instead of yielding a silently unbounded cache.
  npm's `UnboundedCacheWarning` for `ttl`-only caches is not emitted. Object-
  identity keys are supported by pointer identity but are not tracked across a
  GC relocation; primitive keys are the GC-safe path.

  Tracking: #466 (Phase 5 native bindings). PR #7136.
