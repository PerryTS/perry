Pinned the inherited-read walk's marking discipline with a test, and stated the
sentinel-placement requirement at `NEGATIVE_SLOT`.

The walk marks one unmarked prototype hop and **abandons without recording**, so
the next read gets one hop further. Read cold that looks like a free
optimisation to delete: it costs one declined read per hop and buys nothing
visible at the site.

It is what makes an *absent* verdict sound. "This key is on nothing in the whole
chain" is invalidated by `proto_validity`, which bumps only for MARKED
prototypes — so such a verdict holds only if every hop on an exhausted chain was
already marked. The walk can only proceed past a marked hop, so proving
exhaustion implies every hop it passed was marked. That argument lived in
another mechanism's file and nothing stated it.

The test builds `O -> P1 -> P2` with both hops installed **without** the funnel,
so neither starts marked, and asserts each walk advances exactly one hop: after
walk 1, P1 is marked and P2 is not; after walk 2, P2 is; only then does the walk
resolve. It is red on both shapes the "cleanup" takes — mark-and-carry-on, and
dropping the marking entirely.

Train notes: rebased onto v0.5.1629 and reformatted. The PR's one non-test
change to `inherited_read_cache.rs` is a doc comment plus a line wrap that was
correct against its original base and not against current `main`; `cargo fmt`
re-wrapped it. No behaviour change.
