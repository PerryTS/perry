# Bounded polymorphic write PIC follow-up

The write PIC now has two bounded shape entries. The second entry is consulted
only after the first entry has been primed; all existing mutable receiver and
slot guards remain on both hit paths. Measurements below are three alternating
Node/Perry samples with matching checksums.

| Cell | Node median | Perry median | Writes | Checksum |
| --- | ---: | ---: | ---: | ---: |
| `shape_monomorphic` | 133 ms | 123 ms | 120,000,000 | 122,876,400 |
| `shape_two` | 133 ms | 667 ms | 96,000,000 | 98,876,400 |
| `shape_four` | 110 ms | 2,978 ms | 60,000,000 | 62,876,400 |

The two-shape case improves substantially over the prior monomorphic-cache
fallback (~5.3 s), while four-shape receivers remain intentionally outside the
two-entry bound for a later measured extension.
