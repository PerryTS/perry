**gate(gc): promote the parse-then-churn layout-state check to CI (#7647)**

`PERRY_JSON_TAPE=0` + `PERRY_GC_FROMSPACE_SCAN=1` over a parse-then-churn
workload — the known-good end-to-end detector for the layout-state family
(#7630/#7633/#7635/#7643/#7644) that #7643 measured but nothing ran in
CI — now runs as `gc-parse-churn-gate.yml` on every PR and `main` push
(`scripts/gc_parse_churn_layout_gate.sh`). The verdict
(`scripts/gc_parse_churn_layout_check.py`) requires correctness, liveness (a
copying minor actually relocated objects), and eagerness (the from-space
scan's own census reached the record count, so the cohort was not still on
the lazy tape) before it can pass — `--self-test` proves it can say no on
each axis. Not yet required in branch protection; that is a maintainer
action for after the first green run on `main`.

Along the way, fixed a real false-positive source in
`PERRY_GC_FROMSPACE_SCAN`: string allocations left their 0-7 byte
alignment padding uninitialized, so leftover arena bytes there could
occasionally decode as a plausible pointer. Harmless to every string API
(all bounded by `byte_len`, never `GcHeader.size`) but not to a scan that
trusts `GcHeader.size` as the payload's true extent. Fixed at
`string_storage_alloc`'s single choke point, mirroring the existing
`TAG_HOLE` fix for array-growth slack.
