`x instanceof F` now throws a TypeError when `F.prototype` is not an object
and `x` is an object, as OrdinaryHasInstance requires; it returned `false`.
A function whose `prototype` was never materialized still answers `false`.
