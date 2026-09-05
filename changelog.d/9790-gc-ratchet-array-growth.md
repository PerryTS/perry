GC Ratchet now applies traced-counter determinism checks where its documented
probe overrides are available, preserving the full measurement report. Record
why array-growth block placement makes two byte counters informational while
keeping the probe's correctness, retention, cycle and object counts gated.
