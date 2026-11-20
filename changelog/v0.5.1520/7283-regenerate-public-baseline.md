Regenerates `benchmarks/results/public-node-bun-v1.json`, which had been stale for
40+ commits. The freshness gate is `lint`'s step 9, so while it failed, **steps
10-13 never executed in CI at all** — file-size, GC store-site inventory,
addr-class audit, and #7253's gate-wiring check had to be reproduced locally to
even be seen, and every merge required an `--admin` bypass.

**The measurement host changed** (see the PR for why): Apple M1 Pro / 16 GB →
Apple M1 Max / 64 GB. Node, Bun and Zig are held at their original pins, so the
discontinuity is one variable rather than three. Numbers before and after
2026-08-03 are not directly comparable.

Also fixes `EXPECTED_WORKLOADS["app_patterns"]`, which never gained the `batch`
kernel added by #7037. That omission meant #7037 both invalidated the artifact
(kernels are in `SOURCE_PATHS`) and made regenerating it impossible — assembly
aborted with `extra=['batch']` after all five measurement legs had already run.
