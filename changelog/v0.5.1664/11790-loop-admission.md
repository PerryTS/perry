**perf: loop regions admit real loop bodies over arrays (#10741)**

The loop tiers admitted only single-statement, call-free bodies, so a
multi-statement, branching loop over arrays got none of the region win: every
element access ran its full guarded tier. The issue's particle step (`x[i] +=
vx[i]; ...` over 400 elements) cost 995 instructions per step over `number[]`
(19x node) and 661 over `Float64Array` (10x node).

Loop regions (#11680) now take these loops:

- An index is proven when it is the loop COUNTER of `for (...; i < B; i++)`,
  with every write of `i` and `B` accounted for (the update writes `i`;
  nothing else in the body, condition or update writes `i` or `B`), or a
  body-local copy of it (the compound-assignment spill of `a[i] += v` makes
  two). The guard checks the entry value of `i` and `B <= length` once.
- An array the region stores into, or reads by the counter in a Number
  context, is guarded as a dense raw-f64 array (or, unless it is declared a
  plain Array, an owning `Float64Array`): a bare read is one `load double`
  typed as a Number (a typed slot's NaN is canonicalised), and an element
  store of a value proven a canonical double is one `store double`.
- Pure `Math.*` over primitives no longer stales the region's facts.
- A statement that may run JS (a call) no longer refuses the loop: F-body sets
  the region's dirty flag right after it, the accesses after it in that
  iteration take the guarded tier, and the next iteration re-checks.
