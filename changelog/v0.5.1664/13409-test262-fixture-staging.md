### Fixed

- `scripts/test262_subset.py --full-corpus` now stages `_FIXTURE` files next to each case, so 275 dynamic-import cases no longer fail on a missing module; the Test262 baseline moves to 89.24% (47,560 / 53,293).
