De-duplicated `test-parity/gc_repsel_corpus.txt`: `test_gap_repsel_proven_this_frozen`
was registered twice (#6977 added it in two places), so the file held 23 entries with
22 unique names.

The duplicate was not cosmetic — it ran the same file twice per arm, inflating every
`gc_repsel_matrix.sh` count by 20 cells (one per arm) and skewing the liveness
denominators the matrix reports ("collected N/23", "moved-objects N/23"). Conclusions
drawn from the affected runs were unaffected, but the totals were not comparable to
runs taken before the duplication.
