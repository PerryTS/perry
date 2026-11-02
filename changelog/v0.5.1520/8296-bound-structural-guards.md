### perf(codegen): skip unbounded guards for bounded bodies

Perry no longer walks runtime-sized arrays, maps, sets, or recursive object
graphs to enter a function whose own work is statically bounded. This removes
the costly structural parameter guards from the interpreter benchmark's
`peek` and `asNum` helpers while retaining fixed-size object guards and guards
for looping consumers that can amortize validation. Fixes #8202.
