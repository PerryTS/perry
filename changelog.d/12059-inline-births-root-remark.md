Generated stores into compiler-managed GC roots no longer emit an
incremental-mark shading gate and out-of-line call at every assignment.
Budgeted collections already rescan every local and module-global root during
their final remark, while synchronous collections expose no mutator window;
heap stores, runtime-owned roots, weak reads and allocation coloring keep their
existing barriers. This removes a major source of repeated cold code in large
functions without changing relocations or GC root rewriting.

All inline GC headers now incorporate the runtime's live per-thread birth
flags in their initialization store. Non-leaf inline births use the same
runtime seed protocol after every slot is valid and before publication.
The existing per-thread seed queue is resolved before the allocation window,
so the shared seed operation needs no TLS resolver or collecting call.
This covers both small array literals and inline class allocation, including
barrier-disabled build windows and post-remark mutator windows; birth flags
remain off after the sweep snapshot. Whole-module IR checks, poisoned early
seed controls, local-only array/class witnesses and 200 stable-root homes
guard the protocol without per-site root-shading workarounds.
