Native payload close now releases the installed resource while preserving the
object's permanent cell and traced owner edge. Only GC sweep and worker
teardown finalize the cell. A closed instance can reopen through `attach`
without changing object identity, properties, prototype or existing OwnerLink
tokens; `alloc_closed` supports instances born without a resource.

The runtime adds lifecycle/attach state checks, process-wide OpenSerial stamps,
and `link_event_owner` for terminal events queued before release. Native-call
finish returns `CallEnd::Closed` for a deferred close and preserves a callback
exception as `CallEnd::Threw`. Worker teardown also finalizes pinned native
cells whose pending refs expire with the worker. Cell size and the
`payload_mut`/`link_owner` hot paths are unchanged.

The native-payload pattern documents per-item refs, dispatch-time listeners,
serial checks for stale children/completions, reopen and queue teardown rules.
Runtime witnesses cover release/sweep, finalized attach rejection, worker queue
discard, throw-before-close priority, same-cell reopen, moving closed owners,
and 200,000 release cycles. The existing T1–T12 callback witnesses retain all
14 sabotage checks; lifecycle witnesses add four sabotage checks.

The moving-getter stream unit fixture now initializes the GC root scanners
before deliberately collecting, matching generated-program startup. This
fixes its inherited failure on main without changing stream production code.
The DOMException worker-exit fixture initializes its observing heap before
the worker runs, so allocator reuse cannot make the dead worker's stale
header look like a new allocation owned by the observer.

Integration with current main retains `alloc_with_prototype` and provides
`attach_to_object` for existing subclass objects. Reopening preserves the
existing cell and rejects open or finalized cells. AsyncHook's unpublished
record index is initialized in its existing open payload, rather than
replacing that payload through attach and retiring the new record.

The RSS attribution and identical-binary control are recorded in
`docs/native-payload-lifecycle-rss.md`. `scripts/runtime_rss_ab.py` prepares
each executable's file cache identically before interleaved Linux RSS runs;
copied and linked copies of the same ELF can otherwise differ by over 10 MiB
of clean file-backed RSS even with anonymous THP disabled.
