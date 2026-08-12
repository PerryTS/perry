### Fixed

**`[...iterable]` / `Array.from` could read a from-space object after a copying minor (#7475).**

`js_iterator_to_array` — the drain behind array spread and `Array.from` — held
five live GC values in bare Rust locals across a `.next()` call that allocates
the `{ value, done }` result: the iterator object, the accumulator array, the
`next` closure, and the two property keys. One level down, `make_iter_result`
held the caller's element value and its own freshly allocated result object
across four more allocations before storing them, and
`dispatch_array_iterator_method` re-used a backing-array pointer read *before*
its cursor store, which can allocate.

Any of those allocations can trigger the copying minor. It moves the values and
rewrites only the slots it can see; a bare Rust local is not one. A moved
iterator leaves its pre-move copy in retired from-space, and the next dispatch
reads THAT copy's field 0 — so `dispatch_array_iterator_method` called
`js_array_length` on a from-space address, surfacing as
`TypeError: next is not a function`.

Everything is now rooted in a `RuntimeHandleScope` and every address is re-read
at its point of use. The per-iteration result object gets one reusable scratch
slot rather than a fresh handle per turn, since the loop runs up to 100k times.
All handles are NaN-boxed rather than `root_raw_*_ptr`, so
`scripts/raw_handle_debt.py` is unchanged at 999.

**Found through the auto-optimize link, but present in both.** The
`benchmarks/app-patterns` kernel `object_deep_clone` threw under the default
`perry file.ts -o out` and printed the right checksum under
`PERRY_NO_AUTO_OPTIMIZE=1`. That was an exposure difference, not a second bug:
`PERRY_GC_PROTECT_FROMSPACE=1` faults on both binaries at the same retiring
minor. Rebuilding the runtime archive one axis at a time showed the auto-opt
RUSTFLAGS (`-C panic=abort`) are irrelevant and the stripped feature set only
changes allocation timing enough to make the stale read observable. So this was
a latent correctness bug for **every** user of array spread / `Array.from`, not
an auto-optimize-only one — the auto-optimize link was the trigger that made it
visible.

### Added

**`auto-opt-app-patterns` gate.** Every other gate in the repo sets
`PERRY_NO_AUTO_OPTIMIZE=1` for a deterministic link, so the default path — which
rebuilds perry-runtime/perry-stdlib with a per-app Cargo feature set into
`target/perry-auto-<hash>/` and links those over `PERRY_RUNTIME_DIR` — was
covered by nothing. `scripts/auto_opt_app_patterns.sh` compiles the app-pattern
kernels through it and diffs each against the pinned Node oracle.

It asserts its subject was live rather than assuming it: the linker command line
(from `perry -v`) must name a `perry-auto-*/…/libperry_runtime.a` that exists on
disk. The auto-optimizer falls back to the prebuilt archives by design when its
cargo rebuild fails, and such a run would pass every output comparison while
exercising the wrong binary. Its one skip (`promise_all_chains`, a separate
promise-rejection defect tracked as #7497) carries a reason, and a skip entry
matching no kernel fails the script. The remaining eleven kernels pass. A
`--self-test` mode proves the liveness matcher can still fail; it caught a real
over-match while the gate was being written.

Not yet in branch protection's required contexts — a new gate has never been
green; promote after its first green run on `main`.

**`test-files/test_gap_gc_iterator_drain_rooting.ts`**, registered in
`test-parity/gc_repsel_corpus.txt` so `gc-moving-witnesses` runs it and refuses
a cell in which nothing moved.
