The `[[Set]]` receiver own-key probe walked the keys array one element at a
time through `js_array_get_f64`, the JS-facing element accessor. That accessor
runs the whole gauntlet per element — forward resolution, Map/Set/typed-array/
buffer registry probes, the descriptor gate, hole translation — for what is a
raw slot read, and the probe repeated it for every key on every store. At 400
stores that was 163,200 element reads.

The probe now reads the backing storage directly. A test-only entry counter on
`js_array_get_f64` lets the regression test assert the walk no longer reaches
for the element accessor at all (163,200 → 0) rather than timing it, which is
the difference between a test that pins the property and one that pins the
machine it was measured on.
