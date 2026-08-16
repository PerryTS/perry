### Fixed — forced-evacuation App Route arm: two holders outside the GC heap (#8163)

The production Next 16.3.0 App Route fixture's forced-evacuation arm died with
`TypeError: value is not a function` around copying minor #238. Both holders were
Rust tables outside the GC heap that no root scanner marked or rewrote — the class
`PERRY_GC_VERIFY_EVACUATION` cannot see (no scanner to verify) and
`PERRY_GC_PROTECT_FROMSPACE_HOLDERS` cannot see (not in the heap). Found by running
the host under `PERRY_GC_PROTECT_FROMSPACE=1 …_DEPTH=800` and instrumenting the two
suspects until each printed the exact address the fault reporter named.

* **`HEADERS_METHOD_VALUE_CACHE`** (perry-stdlib `fetch`) — the bound-method closure
  behind `headers.get` / `.entries` / …, cached per `(handle, method)` in a
  `lazy_static!` `HashMap<_, u64>`. Its only "rooting" was
  `js_write_barrier_root_nanbox`, which is the incremental-marking shade, not a root.
  Next's route reads `(await headers()).get(...)` twice per request with awaits
  between; the closure allocated by the first read was retired by a later minor and
  the second read handed the pre-move address to `typeof` inside Next's
  `ReflectAdapter.get`. `FORM_DATA_METHOD_VALUE_CACHE` and `RequestRecord::signal`
  (the `AbortSignal` behind `request.signal`) had the same shape. A new
  `fetch/gc.rs` registers a scanner through the C ABI (like `streams/gc.rs`, so a
  trimmed stdlib provider installs it in the runtime image) that marks and rewrites
  all three; `js_request_new` now builds its record before taking the registry lock,
  since the default-signal allocation could otherwise collect under the guard the
  scanner needs.
* **`ServerResponse.once_listeners`** (perry-ext-http) — `res.once(event, cb)` stores
  into a second table that `take_event_listeners` merges into every emit, and
  `scan_http_server_roots` visited only `listeners`. Next's `pipeToNodeResponse`
  registers `res.once('close', …)`; the closure was retired at #211 and reached
  `emit_no_arg_to_listeners` from the `res.end()` tail as a from-space address. The
  scanner now visits it. While there, the `res.end()` tail (`js_node_http_res_end`,
  `_full`, `_with_cb`, `standalone_end`) took listener/callback snapshots OUT of the
  handle and then ran JS before using them — rooting inside
  `emit_no_arg_to_listeners` roots what it is *handed*, so a stale snapshot stayed
  stale. `EndTail` (new `server/response_end.rs`, split out because `response.rs`
  sat at the 2,000-line cap) parks every snapshot in the transient-root stack before
  the first JS call. `js_node_http_im_resume`'s `'end'` snapshot and
  `js_node_https_server_close`'s callback get the same treatment.

**Registering a scanner over a table makes every "allocate under its guard" site a
deadlock, so those were hoisted in the same change.** `std::sync::Mutex` is not
reentrant: the scanner takes `REQUEST_REGISTRY` during a collection on the mutator
thread, so any site holding that guard across a GC allocation self-deadlocks the
first time the allocation collects. Twelve string arms in `dispatch_request_property`
plus `js_request_get_url` / `_method` / `_body`, `js_request_input_to_url` and
`request_string_field` now snapshot the field bytes under the guard and allocate
after dropping it. Worse, `js_request_clone` **threw** under the guard — the
exception transport unwinds through the frame without running `Drop` (it is written
for `panic=abort`), so the registry mutex would stay locked for the life of the
process; it now decides "unusable" under the guard and throws outside it. Two tests
hold this line, because the failure mode is a hang and a test that hangs is worse
than one that fails: `request_reads_release_the_registry_guard` `try_lock`s after
every reader, and `no_allocation_is_taken_off_a_live_registry_borrow` scans for the
`js_string_from_bytes(req.…)` shape (with a planted sample proving the scan can
still match it). Found in review by a parallel session — credit to them; the
contract that caught it is the one this module's own docs state.

**Why the audit missed it.** `scripts/gc_runtime_root_holders.py` could not see any
of this: its `DECL` regex did not match `lazy_static!`'s `static ref`, so 93 tables
across runtime+stdlib were outside the census; `strip_comments` blanks string
literals so `extern "C" fn` reached `FN_DEF` as `extern "" fn` and no C-ABI function
had a body in the walk (the FFI scanner trampolines included); a body-less
`fn f(...);` inside an `extern "C" {}` block started a brace count that swallowed
the following functions; and the registration regex stopped at the first `(` of
`SOURCE.as_ptr()`, dropping the scanner name from every C-ABI registration. All four
are fixed. The census grows from 78 to 129 declarations (74 reached by a scanner,
was 53) — the gate was green because the narrow regex hid the candidates, not
because they were classified. The 31 newly visible uncovered holders each carry a
written verdict in `gc_runtime_root_holders.json` naming the mechanism (the
address-keyed `REGEX_POINTERS` / `REGEX_SOURCE_TABLE` are rekeyed on move and
pruned on death by `regex_header_moved_for_gc` / `_clear_dead_for_gc`, dispatched
from `gc/types.rs`; `THREAD_GLOBAL_THIS` / `THREAD_MODULE_TOP_THIS` register their
own cell address with `js_gc_register_global_root`; `DIAG_STORE_SCOPES` cannot hold
a heap pointer because `store_handle` rejects one), and one stale entry
(`EXT_BLOCKING_TASKS_INFLIGHT`, now reached through the newly visible C-ABI scanner
chain) is deleted.

**Validation.** Same app dylib, same seed (8036), providers from the same tree:
main → 59 `TypeError`s / 0 `PASS`, from-space fault at minor #218; fixed → 2×
`PASS: 21`, 446 copying minors, 0 errors, and clean under
`PERRY_GC_PROTECT_FROMSPACE=1 …_DEPTH=800` on seeds 8036, 8174, 1, 8040, 4242. The
fixture's forced arm (`PERRY_NEXT_ROUTE_FORCED_GC`) is back ON by default. Unit
tests: the fetch scanner emits and rewrites all three slots; the ext-http rewrite
test gains `once_listeners` and `pending_write_callbacks`.

Follow-up (not fixed here): perry-ext-net's `once_flags` keys `HashSet<i64>` by
closure ADDRESS to decide which listener to drop after a `once` fires — a rekeyed
table of the #8174 family (a moved closure fires twice, a recycled address drops the
wrong listener); and the ext crates' handle-struct fields are still outside any
census (`once_listeners` was one).
