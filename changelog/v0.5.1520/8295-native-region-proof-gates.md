### compiler: restore the native-region proof gate

Proven Buffer and Uint8Array read-modify-write loops now keep their enclosing
byte store on the native path when the RHS is a bounds- and lifetime-proven
load plus inert arithmetic. The rooting classifier no longer treats that load
as a possible collection point and no longer discards the cached-view proof in
expressions such as `buf[i] = (buf[i] + 1) & 255`.

The compiler-output harness now distinguishes dynamic index helpers from
dynamic arithmetic helpers, and the native-region workload contracts track the
current optimized block layout and numeric-array representation rules. An
unfinished budgeted GC cycle can also shut down after its independent proxy and
mark-seed thread locals have been destroyed, so the image-convolution witness
exits cleanly instead of panicking after printing its checksum.
