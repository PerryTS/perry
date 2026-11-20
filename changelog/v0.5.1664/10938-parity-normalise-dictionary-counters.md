Taught the parity harness's output normaliser about the `[object-dictionary]`
counter row (#10868 step 2.5 stage 1).

`gc/schedule.rs` prints that row beside the `[gc-schedule]` lines, so it appears
under exactly the fixtures that declare
`// parity-env: … PERRY_GC_SCHEDULE_SEED=…` — 13 of them today. The normaliser
already strips `^\[gc-schedule\]` for the documented reason that Node prints no
such thing and every such fixture would otherwise diff as an output mismatch,
but the new row carries a different prefix, so the existing rule did not cover
it and those fixtures went red.

`test_gap_dynamic_import_alias_binding` was the first to surface it, and it
surfaced misleadingly: the harness's truncated view showed
`object object object object function` for *both* Node and Perry, because the
only difference was four lines further down (#796). Reproducing outside the
harness shows the program output is byte-identical and the whole delta is
instrument noise.

Same reasoning as the `[gc-schedule]` rule, same treatment. A crash under the
instrument is still caught — abnormal exits are detected from the exit status,
before either comparison runs.
