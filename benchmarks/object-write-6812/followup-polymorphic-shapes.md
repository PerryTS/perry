# Bounded polymorphic write PIC follow-up

The write PIC now has four bounded shape entries. Later entries are consulted
only after earlier entries have been primed; all existing mutable receiver and
slot guards remain on every hit path. Measurements below are three alternating
Node/Perry samples with matching checksums from the two-entry implementation;
the four-entry extension has an additional correctness-only parity run below.

| Cell | Node median | Perry median | Writes | Checksum |
| --- | ---: | ---: | ---: | ---: |
| `shape_monomorphic` | 133 ms | 123 ms | 120,000,000 | 122,876,400 |
| `shape_two` | 133 ms | 667 ms | 96,000,000 | 98,876,400 |
| `shape_four` | 110 ms | 2,978 ms | 60,000,000 | 62,876,400 |

The two-shape case improves substantially over the prior monomorphic-cache
fallback (~5.3 s). The four-entry extension also produced exact parity in a
correctness run (`shape_four`: 60,000,000 writes, checksum 62,876,400); a
fresh release-mode timing sweep is still required before claiming its speedup.
