# Native stream pipeline rooting, 2026-10-04

The direct `node:stream/promises` pipeline keeps a Rust `Vec<f64>` of chunks
while calling user-defined destination methods. A moving collection cannot
rewrite that vector. Method getters can also collect before the call receives
its arguments, and the receiver must be reread after each callback.

## Reproduction

Base: `4b7564c6381c5bb9175162905ac4606b8dd276f7` (main fetched at work start).
Tests-only negative control: `2bce367c1f`.
Runtime fix under validation: `4473019ec4`.

On Linux, `pipeline_chunk_snapshot_is_refreshed_after_collecting_write` fails
on the tests-only revision with `native snapshot passed stale chunk addresses
at [1, 2]`. The test registers the runtime-handle, shape-rekey and descriptor
scanners used by the fixture, forces copying collection during the first write,
and asserts that its independent rooted expected array actually moved. The
destination is promoted before the fixture creates young chunks, isolating
chunk currency from receiver currency. All three writes and the end call occur
before the stale-address assertion fails. No stale address is dereferenced by
the callback's comparison.

The earlier test revision `624332b524` omitted shape rekeying and stopped after
one write. That run is retained for transparency; it is not the regression
proof.

## Change

- Copy the snapshot into a sized GC array before callbacks, using one array
  root and one value word per chunk. Reread each element after method lookup.
- Keep the destination in a runtime handle throughout writes and end.
- Root the receiver while allocating a property key.
- Keep the direct pipeline's source, destination, options and signal current
  across user callbacks.

The regression suite also covers a moving destination and a method getter
that collects before returning the write function. Candidate Linux full-runtime
tests and macOS focused tests were started against `4473019ec4`; results are
pending. Production wrapper, integration and full lint checks remain pending.

This establishes a native rooting bug. It does not establish that this path
causes the package-manager corruption reported in #11842. Conservative scan
policy, pacing and incremental collection remain unchanged by this patch.

Claude's `perf-class-value-scan` lane owns class constructor slot scanning.
This patch changes no class roots, remembered-set representation or object
layout.
