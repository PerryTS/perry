Restore the runtime GC-holder custody gate on main with researched verdicts
for seven newly enumerated holders: worker trap depth, old-reclaim deferral
byte accounting and its boolean valve latch, three test-only probe/call
counters, and the worker inbox's Rust-owned serialized command channel.
The valve latch exists in production and is classified as scalar state;
the inbox carries no collectible GC pointer. No new rooting bug was found.

Remove nine stale entries for holders now reached by the scanner census or
deleted. Re-audit PASS1_MARKED against the collector history since its pinned
cycle.rs and policy.rs revisions, append the dated review, and update only
those two source hashes. The new sweep diagnostic runs after snapshot
consumption; old-reclaim policy changes run before collection or after sweep.
No relocation, JS callback, GC allocation or nested collection was added
between census pass 1 and sweep entry. The gate implementation is unchanged.

Validation: the holder gate and its self-test pass. Native-handle ledger and
raw-handle debt source counts and recorded baselines remain unchanged; their
existing main failures are outside this classification change. No Rust was
changed, so no host build was required.
