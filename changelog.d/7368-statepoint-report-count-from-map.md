### Fixed

**`--statepoint-report` reported `0 statepoints emitted` for every compile since #7348.**

#7348 deleted the explicit statepoint bridge, and with it the only callers of
`FunctionRecord::note_statepoint` and `note_skipped` — they lived in the bridge,
which counted safepoints as it emitted them. The methods survived with no
callers, so `statepoints`, `relocations`, `max_live_roots`,
`skipped_non_safepoints`, `live_roots_histogram` and both by-callee maps became
structurally zero in production. Measured on a real compile:

```
5 function(s), 5 bound native root slots (5 logical slots reserved)
5 textual calls: 5 with live roots, 0 without
0 statepoints emitted; 0 non-collecting calls skipped     <-- binary had 120
0 relocations; maximum 0 live roots at one safepoint
```

Counting at IR-emission time cannot work any more, and that is the real lesson:
**Perry no longer decides which calls become safepoints — `RewriteStatepointsForGC`
does, inside LLVM.** The only honest source is the compact-map rewrite, which
already parses the assembly LLVM actually emitted and computed exactly these
numbers before throwing them at `log::debug!`. The report now reads from there:

```
120 safepoints across 6 function(s) in 1 module(s)
36 live roots recorded, 0.30 per safepoint
```

An absent measurement no longer renders as a measured zero. `gc_map.modules == 0`
means "the rewrite never reported", the text report says
`Safepoint counts UNAVAILABLE` instead of printing zeros, and the JSON carries
`gc_map` separately from `totals` so a consumer can tell the two apart.
`schema_version` is now `2`.

**The CI gate now asserts the counts, not just the label.** `gc-native-roots`
checked `--only-backend rs4gc`, which passed throughout the regression — the
backend label was correct, the numbers were fiction. It now also requires
`records > 0` and `roots > 0`. Verified against a synthetic report with the
#7348 shape: the label check reports 9 functions green while the count checks
exit 1.

Note this is the *second* round of dead counters in this file (#7362 removed four
that never had a writer at all). The new test documents why the first invariant
missed this one: `every_rendered_counter_has_a_writer` called the mutators
itself, so "has a writer" passed while "is written" was false. The structural fix
is that the numbers now have exactly one producer and their absence is loud.

Three review fixes on top of the above:

- The report assertion ran on `09_try_catch_roots`, which contains four `try`
  blocks — and RS4GC cannot rewrite WinEH funclet pads, so
  `rs4gc_funclet_refusal` rejects that probe on `windows-msvc`. The probe loop
  above tolerates it by grepping the compile log for "funclet"; this step did
  not. The portable assertion now uses `11_collect_at_depth` (no `try`, compiles
  on all four arms) and `09_try_catch_roots` keeps its own non-Windows step, so
  the try-specific coverage is not lost.
- The `gc_map` doc claimed `records`/`roots` would be *absent* when unmeasured.
  They are plain `u64` fields on a plain derive and always serialise; `modules`
  is the sentinel. Corrected to describe what the code does.
- The "map never reported" guard in `statepoint_report_assert.py` fired for any
  `--require-*`/`--print`, including fields that live in `totals` and are
  counted at IR-emission time regardless of the rewrite. It is now scoped to
  map-backed fields, so `--require-positive textual_calls` is answered from its
  measured value instead of being failed by an unreported map it does not use.
