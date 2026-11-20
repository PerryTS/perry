A `for` whose int32 counter needs no exit check (`i++` under `i < n`) and whose
body indexes a fixed-length view by the counter now gets the same one-time
entry check as the exit-checked counters: counter and bound on their actual
values, plus the bound against the view length. The copy lowered under a
passing check has no per-access branch; the other copy keeps it. A loop with
no counter-indexed view is not copied. An exit-checked counter whose start is
already past its bound skips the loop before the entry check. Tests:
`expr::buffer_bounds_source_tests`, `test_gap_counter_entry_check_adversarial`.
