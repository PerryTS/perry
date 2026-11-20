`test_gap_gc_call_argument_rooting` shipped with #7252 but was never added to
`test-parity/gc_repsel_corpus.txt`, so it never ran anywhere. Registering it is
the whole fix.

This is the third occurrence of the same mistake (#7192 and #7216 are documented
in that file directly above), and the first one caught automatically: the
`test_gap_gc_*` enforcement added in response to the earlier two failed the
`GC Moving Witnesses` job on its first main-line run — which only happened at
all because #7253 gave that matrix a main-line trigger.
