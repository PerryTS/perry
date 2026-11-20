**String spreading keeps its character array valid across moving collections**
(#9983). `js_string_to_char_array` now copies the source bytes out of the moving
heap before it allocates the character strings, and it keeps the result array in
a runtime handle, using the head returned by `across_mut` for each element store.
Before this, a collection during the allocation of a non-ASCII character left the
source-byte slice and the result-array pointer stale. That produced the
unenumerated array slot that `PERRY_GC_VERIFY_MARK` reported on the compiled
claude-code bundle.
