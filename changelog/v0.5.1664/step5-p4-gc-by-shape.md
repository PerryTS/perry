GC (charter step 5, P4 flip): the collector traces every object by its
shape. The shape record's `rep` word selects the slots to skip (the F64
lanes), and nothing else is read: no per-object layout state or mask, and no
shared shape mask. The `PERRY_GC_BY_SHAPE` switch and its check modes are
gone. A debug build verifies every selection and aborts on an F64 lane that
holds a pointer.
