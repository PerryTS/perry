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
that collects before returning the write function. Validation of the final head is
recorded in the PR description.

## Review follow-up (Claude, 2026-10-04)

The review on #11882 found the same class in sibling paths. Each window now
has a test that collects in it, asserts the values under test moved, and runs
with the from-space poisoned, so a stale address reads as no object at all.

- Stream-list path (`pipeline(a, b, [c])`): source, destination, the rest
  array, the promise and the callback were raw across `js_promise_new`, which
  runs `promiseHooks` init hooks, and across the allocations building the
  argument array; the rest entries were a Rust `Vec` snapshot. All are held
  in one handle scope and the arguments are pushed from the rooted array.
  Trigger: a collecting init hook.
- `js_node_stream_readable_chunks_result` returns the chunks as a GC array
  rooted in the caller's scope, and rereads the stream after `_read` (user
  code) and after each hidden-property lookup. No `Vec` of JS values reaches
  a destination write. Trigger: a collecting `_read`.
- The getter test now installs `write` as an accessor only, counts getter
  calls (exactly three) and asserts that the getter's own collection moved
  the chunks. On the original base it fails after one lookup.
- `unpipe()` with no argument and the end fan-out walked a `Vec` of
  destinations, and a raw source, across `'unpipe'`/`'end'` listeners. They
  now share the rooted GC-array snapshot #11841 introduced for writes.
  Trigger: the first destination's listener collects.
- `js_promise_rejected` held its reason raw across `js_promise_new`.
  Trigger: a collecting init hook while the pipeline rejects with a source
  error.

Negative controls: the tests-only commit on top of the PR's previous head
fails the read, promise-hook, rejection, unpipe and end tests; the same tests
on the original base `4b7564c638` fail all of them, including the chunk,
receiver and getter tests.

Remaining raw snapshots of JS values in the stream code are listed in the PR
description; the classic `stream.pipeline` and compose paths hold `Vec`
snapshots of stages across user stage functions and need the same treatment.

This establishes a native rooting bug. It does not establish that this path
causes the package-manager corruption reported in #11842. Conservative scan
policy, pacing and incremental collection remain unchanged by this patch.

Claude's `perf-class-value-scan` lane owns class constructor slot scanning.
This patch changes no class roots, remembered-set representation or object
layout.
