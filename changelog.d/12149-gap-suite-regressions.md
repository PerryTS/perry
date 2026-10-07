Fixes the gap-suite regressions on main. Static private class members read
from the private namespace again, and a static `#x` no longer leaks as a public
property. A plain number or a subnormal double is no longer read as a heap
pointer (an `fs` crash and a `Number#toString` crash). Function property delete,
native-base `super`, Intl subclass fields, `util.inspect` with a negative depth,
an inherited `toLocaleString`, Temporal `constructor` and CJS
`exports.undefined` now match node. `test_gap_iterret_generator_prototype` is
recorded against #12148.
