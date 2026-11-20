- **perf(runtime): the inherited-read cache now serves object literals and absent keys (#10495, #10497, #10753, #10877).** Before this, a read on a plain object (an object literal or JSON row) re-ran the whole generic prototype walk down to `Object.prototype` every time, about 7–11k instructions, whenever the key was absent or lived on `Object.prototype`. The cache only covered receivers with a recorded prototype or a synthetic class id.

  What changed:
  - The walk now follows the default `%Object.prototype%` link for receivers with no class of its own.
  - It records confirmed ABSENT entries. Any entry whose claim depends on the generic getter (through the default link, or for `constructor`) is written pending and commits only after the generic getter returns the same bits with no validity change in between.
  - By-name reads (`O[k]`, `for…in` + `obj[key]`) prime too, on a pair's second miss, and give up after a failed retry. The give-up avoids paying on transient keys or unprimeable pairs.
  - `MAX_HOPS` goes from 4 to 10.
  - The `Function.prototype` fallback in `closure_get_dynamic_prop` uses the canonical interned key instead of allocating one per read.

  Results (qb2, instructions per iteration vs `c1d93bb72`, output identical to Node 26.5.1):
  - lru-cache −42% / −35%, moment −51% / −29%, dayjs −26% / −21%, validator −24% ×2, jsonwebtoken/hs256 −22%, date-fns/format_add −18%, rate-limiter-flexible −13% / −10%, cron −5%.
  - #10877 misses at any depth up to 8 hops: 18.6k–60.8k → 323. #10753 absent `O[k]`: 4,968 → 690. #10497 `fn.isBuffer`: 22,971 → 4,346.
  - Residual regressions of +1.1% to +2.3% on declared-class instances whose prototype lives in the class-method table, and on string receivers. Both are out of scope.
  - Peak RSS +0.4 to +3.1 MB on the measured workloads.

  New gap test `test_gap_10495_inherited_absent_cache.ts`; 6 new cache unit tests. Two per-thread integer tables got verdicts in `gc_runtime_root_holders.json`. Found on the way: qs/parse_nested's stale from-space pointer in `js_dyn_index_set_strict` (#11550, pre-existing on main).
