### Fixed

- **GC: the lazy `globalThis` bootstrap now runs in a no-move window (#7217).**
  `test_gap_gc_spread_accessor_rooting` — #7207's reproducer for #7200 —
  SIGSEGV'd 10/10 deterministically under
  `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off`
  (the allocation-point route) months after three separate rooting fixes had
  been verified green on the safepoint route. **The failing collection was not
  in the code any of those fixes touched.**

  `js_get_global_this()` builds the whole realm on first use, and it is reached
  *lazily* — in this program from `js_object_set_field_by_name` →
  `object_prototype_addr_matches` → `js_get_global_this_builtin_value`, i.e.
  from an ordinary property write several hundred loop iterations in, after
  ~8 MB of churn. The bootstrap then allocates ~1.15 MB of its own, so under an
  8 MB heap limit **minor #0 lands in the middle of it**. #6982 rooted the
  `globalThis` singleton — one pointer. The bootstrap builds a *graph*:
  `intl::install_constructor` threads `ctor`, `proto` and `ns_obj` as bare
  `*mut ObjectHeader` locals across dozens of allocating installs, and so do the
  error, typed-array, generator, Reflect, Atomics and WebAssembly installers,
  across a dozen files. Every one is a slot the collector does not rewrite.

  `PERRY_GC_PROTECT_FROMSPACE=1` (#7196) named it without inference:
  `set_builtin_property_attrs` ← `intl::install_function` ←
  `install_constructor` ← `install_intl_namespace` ←
  `populate_global_this_builtins` ← `js_get_global_this`, on an address with
  `retired_by_minor=#0`. Confirmed before any code changed: adding
  `const __warm = typeof (globalThis as any).Intl;` at the top of the
  *unmodified* reproducer — so the bootstrap runs while the arena is nearly
  empty — makes it clean 5/5 with 6 copying minors and 4 613–5 797 objects
  copied each.

  **Why the safepoint route could not see it, which is the general finding.** A
  loop back-edge poll fires only while user JS is running, and the bootstrap
  runs no user JS, so none of those locals is ever live across a collection
  there. On the allocation-point route the bootstrap's *own* allocations are the
  collection points, so the entire graph is exposed at once. The two routes are
  not two chances to catch the same bug: `loop_polls` cannot expose an unrooted
  local in any runtime code that does not re-enter user JS, which is most of the
  runtime.

  **The invariant: a bootstrap that builds an IMMORTAL object graph through raw
  pointers held across its own allocations must run in a NO-MOVE WINDOW.**
  Rooting each holder individually is unbounded (hundreds of sites) and
  ungateable — `scripts/gc_root_dominance_check.py` reads emitted LLVM IR and is
  structurally blind to all of them. The window is one line and provably enough,
  and it costs nothing a collection would have recovered: every object born in
  it is reachable from `globalThis` for the life of the thread.

  The fix is one line: `crate::gc::GcSuppressScope` (the existing nesting-safe
  RAII no-move window, already used by `descriptor_state.rs`) at the top of
  `populate_global_this_builtins`. `GC_FLAG_SUPPRESSED` gates
  `gc_check_trigger`, the budgeted stepper **and** `gc_safepoint_moving_minor`,
  so the window is comprehensive rather than allocation-point-only. No
  installer's rooting was touched — adding a `RuntimeHandleScope` to one of
  fifty installers would imply the other forty-nine are fine. No env knob is
  added and no collector behaviour changes anywhere else.

  Measured on the allocation-point arm, same host, idle, one target dir, 10 runs
  per cell. The base arm was produced by reverting the source change and
  rebuilding, and came back **bit-identical** to the pre-change build
  (`perry` md5 `6142b49f…` both times) against `068af604…` for the fix, so the
  two arms are demonstrably different binaries. Every row was then re-run
  against the final shipped tree (`f1f002f8…`, after the two #7251 windows were
  dropped) and is unchanged:

  | witness | base `8b024958f` | fixed |
  |---|---|---|
  | `test_gap_gc_spread_accessor_rooting` | **exit=139, no output, 10/10** | **`bad plain 0 hot 0 tail 0` 10/10** |
  | `test_gap_gc_static_block_this_rooting` | `bad 1` 10/10 | **`bad 0` 10/10** |
  | `test_gap_gc_inline_ctor_this_rooting` | green 10/10 | green 10/10 |

  `loop_polls` (compiled **and** run with `PERRY_GC_MOVING_LOOP_POLLS=1` plus
  `PERRY_GC_FORCE_EVACUATE=1`): all five `test_gap_gc_*_rooting` witnesses green
  5/5, and all five still relocate (1–5 cycles, 224–26 063 objects copied), so
  none went inert. Shipped default: clean 3/3, byte-exact against
  `node --experimental-strip-types`. `PERRY_GC_PROTECT_FROMSPACE=1` on the
  reproducer now reports **no fault at all**. The static
  `gc_root_dominance_check.py` reports 0 violations before *and* after, in both
  its default and `--unrooted-allocas` modes — it reads emitted LLVM IR, so it
  is structurally blind to a bug that lives in the runtime's Rust locals, which
  is worth recording as a limit of that gate rather than as a clean bill.

  **The window defers a collection, it does not add one.** On
  `console.log("hi", typeof globalThis.Intl)`, peak RSS drops from
  10 878 976 / 10 895 360 / 10 878 976 bytes to
  10 649 600 / 10 649 600 / 10 633 216, and GC cycles at `HEAP_LIMIT=8` go from
  2 to 1 — the collection that used to run mid-bootstrap copied the bootstrap's
  own live set and then had all of it survive anyway.

### Testing

- **`crates/perry-runtime/src/gc/tests/global_bootstrap.rs`** — a `--lib` unit
  test, so it runs in the per-PR `cargo-test` gate rather than in a
  nightly-only `tests/*.rs` suite. It arms **one** pending collection, runs
  the bootstrap, and asserts it was not serviced, that the request is
  **deferred rather than dropped**, that the window spans at least one arena
  block (so `arena_alloc_gc` genuinely reached `gc_check_trigger` inside it),
  and that the window closed. Each then runs **the control**: the *same* armed
  request, on the *same* thread, must be serviced by ordinary allocation once
  the window is over. Without that second half the test would pass on a tree
  where nothing was ever due — CLAUDE.md's fourth way a gate cannot fail.
  Sabotage-checked in the failing direction: removing the `GcSuppressScope`
  reddens it with `left: 1, right: 0` and the message naming the installer
  locals.

### Known issues

- Two of the five `test_gap_gc_*_rooting` witnesses remain red on the
  allocation-point route for **unrelated** reasons, and their twenty
  `test-parity/gc_repsel_triage.txt` entries are retargeted rather than deleted
  — one of the two triage texts was asserting a cause now known to be wrong.
  Both are green on `loop_polls`, which `gc-moving-witnesses.yml` gates.
  - **#7247** — `test_gap_gc_regexp_receiver_rooting`, unchanged (exit=139 10/10
    on both arms). `js_regexp_new` holds `string_as_str(pattern)` /
    `string_as_str(flags)` — `&str` borrows into a movable `StringHeader`
    payload — across its whole body. The #7215 borrow shape.
  - **#7248** — `test_gap_gc_assign_string_source_rooting`, improved from
    `bad char 3 count 3` to `bad char 1 count 1` (10/10 each). The residual
    failure is a stale `js_eq` left operand in the test's own
    `got !== ALPHA[i % 26]` assertion — a register loaded above the allocating
    `js_string_index_get_boxed` sibling and never re-read — not anything in
    `js_object_assign_one`. The #7206/#7214 operand family.
- **#7251** — the same defect shape exists in `ensure_generator_intrinsics` and
  `ensure_typed_array_intrinsic`, which build the same kind of immortal tower
  through the same kind of raw locals and are *also* reachable lazily ahead of
  the bootstrap. Windows for them were written and then **deliberately dropped
  from this PR**: a tower is three orders of magnitude smaller than the
  bootstrap, fits inside one arena block's tail, and so may reach no
  `gc_check_trigger` at all — three successive versions of a gate for them
  passed with the window deleted. Shipping a GC-trigger change with no test that
  can fail without it is the thing CLAUDE.md's knob-kill policy exists to stop,
  so the exposure is tracked instead, with the two candidate gate designs and an
  unexplained observation (something may already be suppressing across part of
  the tower build) written up in the issue.
- **#7154's `sfw-registry --help` symptom was not run** (the workload is not
  present on this machine) and is **not** claimed resolved. What is measured is
  that the five witnesses are green in the configuration a #7161 revert would
  ship.
