Disabled the evacuating (moving-loop) minor GC by default, restoring the
non-moving minor as the default collector. The evacuating minor #7019 made
default-on has a use-after-free (#7154): a young closure referenced from a
dynamically-added object field is reclaimed while still live, so a later call
dies with `TypeError: value is not a function`. This reproduced in the default
configuration, so the shipped binary could corrupt the heap. The moving-loop
path is unchanged and still available behind an explicit
`PERRY_GC_MOVING_LOOP_POLLS=1` opt-in (compile and run). This is a stopgap until
the root cause tracked in #7154 is fixed; it trades #7019's minor-GC
RSS/throughput win for correctness.
