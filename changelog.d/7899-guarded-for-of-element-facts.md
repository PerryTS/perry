Fixed the representation-selection census regression introduced when proven
array `for…of` loops gained a runtime iterator-patch guard. The guarded loop's
byte-identical index arm now retains its element-shape facts when it is the
last use of the array and its producer/reader objects, restoring the liveness
fixture from zero to three selected and consumed `Ptr<Shape>` locals.

The lazy iterator arm remains an arbitrary escape: a patched iterator can
reshape the array's elements before returning. Facts therefore never cross
the guard's join, apply to a group member reused in the lazy arm, or survive a
nested guard with a backedge. Focused tests sabotage both the positive
exception and the post-join boundary, while the existing end-to-end element
gap test remains byte-exact with Node.
