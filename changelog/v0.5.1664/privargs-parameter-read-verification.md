Add Uint8Array, Buffer and Uint32Array parameter witnesses with unrelated
aliasing views, invalid indices and receivers passed through typed annotations.
Current main already admits these reads from each receiver's kind and storage
header without a global aliasing latch; no additional read mechanism is added.

Extend the construct harness with Buffer, Effect, sloppy .cts parity, semantic
negative controls and a copying-GC witness. Independent build arms report
five interleaved instruction/RSS trials, emitted text and collection counts
for the real drivers and all four class micros with huge pages disabled.
