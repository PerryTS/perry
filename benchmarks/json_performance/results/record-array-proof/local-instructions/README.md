Local instruction diagnostic only; no quiet-host CPU or RSS acceptance.

36 fresh processes: three per arm across six stringify fixtures, fixed work,
random engine order and default GC. Instruction counts include process startup
and warmup. Both arms use the same worker.o. All processes exited successfully.

Relative to the qualified record release, median process instructions change
by -5.62% for 16 KB records, -2.03% for small records, -5.50% for 1 MB records,
-1.71% for 20 MB records, -0.06% for numbers and -0.05% for heterogeneous data.
The new helper removes repeated primitive-array validation after record-wide
preflight. These counters show reduced work on the intended path; they do not
establish CPU parity, RSS parity or no-regression acceptance.
