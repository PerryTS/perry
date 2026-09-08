**String spreading keeps its character array valid across moving collections**
(#9983). The builder now copies source bytes out of the moving heap before it
allocates character strings, roots the result array, and re-reads its current
head for every element store. Previously a collection during a non-ASCII
character allocation left both the source-byte pointer and the result-array
pointer stale, which produced the late unenumerated string slot seen in the
compiled claude-code bundle.
