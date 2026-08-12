**`--opt-report` now names the module-wide `Ptr<NumArray>` kill instead of going silent** (#7112, whole-analysis half).

`collect_num_array_locals` returns an empty map outright when the module has a shape-barrier site, a numarray prototype-index barrier, or an opaque prototype mutation — and when the analysis is switched off entirely. That happens **before** any array local becomes a candidate, so no per-value `deny()` runs and the report shows the module as having no array candidates at all.

That reads identically to "the analysis never looked at these values", which is precisely the ambiguity the report exists to remove — and it is why 8 of the census's 18 real workloads read "zero candidates".

The kill is now recorded once, naming which of the four causes fired:

```
<module> | rule 0 (analysis disabled)
        | cause: an opaque (aliased) prototype mutation in this module
        | tier: compiler-limitation | issue: #7112
```

Verified both directions on a two-module pair: a module containing `const p = Array.prototype; p[5] = 1` alongside a numeric array emits exactly one `rule 0` entry with that cause; the same module without the prototype write emits **zero**. So the entry means "the analysis stopped upstream", not "this module has arrays".

Two deliberate choices worth stating:

* It is attributed to a pseudo-name `<module>` with no `local_id`, rather than to some array local. A module-wide kill has no value to blame, and inventing one would put a denial on an array that was never examined — the same misattribution in the other direction.
* The four causes are now separate `if` arms rather than one `||` chain, purely so the report can say *which* barrier fired. The chain's alias-hole rationale (why `has_opaque_prototype_mutation` exists at all, given the direct-form kill above it) moves onto the arm that owns it rather than being left orphaned above a condition it no longer describes.

**The element-type pre-filter is covered too**, which is the case the issue leads with. `benchmarks/suite/11_prime_sieve.ts` reported **0** `ptr-numarray` candidates — reading as "this program has no array worth promoting", for a program whose 1,000,000-element array is in the hot loop indexed by a proven integer. It now reports:

```
<array local> | rule 0 (element type) | declared type: Array(Boolean) | tier: fixable
```

`Fixable`, not `CompilerLimitation`: the declared element type is the trust boundary every numeric fast path uses, so the program can move to `number[]` (or the analysis can grow a boolean slot representation). That is a different answer from "the compiler cannot express this".

Only ARRAY types are recorded. A non-array `Let` is not a rejected array candidate — it simply is not an array — and filing a denial for every `let x = 1` would drown the report in rows answering a question nobody asked. Verified on a control module: a `number[]` is reported `selected`, and the `const flag = 1` / `const label = "x"` beside it produce **zero** element-type rows.

Still untouched: the `Ptr<Shape>` side, whose provenance pass has the analogous structure.

`cargo test -p perry-codegen --lib` 851 passed / 0 failed; fmt and file-size clean.
