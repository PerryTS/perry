**Fixed** the macOS `native-roots-rs4gc` arm's in-process step could never pass.

It asserts that a copying minor moved objects by counting
`[gc-copy-minor] ran copied_objects=` lines, but both prints are gated on
`PERRY_GC_DIAG` and the step never set it. The trace held only the probe's own
`#gcmetric` lines, so the assert read 0/0 off an effectively empty file and
failed regardless of collector behaviour — the inverse of a gate that cannot
fail. It shipped with the step in #7339 and had never run, because three of four
arms in this matrix were permanently queued until #7393.

The liveness assert now distinguishes "no collector diagnostics at all" from
"the collector moved nothing", which the old message conflated.
