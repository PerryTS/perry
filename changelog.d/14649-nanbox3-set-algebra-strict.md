The ES2025 Set methods (`union`, `intersection`, `difference`,
`symmetricDifference`, `isSubsetOf`, `isSupersetOf`, `isDisjointFrom`) decode
their `other` argument by its tag alone. A word that was not NaN-boxed as an
object was read as a SetHeader address, so an address-shaped Number (a denormal
double, also a negative one) crashed with SIGSEGV, and every other primitive
was taken as an empty Set. A primitive `other` now throws the TypeError of
GetSetRecord, as in Node ("Set.prototype.union argument must be an object").
