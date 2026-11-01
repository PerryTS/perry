for-of over an array runs the indexed loop (#13645). A counted iterator
record whose entry picks the array arm now lowers that arm as its own
`for (let i = 0; i < a.length; i++) { const x = a[i]; ... }` copy through the
ordinary counted-loop chain: the source's declared element type picks the
read's lowering and the read's guards prove it, the binding reads the element
directly, and completion state, cursor and release stay the record's. The
protocol arm keeps the record loop. `GetMethod(%ArrayIteratorPrototype%,
"return")` on a close the facts do not prove absent asks the shape-validated
read-site memo before scanning the chain. Repro (5M probes): for-of 415 ms ->
275 ms, indexed 290 ms.
