### Performance: the object property-MISS path no longer allocates for absent rare-subclass features

**`asyncpipe` 0.722 s → 0.132 s (−81.7%)** — quiet M1 mini, best-of-5, output
verified byte-identical to node and exit code checked before timing. That moves
an async service pipeline from **9.69× node to 1.76×**, and from 1.92× scriptc
to **0.35×**. The rest of the 19-benchmark corpus moves within ±1.2%
(measurement noise): `churn` 0.424→0.420, `interp` 1.888→1.902,
`pipeline` 0.540→0.540, `tree` 1.634→1.645, `fib40` 0.394→0.393.

`await`ing a plain object is one of the most common things async TypeScript
does, and it was the slowest thing Perry did. The spec requires a thenable
check on every promise resolution — `Get(resolution, "then")` — so every
`return <plain object>` from an `async function` performed a property lookup
that MISSES. That miss path turned out to be a cascade of un-gated
"is this receiver a rare exotic?" probes, and **four of them cost a key-string
allocation plus a full recursive `js_object_get_field_by_name` each, per miss**:

| probe | hidden field it reads |
|---|---|
| `promise::subclass::subclass_backing_promise` | `__perry_promise_backing__` |
| `object::fetch_subclass_handle_id` | `__perry_fetch_handle__` |
| `object::temporal_subclass_cell` | `__perry_temporal_cell__` |
| `object::map_set_subclass::subclass_backing_of` | `__perry_collection_backing__` |

Each answers "is this a `class X extends Promise / Request / Temporal.* /
Map | Set` instance?" — virtually always *no*, and knowable without touching the
object at all. Each hidden field has exactly **one** writer, so a monotone
process-wide "has one of these ever been created?" flag is an exact answer.
`FETCH_SUBCLASS_EVER` already existed for the `in`-operator fast path (#6748)
but was never consulted here; the other three flags are new and are armed at
their single stash site, *before* the field is written, so no reader can
observe a stashed field while the flag still reads "never".

The same miss path also re-resolved the default `Object.prototype` on every
call — `globalThis.Object` (which interns an `"Object"` key string) plus
`closure_get_dynamic_prop("prototype")`. It now reads the memoized, GC-healed
`object_prototype_addr()` cache that the array index fast path already relies
on; it is a registered GC root (`scan_prototype_addr_cache_roots_mut`), and
`Object.prototype` is non-writable/non-configurable per spec so the memo cannot
go stale. The recursive prototype read itself, and the rooting that protects it,
are unchanged.

Nothing about property semantics changes: a user-installed
`Object.prototype.then` still makes plain objects thenable, `Object.prototype`
accessors still run, and builtin members still read as functions.

**Coverage.** Nothing in the tree used `class X extends Promise` before this
change, so the promise-subclass probe had no test at all and a wrong gate would
have broken it silently. `test-files/test_gap_7795_promise_subclass_probe_gate.ts`
exercises the gate's OPEN state (a subclass instance exists, so the probe must
still find its backing cell) alongside the plain-object fast path;
`test-files/test_gap_7795_object_prototype_miss_path.ts` pins the
`Object.prototype` semantics above. The Map/Set, fetch and Temporal gates are
covered by the existing `#6325` / `#7570` / `#7575`, fetch-subclass and
`#5587` suites.

**This masks a pre-existing GC bug — see #7794.** `asyncpipe` exits 138 under
`PERRY_GC_PROTECT_FROMSPACE=1` on `main`, faulting on a retired from-space
`GC_TYPE_PROMISE`. Removing ~4 key-string allocations per thenable check
removes most of the collection opportunities in that window, so the program
stops faulting under default GC pacing after this change. It is **not** fixed:
with this change applied, `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`
reproduces the identical fault. #7794 has the root cause
(`async_step_fulfill_thunk` holds two bare `*mut Promise` locals across the
step-body call) and the symbolicated backtrace.
