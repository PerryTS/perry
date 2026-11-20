### Fixed: an ordinary frame now releases the box cells it minted (#10464)

A `let`/`var` that a closure captures and something reassigns is stored in a
malloc-side **box cell**, and every registered cell is a strong GC root
(`scan_box_roots_mut`). The only release Perry emitted was the async-to-generator
transform's terminal `Stmt::ReleaseBoxes` (#7933/#8208/#8303), so a synchronous
function, method, arrow, generator — or an `async` function with no `await` —
leaked one registered root per boxed binding per call, plus everything that
binding last pointed at. `PERRY_GC_DIAG=1` reported `releases=0` for every
non-async workload; the issue's repro reached 601 MB RSS at 100k calls where the
equal-allocation control stayed at 69 MB, and real packages accumulated cells by
the hundred thousand (qs 792k, dayjs 434k).

**Root cause.** `js_box_alloc_bits` registers each cell for the life of the
thread, and only `perry-transform`'s async step lowering produced the
`Stmt::ReleaseBoxes` that `emit_release_boxes` lowers. Nothing named the cells of
an ordinary frame, so nothing could ever reclaim them.

**Fix.** Codegen registers every entry slot that holds a cell *this* frame minted
(`stmt/boxed_frame_release.rs`); the return-site rewrite that already injects
`js_shadow_frame_pop` now also emits `js_box_scope_release` for each of them
before every `ret`, and a declaration inside a loop releases the previous
iteration's cell before minting the next. The runtime publishes a cell no closure
captured (de-register, cache-evict, clear, free-list push) and marks a captured
one frame-released in its capture-edge record, so the last capture edge's GC
death publishes it — the same escape contract #8303 built for async activations,
including the full-trace ephemeron rule that keeps `box -> closure -> same box`
collectable. Two holders the runtime cannot count keep their cells: a sloppy-mode
mapped `arguments` object, and a plain-async step closure's own activation cells.
A step closure's capture of an *enclosing* frame's cell is now counted, because
the activation token never covered it and that frame does release its cells.

Fixing this exposed a latent GC hole shared with #8303: a full trace that stops
rooting a released cell must still keep it in the box young log, because a minor
walks only that log. Without it the next minor had no root for a payload a live
closure still reads — `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1
PERRY_GC_PROTECT_FROMSPACE=1` faults on it immediately, and a unit test now
re-derives the rule.

**Validation.** New gap test `test_gap_10464_box_cell_release.ts` (the repro's
memory shape compared against its own control in-process, plus escaping-closure,
per-iteration-binding, generator, class, async and self-cycle cases) differs from
Node on the parent commit and matches it here. GC stress over seeds 1-7 at
`PERRY_GC_SCHEDULE_RATE=1` with the from-space quarantine confirmed armed
(`[gc-fromspace-protect] retired_set=#0`, 141,635 hits over the run) held
byte-identical to Node on every deterministic output line, and
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1` (panics on any stale
forwarded-pointer read) passed clean. Instructions *improve* slightly on a hot
captured-variable loop (20M calls, -3.6%) and on 1M closure creations (-2.6%),
from reduced GC root-scanning pressure; the issue's `payload` repro reproduces
the RSS claim independently: 608 MB -> 152 MB peak RSS here (originally recorded
616 MB -> 163 MB on the destroyed build host).
