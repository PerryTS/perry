### Fix oversized code from nested loop unrolling

Bound HIR loop expansion by the total copied statements and expressions,
including previously expanded inner loops. Large formatter bodies now remain
loops under the default size optimization instead of growing dramatically when
`+= 1` is spelled `++` (#13649). Small fixed-trip loops remain eligible for
constant-index specialization.
