# Native payload lifecycle runtime validation (#11919)

Lane base: `51a20469efd7bf418e3a52b9b6aa887eef4f2290`. Final verification and
A/B code base: `014f3e553d574e9e136b57b8ac57c89519755459`. Final head includes current
main `a6c147b7b00b51f18f56ce2e056cf190deca893d`; the two intervening commits
change only root-holder inventory and changelog metadata. Builds and tests run on
`perrymaster` in `/root/codex-lanes/cx-lifecycle`, with separate `target` and
`main-target` directories, Node 26.5.1 and `RUST_TEST_THREADS=1`. No builds ran
on macOS. The exact raw results are also delivered beside `lifecycle.bundle`.

## API and invariants

| API | Behavior |
|---|---|
| `alloc_closed(family, own)` | Installs the object's permanent cell without a native resource |
| `lifecycle(value, family)` | `Open`, `Closing`, `Closed`; finalized is `PayloadMiss::Closed` |
| `attach<T>(value, family, payload, bytes)` | Installs into that same CLOSED cell; rejects `Foreign`, `Open`, `Closing`, `Finalized` and drops the rejected input |
| `next_open_serial()` | One process-wide atomic supplies opaque `OpenSerial` equality stamps for reopenable payloads, children and resource completions |
| `close(value, family)` | Releases, preserving the cell, owner edge, type/drop metadata, refs and creator thread; busy calls defer release |
| `owner_link(value, family)` | Accepts every non-finalized state for owner-linked families |
| `link_owner(link)` | Existing OPEN-only, non-throwing synchronous trampoline path, unchanged |
| `link_event_owner(link)` | Non-throwing OPEN/CLOSING/CLOSED owner lookup for event dispatch |
| `enter(value, family)` | Independently requires OPEN before incrementing busy |
| `NativeCallGuard::finish()` | `Result<(), CallEnd>`; `Threw(value)` wins over `Closed`; outermost close releases after C returns |
| `link_ref` / `link_unref` | Unchanged; a queued item owns one ref through dispatch, even after close |

Release sets CLOSING around the drop thunk, nulls the resource, clears
ownership, returns external bytes and clears CLOSING. Sweep and teardown alone
finalize Rust payload cells. `payload_mut`, `link_owner`, the owner/metadata
stores and cell layout are unchanged (136 bytes on 64-bit). The GC visitor
continues visiting CLOSED owner edges. Teardown finalizes pinned native cells;
other pinned objects retain their existing teardown behavior.

The requested non-generic `alloc_closed` cannot know `T`. Its first attach
therefore establishes the existing type-layout tag and drop thunk. Subsequent
attaches preserve them; a different layout or drop thunk is rejected as
Foreign, including a distinct payload type with the same size and alignment. This is the
only additional first-install metadata write, and avoids changing the family
API, growing the cell or changing payload access. Reporting external bytes
can collect **after** installation, so attach roots the owner for that report.
There is no GC allocation or JS call before installation.

PR #12020 / `attach_rooted` was absent from the base; no ALS call site was
available to replace. Family-specific listener queues, sqlite child serial
checks and callback-array clearing remain the family lanes' responsibility.
The L8 runtime witness models the plain-data pump queue and exercises actual
worker TLS cleanup with a still-pinned cell; it does not convert a family's
queue in this runtime lane.

## Witnesses and sabotages

All sabotage arms exist only in test binaries and run their exact witness in
an isolated child. The harness asserts one test ran and that it failed.

| Design test | Runtime witness | Sabotage |
|---|---|---|
| T1 | Owner relocates while a native call/site is live | Omit owner rewrite |
| T2 | Ref'ed cell preserves owner/callback through full GC | Omit owner mark; omit pin |
| T3 | Malloc-cell owner store enters the remembered set | Omit barrier |
| T4/T5 | Exact throw identity, C regains control, reuse, first throw wins | Omit catch; omit pending short circuit |
| T4 validation | Pending TypeError is parked without throwing through C | Covered by pending/throw protocol |
| T6/T11 | No destruction callback enters JS at close, sweep or worker exit | Omit finalized lookup; finalize after drop |
| T7/T8 | Nested calls, immediate closed visibility, deferred release | Release while busy; reject reentry |
| T9 | Wrong-thread owner lookup never throws | Use throwing lookup |
| T10 | 200,000 owner capture cycles collect | Leak a ref |
| T12 | Nested catch/rethrow leaves busy and try depth balanced | Throw from conversion before finish |
| Pin cost | Cell refs do not arm young-pin latch | Pin owner |
| L4 | Release then unrooted sweep: finalized +1, drops unchanged, no JS | Leave close ref/pin |
| L5 | Teardown-finalized cell rejects attach and drops input | Ignore finalized at attach |
| L8 | Worker discards queue without dispatch; pending pin cannot leak cell | Dispatch queue after finalize |
| L9 | Callback closes then throws; exact throw wins, reopen works | Prefer Closed over Threw |
| Reopen identity | 1,000 attaches preserve object/cell/properties/prototype/links and return bytes; reopen after moving GC; reject Open/Closing | State/identity assertions |
| Terminal events | Closed owner survives full + moving GC; dispatch reads late listener; last unref collects it | State/trace assertions |
| Lifecycle churn | 200,000 alloc_closed/attach/ref/close/unref cycles; created = finalized = drops; RSS delta <4 MiB | Exact counts and RSS bound |

The callback suite retains all 14 original sabotage pairs. The lifecycle
suite adds four pairs for L4/L5/L8/L9. These are runtime-contract units; the
sqlite/net behavioral and Node-oracle witnesses remain their family lanes.

## Recorded results

Results will be filled after Linux validation.

A pre-existing stdlib fixture, `streams::tests::pipe_through_pair_survives_a_moving_getter`,
failed on both unmodified main and head with `Invalid transform writable`,
including an isolated main run. It bypassed program startup and collected
without registering the runtime handle/accessor root scanners. This lane adds
`gc_init()` to that fixture before its deliberate moving collections; its
existing child-moved assertion remains intact. No stream production code is
changed.

The existing DOMException thread-exit fixture also initializes its observing
heap before starting its worker. With the full runtime features (mimalloc),
initializing that heap only after join can reuse the dead worker's arena block
and classify its stale DOMException header as observer-owned. The fixture
still requires the worker's live brand and rejects the dead worker's brand;
this removes allocator reuse from that ownership observation.
