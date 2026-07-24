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
| `shape_four` | 110 ms | 461 ms | 60,000,000 | 62,876,400 |

The two-shape case improves substantially over the prior monomorphic-cache
fallback (~5.3 s). The four-entry extension beats the prior four-shape
fallback (~3.0 s) while preserving exact parity.

The final 15-pair raw samples were Node
`[108, 110, 110, 109, 113, 109, 109, 110, 108, 110, 113, 108, 109, 111, 111]`
and Perry
`[461, 464, 461, 461, 463, 472, 460, 492, 459, 461, 459, 460, 460, 460, 463]`.
