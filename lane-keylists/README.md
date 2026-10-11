# Keylists lane evidence harness

Frozen baseline: 301e9b2e114471d69957f0895ff4ef266ecac356. Builds and runs on qb6,
CPUs 0-23,32-55, at most eight build jobs. No version bumps.

Canonical and class key backings use I3 old, stationary storage. The trie and
class memo remain weak shape authorities, pruned only after a full trace.
Owned tombstone and Array-subclass key lists remain movable and keep their
existing shape-family relocation support. Prototype words belong to lane 3.

The measurement linker shim reads the existing always-on counters at the exit
funnel with PERRY_GC_DIAG unset. It disables THP for its process with prctl;
it neither changes the system THP setting nor adds a production runtime mode.
Both arms link the same shim. The private cc wrapper links a pure executable
from the very same fresh compiler objects, removing the observer object and
wrap argument. Pure executable .text sizes are reported separately. Globalizing
the counter symbols changes their binding only; it changes no Rust machine code.

The original timers omit synchronous automatic full GC, and the exported
collection count belongs to one thread. The worker driver demonstrated that
limitation (41 full events versus 19 on the main thread). Final measurements
use identical private observation patches in both arms, counting kind and
active elapsed time at each collector completion. This adds two atomic adds
per collection and no operation/site work; diagnostics remain off. These
copies have separate sources and targets. Pure twins link the original
archives, so reported .text is emitted shipping code. The observation patch
exists only in the reproducible lane harness.

Keylists2 additionally makes canonical attributes old and stationary, requests
first-copy promotion of still-young string keys, and removes redundant dirty
repair after synchronous full marking. Sliced collections and all barriers
retain their custody. Full shape walks retain the existing writer log instead
of reconstructing it over every stationary family. Full marking reuses the
barrier's parent classification instead of looking up generation twice.
All field ranges keep one descriptor slot loop, without a page coverage branch. Shared key storage cannot contain foreign
handle ids, so active handle providers do not force scans of its scalar words.
The child descriptor still uses the global observer gate. Tests reject a handle
word falsely declared as shared metadata and keep ordinary-array observers live.

`prepare-phases.py` adds thread-local instructions:u attribution to private
observation sources only. Phase-instrumented runs are diagnostic, never accepted
as the final instruction/RSS A/B. `build-stream-graph.py` reproduces the coherent
Fastify library selection in independent targets. `summarize2.py` derives the
current report from n=5 raw rows, including per-program text and class slopes.

The latest main includes presence/accessor lowering and whole-program function
layout. Final frontend objects must be rebuilt from that compiler. The observer
sets PR_SET_THP_DISABLE before main; the hot-text mapper honors that flag.
Synchronous full sweeps reuse the completed element-shape death prune while
retaining header and epoch invalidation. Minors and sliced fulls keep the
per-object cleanup. Test builds assert that a claimed prune removed the record.

The old-arena mark loop specializes the parent proof at object entry. Malloc
parents reuse the ordinary loop and one cold descriptor pass over post-observer
words, with owner-qualified remembered coverage and weak/ephemeron exclusions.
This removes the third complete mark-loop copy. Controls separately check
liveness, missing coverage and observer rewrites in census and classifier modes.

Runtime review rounds may reuse a completed current-main frontend batch. The
harness compares compiler-source digests before reuse and relinks both runtime
arms. All 18 programs, both class micro sizes, locked pauses and moving stress
still run in the complete acceptance round. A frontend-only completion marker
states its scope explicitly and is not an acceptance result.

`KEYLISTS_ROOT` selects a separate directory inside the approved hostdir for
a fresh baseline while an earlier diagnostic round finishes. Every arm keeps
its own target; cache copies remain within the same arm.

The preceding 16c9af baseline needed one identical build correction in both arms:
`object/spread.rs` supplied zero for the new ordinary shape integrity argument.
That correction is now upstream; the frozen 301 baseline is untouched.
The early tsc overlap uses two jobs and a compiler/input hash proof; native
corpus work reserves two more jobs. Stream graphs and other frontends reduce
their job limits while either reservation is active. These are harness files,
with no runtime options or shipping counters.

The final copying root walk passes its existing decoded RootWord directly to
the classifier/marker and publishes its diagnostic kind once per producer
walk. Native-slot provenance still updates at every native slot. The same
alignment rule, header validation, handle rejection and tag re-encoding stay
in force. A producer-walk test requires a live staged slot, its exact phase
label, and restoration of the caller's label.

`baseline-overlap.py` builds the fresh baseline and overlaps its two-job tsc
frontend with observation/selected libraries and units. `current-runtime.py`
reserves two runtime jobs; other frontends deduct both that reservation and
native-corpus/tsc reservations so the combined work stays within eight jobs.
`relink.py` compares compiler-source digests before reusing frontend objects.
The frozen cohort prevents later main updates from mixing into an active A/B.
