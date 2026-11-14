codegen: a method whose index parameter is a proven Number (an `F64` slot of
its whole-program signature: a counter bounded by a length, a value a fraction
can reach) gets its typed body in place of its guarded index clones
(`$idx_u31_N`, `$generic`). The typed body converts the key exactly at each
element access, which folds to the integer compare when the caller passes an
int, and takes and returns raw values. An at-rest read with a default
(`a[k] ?? <int32>`) takes its i32 form for such a key too. A parameter that
may hold a non-Number (`undefined` from an Array with holes, a string through
an untyped caller) keeps the clones: only their guard proves a Number.
Two accessor loops over plain Arrays: 2 220.8 -> 1 975.8 M and 3 461.2 ->
2 851.2 M instructions (-11.0 %, -17.6 %); the chess engine's `at` is typed
(search -0.010 %). `PERRY_SIG_NUMBER_INDEX=0` keeps the clones,
`PERRY_REST_DEFAULT_NUMBER_KEY=0` keeps the double default read.
