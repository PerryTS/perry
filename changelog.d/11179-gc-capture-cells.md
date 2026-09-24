Move mutable capture cells into the GC arena (`GC_TYPE_BOX`, plus pointer-free i32/bool cell kinds). Closure captures and generated frame roots now retain and relocate cells; NaN-box cells trace their contained value and setters maintain generational write barriers. Remove the per-cell malloc allocations, pre-sized box registries, registry scans, pointer caches, capture-count tables, and manual scope-release machinery.

Root incoming parameters before allocating their cells, including closures and inline constructors, and root cached capture addresses across safepoints. Mapped arguments trace their cells as owner edges and rekey metadata before copied-object traversal. Async generation tokens remain separate from GC cell lifetime.

Add relocation, old-to-young barrier, unreachable-cycle, mapped-arguments, and compiled closure/async regression coverage. No version bump.
