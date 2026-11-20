Arrays now keep the slots past their length filled with the hole sentinel:
`pop` (inline and runtime), `splice`, `shift` and every other shrinking path
write it, and allocation fills the whole capacity. A popped element no longer
stays behind in the vacated slot, where it was retained past its lifetime and
could be read back after a later `length` extension. Debug builds (and release
builds with `PERRY_GC_VERIFY_ARRAY_HOLES=1`) verify the invariant at every
collection.

An array whose backing store would not fit the GC header's 32-bit size field
(more than 536,870,909 slots) now raises `RangeError: Invalid array length`
before allocating, instead of recording a truncated size.
