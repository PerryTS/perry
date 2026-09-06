Primitive-array validation reuse, following the qualified record release.

The record emitter preflights every field before writing any output. Its dense
primitive-array children previously went through the same validation again on
emission. A shared, inlined emitter now consumes that prior proof, under an
explicit unsafe contract: no callback, managed allocation or safepoint may
intervene. The generic primitive-array entry still validates before calling it.
A record whose last array contains a complex value still declines before any
output, even if an earlier field holds an eligible array.

All 133 JSON-filter runtime tests, 31 compiled Node comparisons and 20 moving-GC
stress runs pass. The static root-holder inventory passes with its existing
frontier unchanged. GC production source and policy remain untouched.

A local 36-process instruction diagnostic finds about 5.5% fewer instructions
for 16 KB/1 MB records, about 2% fewer for small/20 MB records, and essentially
unchanged instruction counts for numeric/heterogeneous controls. This is not
CPU/RSS acceptance evidence. The first shared-M1 run was rejected before any
measurement because a foreign ccperf worker had started. A second full run completed but was unqualified: 20 of 33 external observations
detect foreign workers, and the ending load gate fails. All raw trials remain
in fastpaths-unqualified. Qualified performance for this follow-up is pending; it does not replace the record-release matrix.
