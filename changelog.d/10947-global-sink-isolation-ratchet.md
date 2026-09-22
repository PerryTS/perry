Ratcheted bare process-global statics that tests assert on (#10944).

`scripts/global_sink_isolation.py` gains a second rule, recorded as a baseline
that may only shrink — the same shape as `raw_handle_debt.py` and
`unrooted_local_shape.py`, including their merge-base half.

The problem is measured, not suspected:

```
--test-threads=1   4215 passed;  0 failed
parallel (x6)      4198-4205 passed; 10-17 failed, a DIFFERENT set each run
```

Zero of those are genuine failures — every one is a test's assertion disturbed
by another test's increment, always off by exactly one. But the population is
not enumerable by inspection: six parallel runs *after* three modules were
converted still produced 17 distinct names, some of which no earlier run had
shown. Converting them all at once would mean editing modules owned by several
lanes, so today's 62 are recorded and only **additions** fail. Each entry gets
converted by whoever owns the file, and no new instance arrives quietly.

The existing rule covers tables the GC guards clear; this one covers the much
larger class the module docs already argue for — "a new sink cannot be added
quietly, and a new *reader* never has to remember anything."
