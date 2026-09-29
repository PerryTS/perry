Typed-array element updates (`ta[i]++`, `ta[i]--`, `++ta[i]`, `--ta[i]`) now
take the guarded inline element read and store that the explicit
`ta[i] = ta[i] - 1` uses, with one inline IEEE step for a Number element,
instead of four runtime calls per update. BigInt elements and receivers that
are not typed arrays keep `ToNumeric` and the numeric step. fannkuch: -21.9%
instructions. A number-context read of a tracked typed array whose index is not
proven in bounds now converts the out-of-bounds `undefined` to NaN inline
instead of calling `js_number_coerce` per element, and a function's specialized
entry is chosen by how many raw slots it delivers across all its call sites
rather than by the most frequent exact tuple, so `Atu(n, w, v)` and
`Atu(n, w, u)` share one entry. spectral-norm (typed): -15.4% instructions.
