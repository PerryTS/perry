Plain-array `a[a.length] = value` now performs GC scan-layout transitions inline.
The first pointer in a pointer-free array and the first scalar in an all-pointer
array no longer require the generic append helper. Unique heap strings become
shared before storage; read-only shared image strings are never written.
The existing slot barrier still precedes publication of the new length.

Capacity growth remains out of line and geometric. Growth copies only the live
dense prefix, including when a bulk append grows a partially filled array, and
hole-initializes all remaining capacity. Mixed values, holes, array integrity,
length accessors, subclasses, named properties, sparse arrays and RHS mutation
are covered by the new GC gap test.
