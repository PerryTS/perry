### Performance

Named-property writes to object spill slots now skip the full GC layout update
when an overwrite keeps the same pointer kind. Scalar-to-pointer and
pointer-to-scalar transitions still update the slot mask, while repeated
numeric and pointer writes avoid the redundant bookkeeping.
