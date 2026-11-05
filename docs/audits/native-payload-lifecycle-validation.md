# Native payload lifecycle runtime validation (#11919)

Current-main rebase integration and RSS attribution are recorded in
[native-payload-lifecycle-rss.md](native-payload-lifecycle-rss.md). The results
below describe the earlier base and its original measurement protocol.

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

`cargo build --release -j 8 -p perry -p perry-runtime -p perry-runtime-static
-p perry-stdlib-static` passed for each arm. Final combined validation:
`cargo test --release -j 8 -p perry-runtime -p perry-stdlib -p perry-codegen
-- --nocapture` exited 0.

| Package | Unit | Integration | Doc | Failed | Ignored |
|---|---:|---:|---:|---:|---:|
| runtime | 5,019 | 1 | 0 | 0 | 13 |
| stdlib | 247 | 0 | 0 | 0 | 0 |
| codegen | 1,991 | 516 | 3 | 0 | 6 |
| Total | 7,257 | 517 | 3 | 0 | 19 |

Every T1–T12 witness passed; all 14 callback sabotages were RED. L4, L5,
L8, L9, reopen identity, terminal-event tracing/late listener and churn passed;
all four lifecycle sabotages were RED. The isolated lifecycle churn reported
`created=finalized=drops=200000`, warmed RSS 45,883,392 bytes, peak 45,883,392,
delta **0 bytes**. In the complete runtime process its warmed delta was
24,576 bytes. The <4 MiB assertion is unchanged. Each batch runs a moving
minor and full sweep: a full-only schedule protects recent nursery bump
blocks and initially exceeded the RSS bound despite exact finalized/drop
counts; the witness now exercises the production generational path.

Program measurements use three alternating samples per arm, `instructions:u`
from `perf stat`, task-clock converted from nanoseconds to milliseconds,
`/usr/bin/time` peak RSS and separate GC diagnostic replays. Every sample
compares stdout byte for byte with pinned Node 26.5.1. Results below are
medians; GC columns are full/minor counts, identical between arms.

| Program | Instructions main → head | Δ instructions | CPU ms main → head | RSS KiB main → head | GC full/minor | Node output |
|---|---:|---:|---:|---:|---:|---|
| hello | 1,338,162 → 1,338,237 | +0.0056% | 1.91 → 2.13 | 14,680 → 14,328 | 0/0 | equal |
| tsc | 28,263,596,386 → 28,262,329,615 | -0.0045% | 2,086.65 → 2,067.22 | 258,220 → 261,724 | 1/4 | equal |
| zod5k | 16,054,077,702 → 16,053,836,420 | -0.0015% | 931.96 → 935.72 | 55,828 → 56,096 | 0/97 | equal |
| qsparse | 28,620,443,360 → 28,621,744,435 | +0.0045% | 1,886.07 → 1,882.21 | 58,876 → 58,900 | 0/119 | equal |
| qsstr | 76,491,515,715 → 76,447,325,656 | -0.0578% | 4,376.35 → 5,053.81 | 58,508 → 58,660 | 0/439 | equal |
| commander | 7,797,884,533 → 7,797,798,652 | -0.0011% | 571.04 → 574.23 | 51,572 → 51,892 | 0/33 | equal |

The largest median RSS difference is tsc +3,504 KiB (+1.36%); the other
programs differ by −352 to +320 KiB. Instruction changes are all within
±0.058%. CPU timings varied between passes on the shared host (qs stringify
was 5,228.78 → 5,085.54 ms in the first pass, 4,376.35 → 5,053.81 ms in the
final pass); the requested instruction gate passes in both passes.

| Gate | Final head / main comparison |
|---|---|
| native_handle_ledger | PASS: 202 tables / 188 producers; ceilings unchanged |
| native_handle_ledger self-test | Same inherited failure: stale ext-zlib `__STATICS_HANDLE_TABLES` classification |
| raw_handle_debt | Same 868 sites vs baseline 861; three inherited module violations, no new sites |
| raw_handle_debt self-test / no-raise-vs | PASS; recorded baseline 861 and 107 module ceilings unchanged |
| gc_runtime_root_holders / self-test | PASS / PASS; 1,542 declarations, 155 registered scanners; 92 planted shapes |
| fmt / diff whitespace / file-size gate / Node-version consistency | PASS |

Main and head outputs for the three census gates and their self-tests are
byte-identical. Raw debt's inherited violations are `node_stream/async_iterator.rs`
(10 vs ceiling 7), `node_submodules/zlib.rs` (1 unlisted) and
`object/field_get_set/exotic_named_read_tests.rs` (3 unlisted). No ceilings,
inventory verdicts or gate logic are changed by the lane.

A pre-existing stdlib fixture, `streams::tests::pipe_through_pair_survives_a_moving_getter`,
failed on both unmodified main and head with `Invalid transform writable`,
including an isolated main run. It bypassed program startup and collected
without registering the runtime handle/accessor root scanners. This lane adds
`gc_init()` to that fixture before its deliberate moving collections; its
existing child-moved assertion remains intact. No stream production code is
changed.

The existing DOMException thread-exit fixture also initializes its observing
heap before starting its worker. The unchanged fixture failed in an isolated
full-feature main run; the corrected fixture and combined head run pass.
With the full runtime features (mimalloc),
initializing that heap only after join can reuse the dead worker's arena block
and classify its stale DOMException header as observer-owned. The fixture
still requires the worker's live brand and rejects the dead worker's brand;
this removes allocator reuse from that ownership observation.
