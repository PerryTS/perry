### Added

- **The runtime-side GC-pointer holders are enumerated, and the enumeration is a
  gate (#7231).** A `thread_local!` or `static` in `perry-runtime`/`perry-stdlib`
  that stores a pointer into the GC heap *is* a GC root, and nothing static could
  find the class: `scripts/gc_root_dominance_check.py` reads emitted LLVM IR, and a
  runtime table is not in it. #7226, #7239, #7268 and #7274 were each found by hand,
  and each re-derived the same sweep.

  `scripts/gc_runtime_root_holders.py` (run from `lint`, already a required context)
  enumerates every static-shaped declaration whose type can hold a GC pointer —
  rule A on heap-header/`JSValue` types, rule B on an integer/`f64` cell that some
  function in its own file both names and allocates in, which is the only way to
  catch `CACHED_ENV: Cell<f64>`. It then *computes* coverage rather than trusting
  names: registered scanners are read from every `gc_register_*root_scanner*` call
  site, a call graph is walked from them, and a holder counts as covered when its
  identifier appears in a reachable function **defined in the same file**. The graph
  walk finds holders reached through an accessor (`cp_live_lock()`,
  `get_closure_props()`); the same-file rule stops `REGISTRY`/`SLOTS`/`ROOTS`/
  `STATES`/`CACHED` — each of which names several different holders here — from
  certifying the wrong one. Everything left needs a written verdict in
  `scripts/gc_runtime_root_holders.json`; an unclassified holder fails, and so does
  an entry that no longer matches, so a fix must delete its own exemption.

  Current state: 81 holders, 47 reached by a registered scanner, 30 classified (11
  `covered_elsewhere` with the covering scanner named, 15 `not_a_gc_pointer`, 1
  `test_only`, 1 `unverified`, and two previously untracked `open_gap`s —
  `json/mod.rs`'s `PARSE_KEY_RING`, an unrewritten mirror of the rooted
  `PARSE_KEY_CACHE`, and `perf_hooks.rs`'s `PERF_ENTRY_KEYS_ARRAY`, a nursery
  `keys_array` address compared by identity and never rewritten).

  Three bugs were found while building it, all in the checker and all of the "green
  because it matched nothing" shape: unstripped string literals made brace counting
  swallow `scan_raw_json_key_root_mut`, so `RAW_JSON_KEY` — visited three lines below
  its own declaration — read as uncovered; the registration regex captured only the
  first argument, so `gc_register_mutable_root_scanner_named("…", scanner)`
  registered nothing and six `worker_threads` holders read as uncovered; and rule B
  keyed on the file rather than the function and reported 544 holders, four fifths of
  them counters.

  `--self-test` plants a covered holder, one reachable only through an accessor, one
  uncovered per rule, and a same-named decoy in another file, then asserts the
  verdict machinery itself can go red (empty inventory, entry matching nothing, entry
  for a covered holder). Live sabotage: planting an unrooted `Cell<*mut ObjectHeader>`
  into `regex.rs` fails the real scan. The docstring names what the gate cannot see —
  `RuntimeState`'s fields, integer holders in non-allocating files, cross-file
  scanners, and whether a "covered" holder is covered *correctly*.
