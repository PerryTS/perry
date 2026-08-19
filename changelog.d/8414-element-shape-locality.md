Speed up loops over contained class-instance arrays by carrying their proven
numeric field layout into direct `array[i].field` reads. Specialized loops now
skip redundant per-object header and shape checks when runtime parameter guards
and the complete reachable-store proof establish the exact raw-f64 layout; the
generic fallback remains guarded. On the `churn` corpus row this reduces retired
instructions by 8.8% and median wall time by 30%, putting Perry ahead of Node
without increasing RSS.
