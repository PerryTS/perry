fix(gc): extend the root-holder census across perry-ffi/ext/ui, and fix the five unrooted holders it exposed

#8211 made `scripts/gc_runtime_root_holders.py` parse what it was already
pointed at. This PR closes the census's remaining WHOLE-CRATE blind spot:
its crate list was `perry-runtime` + `perry-stdlib`, while every
`perry-ext-*` crate parks user closures in handle side tables as NaN-boxed
`i64`/`f64` — shapes its type-name/allocator-context rules could not match
even when pointed at them. `gc_root_dominance_check.py` reads emitted LLVM
IR and is structurally blind to Rust-side tables, so this census is the
ONLY detector for the class; a crate outside its scope is not less checked,
it is unchecked.

The census now has three glob-discovered crate tiers (a new crate is in
scope the day it is created):

* **core** (runtime/stdlib): rules A/B unchanged on top of #8211's parser,
  plus a multi-line declaration join — `static X: T` with the `=` on a later
  line was invisible to any single-line regex; it recovers four more real
  holders (`ACCESSOR_RECEIVER_OVERRIDE`, `PROCESS_FINALIZATION_BEFORE_EXIT_LISTENER`,
  `CURRENT_MICROTASK_VALUE`, `SET_INDEX`), now verdicted.
* **ffi-side, gated** (`perry-ffi` + every `perry-ext-*`): type-SHAPE rules,
  because JS values cross the C ABI as integers — value-position primitives
  in containers (`Vec<i64>` listeners, `HashMap<K, f64>` callback maps; map
  KEYS are exempt handle ids), recursive crate-local struct/enum resolution
  (enum struct-variants like `ClientClose { callback: i64 }` included),
  `dyn`-erased payloads, and bare scalars in closure-handling files.
  Uncovered holders need a written verdict, same as core.
* **frontier** (`perry-ui*`): 461 real unregistered callback tables across
  eight platform crates (three not buildable on the gate host), pinned BY
  IDENTITY as a ratchet — a new UI holder fails lint, a fixed one must
  delete its entry. Named debt, not verdicts; the green run prints them as
  UNVERIFIED.

Two coverage-soundness fixes keep the wider walk honest: registration
arguments seed the call graph as BARE PATHS only (harvesting every
identifier out of `SOURCE.as_ptr()`-style C-ABI calls seeds `as_ptr`/`len`/
`state` — names that ARE `fn`s in these crates — and falsely covers
holders), and deep-hop attribution is fenced to crates that register at
least one scanner (the platform-ported UI crates define whole files of
same-named functions, which read 27 frontier holders as covered by name
collision). `--self-test` plants every new shape — fn-body `OnceLock` i64
table, `lazy_static!` `static ref`, multi-line decl, struct/enum payload,
`dyn` payload, closure-context scalar positive AND negative, C-ABI
trampoline coverage, the fence case, and both frontier-ratchet red paths —
and requires detection.

The widened census found five real unrooted holders, each fixed with a
seeded fail-before/pass-after fixture (registered in
`test-parity/gc_repsel_corpus.txt`; all four byte-match Node under
`PERRY_GC_SCHEDULE_SEED=7` with the fromspace-protect instrument armed and
1697–2543 copying minors per run):

* `perry-ext-http` `H2_PENDING_EVENTS`: `session.close/settings/ping(cb)`
  park the callback as raw NaN-box bits across a pump tick (the
  settings/ping callbacks have NO other holder). Pre-fix repro is a literal
  `TypeError: value is not a function`. Fixed with a scanner over the queue
  PLUS a drain custody chain — the drain used to snapshot into a bare local
  `Vec`, unrooting every not-yet-fired callback again while earlier events
  ran JS, so drained events now sit in a scanned thread-local and each arm
  parks its callback in a scanned stack across its allocating prep, reading
  the possibly-rewritten value back at call time (also un-breaks the
  stale retain-by-value removal in `close_callbacks`).
* `perry-ext-net` `once_flags()`: once-listener membership keyed by the
  closure's ADDRESS BITS; evacuation rewrote the scanned `listeners()` copy
  but a `HashSet` element cannot be rewritten in place, so once-listeners
  fired forever after a move (fixture: pre-fix `once fired: 2 of 2`,
  post-fix `1 of 2`). `scan_net_roots` now drains/forwards/reinserts the
  set in the same pass.
* `perry-ext-fetch` `REQUEST_HANDLES[*].signal`: a NaN-boxed AbortSignal
  whose only holder is the table, in a crate that registered no scanner at
  all. New `gc.rs` scanner (via `perry_ffi`'s named wrapper, i.e. the same
  C-ABI route as every ext provider), armed at `store_request`.

Because that scanner takes `REQUEST_HANDLES` during a collection on the
mutator thread, every reader holding the guard across a GC allocation
became a self-deadlock (and under panic=abort + invoke-EH an unwind does
not run `Drop`, so the failure is a permanent hang, not a panic). All
ext-fetch sites are hoisted (clone out, drop the guard, then allocate) and
two unit tests keep the invariant the same way #8211's stdlib tests do: a
`try_lock` probe after every reader — a re-introduced held guard FAILS
instead of hanging — and a source scan for allocation-through-a-live-borrow
that asserts against its own planted sample. The scan promptly caught three
MORE sites beyond the request getters (`js_fetch_response_status_text/type/url`,
borrowing `FETCH_RESPONSES`): latent rather than live, since no scanner
takes that lock today, but hoisted anyway so the invariant is file-wide and
any future response scanner is born safe — which is why the test exists
rather than a one-off fix.

Inventory: 13 new verdicts on top of #8211's 51 (the eight ext queue/
registry tables — all handle-id or Rust-owned payloads, with the JS values
living in already-scanned holders — plus backoff's `NEXT_ID`, ext-fetch's
`REQUEST_HANDLES` now covered from its `gc.rs`, and the three multi-line
core holders above), and the `frontier` list. Census totals on this tree:
621 declarations scanned across the three tiers, 92 reached by a
registered scanner, 64 classified, 461 ratcheted.

Known pre-existing failure, NOT absorbed here: `perry-ext-net --lib`'s
`gc_mutable_scanner_rewrites_listener_roots` fails on the current base even
with origin/main's `lib.rs` swapped into the same tree (A/B verified);
#8204 touched zero perry-ext-net files and no workflow runs that suite, so
nothing ever watched it. The once-flags fixture above covers this PR's
`scan_net_roots` change independently.
