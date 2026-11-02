**Indexed reads on an ordinary `Array` no longer re-prove a loop-invariant receiver on every element.**

An indexed read cost **87 instructions per element** — against 6 for the same arithmetic on a `Float64Array` and 16 for node — and none of it was a runtime call. 56 of the 87 were loop-invariant receiver revalidation re-executed every iteration: the NaN-box tag and handle-band test, the forwarding-flag follow, and a six-load live-head guard.

perry already had tiers that hoist that proof into the loop preheader. They were declining at one gate, `array_static_type_excluded` — a *declared static type* test in front of a tier that is otherwise fully runtime-guarded — so `const a: number[]` got it and plain `new Array(400)`, which infers `Array<any>`, did not. Ordinary JavaScript never reached the tier it already had.

Separately, `a[i] += 1` cost **948** instructions per element, 3.7× the identical `a[i] = a[i] + 1`, and no annotation helped: the compound-assignment spill temporaries were minted as `Type::Any`, erasing the receiver's array-ness and the index's integer-ness before codegen saw the statement.

Array read **87 → 13.5** (node 16.3), `a[i] += 1` **948 → 273**, `a[i] += b[i]` **1025 → 347**. A particle simulation over four numeric arrays spends **60.9% fewer instructions** and **59% less peak RSS**. The bare loop and both `Float64Array` paths are unchanged to the instruction.
