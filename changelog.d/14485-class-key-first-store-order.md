A class's keys are now laid out in the order its constructor first stores
them, whether the store is at top level or inside a branch, loop or `try`.
A key stored only in the arms of an `if`/`else` used to be placed after every
key stored at top level, so a constructor that stores it before a later
top-level key (the `semver` package's `SemVer`, `prerelease` then `build`)
gave every object a 544-byte overflow table; those objects now keep their
birth layout, and enumeration order is unchanged.
Stores in an `if` arm that leaves the constructor early (`return`, `throw`) no
longer fix a key's position: the order is that of the path that reaches the end
of the constructor (semver's `Range`, built from a `Comparator`), and keys only
such an arm stores follow. A constructor-stored key is no longer reported as
always present when a run can skip its store.
