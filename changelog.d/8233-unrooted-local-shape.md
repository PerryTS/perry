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

Current surface: **218 findings across 51 files**, led by `mysql2/result.rs` (27),
`crypto/sign.rs` (16), `sqlite/better.rs` (12), `events/events_on.rs` (11). This is an
*exposure surface*, not a bug count — most sites allocate and return without holding
anything across a second collection point.

It ships as a ratchet against a recorded baseline rather than a zero target, because
per CLAUDE.md a new gate has never been green. It runs in the `lint` job with no
`continue-on-error`. `lint` is not itself a required context — `pr-gate` is the only
one — but the `gate` fan-in lists `lint` among its `needs` and fails on any
`failure`/`cancelled` result, and `scripts/ci_plan.py` enables `lint` in all three
tiers (`pr`, `sweep`, `full`), including the docs-only PR case it self-tests. So the
path from a red step here to a blocked merge is unbroken.

The detector is validated against independent ground truth rather than only its own
fixtures: it reproduces both sites named in #8233, including
`events/events_on.rs:40`, where `state` is bound at :37 and can move at :38, and the
push also stores a stale `buffer` **into the heap** — damage that outlives the frame.
`--self-test` plants a stale-local site and asserts it is flagged while an
allocation-free function is not, so a detector that silently stops working fails
loudly.

Refs #8233.
