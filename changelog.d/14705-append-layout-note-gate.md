Appending to a dense array (`a[i] = v` one past the end, e.g. a `boolean[]` fill loop) no
longer calls the GC slot-layout note on every element: the call now runs only when the
stored value may be a heap pointer or the array's layout bits say the note can act. On
`11_prime_sieve` the note drops from ~22% of samples to none (#13491).
