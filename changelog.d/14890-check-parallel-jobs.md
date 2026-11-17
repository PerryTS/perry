CI: clippy and the auto-optimize pruned builds run in their own `clippy` job beside `check`, so `check`, which the fan-out waits on, finishes about seven minutes sooner.
