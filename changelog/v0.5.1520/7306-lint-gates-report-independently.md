Every gate step in the `lint` job now carries `if: ${{ !cancelled() }}`, so one
failing gate no longer hides the ones after it. Setup steps (checkout, Node,
Rust toolchain) still stop the job — there is nothing to gate without them.

This was not hypothetical. The public-baseline freshness check sits at step 8 and
was stale for 40+ commits, so **file size, GC store-site inventory,
address-classification audit, moving-GC gate wiring, GC matrix liveness and the
dark-test registration check never executed in CI at all**. Four of them were
found red only by running them by hand (#7256, #7273), and #7253's wiring gate —
added specifically to catch gates that cannot fail — was itself unreachable.

Before this change 1 of 17 steps ran unconditionally. The job still fails if any
gate fails; it just reports all of them.

Relevant now because the benchmark artifact is expected to go stale during the
current optimization work — its only fix is a ~2-hour regeneration on a specific
quiet host, and that must not silently disarm six other gates each time.
