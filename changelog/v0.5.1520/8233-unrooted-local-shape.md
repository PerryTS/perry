**GC tooling: an instrument for unrooted locals in `perry-stdlib` and `perry-ext-*`.**

`scripts/raw_handle_debt.py` counts bare reads *out of* a `RuntimeHandle` — debt in
code that already adopted the rooting API and then degraded a use. Code that never
roots at all has no `get_raw_*_ptr` to count and scores **zero**, the ratchet's best
possible result, and its scope is `perry-runtime` only. So a file holding four
unrooted heap pointers with no `RuntimeHandleScope` anywhere was indistinguishable
from a perfectly rooted one, and two whole crate families sat outside the
denominator.

`scripts/unrooted_local_shape.py` detects the shape instead: a local bound from an
allocator return, used again after an intervening call that can allocate or run JS.
Neither existing instrument can see this — `gc_root_dominance_check.py` reads emitted
LLVM IR and is structurally blind to Rust locals, and `gc_runtime_root_holders.py`
enumerates `static`/`thread_local!` declarations, not stack slots.

Current surface: **605 findings across 84 files**, led by `mysql2/result.rs` (43),
`webcrypto/keys.rs` (40), `streams.rs` (25), and the events extension (23). This is
an *exposure surface*, not a bug count — the line-order heuristic deliberately
over-approximates control flow and allocator behavior.

It ships as a ratchet against a recorded baseline rather than a zero target, because
per CLAUDE.md a new gate has never been green. It runs in the `lint` job with no
`continue-on-error`. `lint` is not itself a required context — `pr-gate` is the only
one — but the `gate` fan-in lists `lint` among its `needs` and fails on any
`failure`/`cancelled` result, and `scripts/ci_plan.py` enables `lint` in all three
tiers (`pr`, `sweep`, `full`), including the docs-only PR case it self-tests. So the
path from a red step here to a blocked merge is unbroken.

The detector reproduces the still-present `events/events_on.rs:40` ground-truth
site from #8233, where `state` is bound at :37 and can move at :38, and the push
also stores a stale `buffer` **into the heap** — damage that outlives the frame.
`--self-test` separately plants collecting-call, ordinary-return, and later-`let`
RHS uses, including a pointer extracted from a NaN-box. It also asserts that a
function with one allocation and no intervening collection point stays clean, and
that total, per-file, and schema baseline violations can turn the gate red.

Refs #8233.
