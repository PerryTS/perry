`transition_fast_rejects_object_prototype_even_with_a_cached_edge` now
materializes the realm global itself instead of assuming an earlier test did.

v0.5.1627 stopped `resolve_prototype_addr` from building the realm global as a
side effect of answering "is this `Object.prototype`?" — correct, and worth
~5 ms on the first `setTimeout`. A consequence: `object_prototype_addr()` now
answers `0` until something has actually built the global, which is the honest
answer because no intrinsic prototype exists before then.

This test asserts `!prototype.is_null()` as its premise. It passed only when an
earlier test in the same process had built the global, so it **passed in the
full suite and failed run alone** — a pass that depends on test order rather
than on the code under test. `cargo test --lib transition_fast_rejects_object_prototype`
failed on `main` in both debug and release.

One `js_get_global_this()` in the setup establishes the premise. `object::` goes
from 487 passing / 1 failing to **488 / 0**, and the test now passes in
isolation as well as in the suite.
