Keep malloc-backed GC parents' young edges remembered regardless of whether
a store caller labels its slot external. Large-capture closures previously
skipped newborn barriers because malloc headers are never tenured; generated
stores had the same gate. Both gates now skip only untenured arena parents,
while retaining the incremental-marking clause. Remembered entries for malloc
parents use the external (page, owner) representation so minor collections can
find and rewrite them. Deterministic witnesses cover closure birth and generated
stores across an evacuating minor that leaves the parent otherwise untraced;
restoring either defective gate or runtime classification makes them fail.
