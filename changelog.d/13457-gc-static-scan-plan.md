Static GC scan plans: an object born on a class or literal family layout is
scanned by the copying minor and the full mark from a plan stored in its layout
descriptor (`PerryLayoutDesc.gc_plan`, emitted by codegen from the family's slot
count and field representation) instead of a plan rebuilt from the shape for
every object. The walk is the plan's pointer-slot mask, the meta record and the
arguments tail; the shape's keys edge and the old-carrier notes are skipped for
a family record (a static record whose keys the shape table already roots), and a
minor skips the prototype word its young log already roots. A full mark still
emits the prototype identity once per trace. Other objects and descriptors
without a plan take the generic walk unchanged. In test and debug builds every
planned walk is cross-checked against the generic walk, and a planted mismatch is
caught.
