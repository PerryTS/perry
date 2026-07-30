`scripts/gc_repsel_matrix.sh` now reports the two relocating collectors separately
instead of summing them into one `moved=` figure.

`moved_objects=` comes from the C4b evacuation policy inside the mark-sweep
collector; `[gc-copy-minor] ran copied_objects=` comes from the copying young-gen
minor that #7019 made default-on. Summing them let a `requires=move` cell report
green on relocation the arm was not testing — a cell could show a healthy
moved-objects count while running zero copying minors, which is the exact shape
#7024 describes.

Cell evidence is now `cycles=N evacuated=X scavenged=Y`, and the per-arm liveness
summary gains a `copy-minor n/N` column alongside `moved-objects n/N`.

Verdicts are deliberately unchanged: the `move` predicate still tests
`evacuated + scavenged > 0`, so this run is directly comparable to the recorded
baselines. Tightening the predicate to require a copying minor where the arm
demands one needs #7024 fixed first — under `--pressure` the copying minor
currently never runs at all, so a stricter gate would fail arms for a harness
defect rather than a real one.
