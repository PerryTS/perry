### Fixed: iterator own properties were write-only through the by-name GET; `it.next = undefined` silently ran the builtin (follow-up to #9066, PR #9075)

The Map/Set-iterator arm in the by-name GET tail returned `undefined` for every non-`next` key without consulting own fields, so the #9066 reserved-floor storage was write-only through that lane — user properties stored past the floor (and hole-squeeze survivors) read back `undefined` while their values sat intact in the overflow spill. The arm moved to `accessors::map_set_iterator_property` with own-field shadowing first (ordinary [[Get]] order, so an own `return` patch also shadows the synthetic bound method).

Also per review: an own `next` explicitly assigned `undefined` is present-but-non-callable and now throws per IteratorNext (bytes-based presence scan, no allocation on the unpatched hot path), and a failed reserved-floor seed drops the write instead of proceeding unseeded onto the backing-collection field.

Validated byte-for-byte against the pinned Node 26.5.1 oracle (16 gap cases, including the reviewer's 12-add/10-delete survivor shape) plus 6 `reserved_floor` unit tests.
