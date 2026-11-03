Program facts are solved over the whole program: one solve over every
module, where a function body contributes only once the program can run
it. The per-unit solve (one solve per module, contracts exchanged between
the units to a fixed point) is retired; `PERRY_FACTS_UNITS=1` keeps it
reachable for its tests until it is deleted. On tsc-sized programs the
facts pass is about 10x faster (the TypeScript compiler's sources: 652 s
per unit, 32 s whole, on the same host) and proves more sites (22,792
against 20,644); small programs lose only proofs in functions the program
never runs.
