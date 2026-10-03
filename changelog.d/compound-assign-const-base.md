Compound member assignments through a `const` binding (`b.vx -= e`,
`t[k] += 1` with `const t` and `const k`) no longer copy the receiver into a
fresh temp. The copy was a second pointer-typed local, so each such statement
paid a string-addref test, a root-slot store and an incremental-mark root
shading gate for an object the binding already roots. n-body runs 20% fewer
instructions. Mutable receivers keep their evaluated-once snapshot.
