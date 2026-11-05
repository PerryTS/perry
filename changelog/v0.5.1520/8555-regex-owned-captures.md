**Fix moving-GC safety when materializing regular-expression matches.**

`RegExp.prototype.exec` and non-global `String.prototype.match` kept the
subject string payload and engine capture objects borrowed while allocating
their result arrays, capture strings, named groups, metadata, and `d`-flag
indices. A moving collection at any of those allocation points could relocate
the subject and leave the remaining captures reading retired nursery bytes.

Both the standard and fancy-regex paths now snapshot capture byte ranges and
UTF-16 metadata before the first runtime allocation, then copy each capture
from the current rooted subject with `string_copy_range`. Global
`String.prototype.match` uses the same range-based materialization. Allocation-
point GC regressions force the subject to move after matching and verify the
standard `exec` and fancy `String#match` results remain intact.
