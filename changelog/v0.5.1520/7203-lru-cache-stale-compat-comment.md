### Documentation

- **Corrected the stale `lru-cache` compat comment in
  `well_known_bindings.toml`.** The row justified its `compat = "partial"`
  marker by claiming "the wrapper's store is numeric-value-oriented" — which
  stopped being true when #7136 landed. That PR gave the binding real JS keys
  and values, content-compared string keys, GC rooting for cached heap values,
  `ttl`, and `updateAgeOnGet`, so the comment left readers with a wrong mental
  model and an obvious-looking reason to flip the marker.

  Re-measured both directions against npm `lru-cache@11.5.2` on Node 26.5.0
  with the bundled binding linked in. A 20-assertion probe of real-world usage
  (string keys, key-compare-by-content, miss, `has`, object values by identity,
  mutation through the cached reference, overwrite, `size`, `delete`, `clear`,
  eviction at `max`, `get`-promotes / `peek`-does-not, numeric values, `ttl`,
  object survival across forced heap churn) prints **identical** output on both
  — the binding is genuinely faithful for the surface it implements.

  The marker nonetheless stays `partial`, and the comment now says why.
  `full` means an exhaustively audited drop-in for the package's *entire*
  public API, and `is_faithful()` gates exactly one decision: whether Perry may
  auto-prefer this wrapper over a user's installed `node_modules/lru-cache`.
  The wrapper exports 8 entry points and omits `maxSize`/`sizeCalculation`,
  `dispose`/`disposeAfter`, `fetch`/`forceFetch`, `allowStale`, per-call
  `set`/`get` option objects, and the iterator surface. Two of those gaps fail
  *silently* rather than loudly — measured, `forEach` visits nothing where npm
  visits every entry, and a `dispose` callback is never invoked where npm
  invokes it on eviction — so a `full` marker would let Perry swap a wrong
  implementation in for a correct installed one with no diagnostic. This makes
  `PERRY_REQUIRE_FAITHFUL_BINDINGS=1` refusing `lru-cache` the correct outcome,
  not a false positive.

  Added `lru_cache_stays_partial_until_the_silent_gaps_are_closed`
  (`crates/perry/src/commands/compile/well_known.rs`) so the next person
  tempted to promote the marker meets the reasoning and the evidence together
  instead of re-deriving them. No behavior change — the marker value is
  unchanged.
