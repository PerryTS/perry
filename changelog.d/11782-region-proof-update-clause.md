Fixed a miscompile in numeric loop regions: a loop-carried local that the
loop's `for` update clause or condition rewrote (for example
`for (...; i++, s = "a") { o.x = o.x + s; }`) kept its Number proof, because
the proof only judged writes in the loop body and the entry test runs once
before the loop. The region then added the string's bits as a double and
stored the result, a live string pointer, into the pointer-free F64 field
(`o.x` printed `a` instead of `1aa`). The proof now judges the condition's and
update's writes with the body's, so such a local takes the generic route.
