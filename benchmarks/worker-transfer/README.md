Transfer 100 fresh 8 MiB ArrayBuffers to an in-process worker and back, one
roundtrip at a time. Fill every buffer and verify its endpoints and both sender
detaches. Time the complete process with `/usr/bin/time -f "%e %U %S %M"`.

Compile `roundtrip.ts` using a coherent Perry compiler/runtime/stdlib build;
compare that executable with `node --experimental-strip-types roundtrip.ts`.
Interleave runs and report medians and ranges. Fresh buffers deliberately
include allocation, zeroing, fill and normal GC in the measurement.
