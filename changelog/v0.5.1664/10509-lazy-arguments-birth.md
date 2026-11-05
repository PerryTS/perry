Defer Arguments object birth until its binding is read or captured. Direct
calls and conditional function-object dispatch retain a managed argument
snapshot; the body creates the existing fixed-layout, shape-stamped object
when execution observes it. An unconditional first observation retains direct
runtime object birth, avoiding an intermediate Array. Unexecuted branches avoid object allocation, snapshot
copying, mapped-index setup and descriptor work. Identity, strict callee
restrictions, mapped aliases, reassignment and escaping captures are preserved.
Inline constructor entries reset the binding on each execution, and iterator
state materializes it when retaining the parameters.

The existing argument Array snapshot remains. Removing that packing requires
statepoint support for native argument snapshots: the current root lowering
rejects escaped addresses of mutable root slots. No root registry, cache or
native-frame fallback is added here.

Validation includes strict/sloppy Node witnesses, retained closures,
constructor loops, generators, async reads, allocation-pressure checks,
codegen dominance coverage, and a copied-binary negative control that bypasses
object birth and exposes the argument Array.
