**`a[i] += 1` reaches the same loop tier as `a[i] = a[i] + 1`.**

The two spellings are the same operation and node compiles both to the same cost. perry compiled them **11× apart** — 277 instructions per element against 24 — and the slow one was the idiomatic spelling.

HIR lowers a compound member assignment into two immutable alias `Let`s plus the store, so the base and the key are each evaluated exactly once and before the right-hand side. The classic range-loop matcher admits exactly ONE statement, so the lowering guaranteed the statement could never reach the tier. Annotating the array changed nothing: the obstacle is the statement count, not type information.

The temporaries stay. They are load-bearing — an RHS call can reassign the bindings they were read from, and the store must still land at the index evaluated before it ran. Instead the matcher folds them, and only for the guarded fast clones: the slow clone lowers the statements as written, so a failed guard and every side exit still execute the specified evaluation order. Inside the matched subset the fold is exact, because the body walk is a whitelist that admits no call, closure, `await`, update or assignment anywhere in the statement — nothing can write the locals the aliases read.

`a[i] += 1` **277 → 25.5**, `a[i] -= 1` **208 → 27.5**, `a[i] += b[i]` **347 → 35.9** (identical to `a[i] = a[i] + b[i]`), `a[i] *= 1` **206 → 25.5**, `a[i] |= 0` **236 → 52.5**. The bare loop, both `Float64Array` paths, the indexed read and write, and both expanded spellings are unchanged — their emitted LLVM IR is byte-identical.

This needs none of #10741's mid-iteration side-exit discipline: the folded-away statements perform no stores, so there is nothing to un-do when a guard fails partway. It also moves none of the five real programs in #10695 — their loop bodies are still multi-statement or contain calls, which no current tier admits.
