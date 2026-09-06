Local stack diagnostics on the qualified empty-object worker, with default GC.
These samples are not quiet-window CPU or memory evidence. All three workers
and samplers exited successfully; metadata records their commands and hash.

Wide-object stringify: 1998 of 2252 main-thread samples enter stringify.
The key/field access closures account for 417 and 150 top-of-stack samples;
write_escaped_string accounts for 445 and the object walker itself for 330.
This supports eliminating repeated rooted retrieval after primitive-field
validation. The key escaper and copying remain material costs.

Heterogeneous stringify: 2081 of 2248 main-thread samples enter stringify;
537 top-of-stack samples are in the object walker, 207/91 in its access
closures, and 297 in write_escaped_string. This also directs attention to
object traversal, although complex fields must keep callback-safe rooting.

Wide-object parse: 1730 of 2215 main-thread samples enter the moving-GC
safepoint from the benchmark loop. This is sampling evidence of a major GC
contribution, not an exact CPU percentage. JSON parser changes alone cannot
be assumed to eliminate the full end-to-end gap; GC production policy stays
unchanged in this work.
