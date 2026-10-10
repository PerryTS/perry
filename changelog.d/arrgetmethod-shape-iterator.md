Array for-of admission now acquires refused IteratorRecords directly from the
receiver's own symbol slot or Array.prototype's slot, with the same receiver
and owner facts used by the entry proof. The entry captures the iterator and
next once, and the stack dispatcher no longer repeats GetIterator.

Array GetIterator shares that shape-slot reader. Accessor descriptors call their
getter with the original array receiver, and methods use the existing
explicit-this dispatcher. Non-callable methods and primitive iterator results
throw TypeError; array-valued iterator results retain their own next property
instead of being converted to an intrinsic iterator.

Coverage includes own/prototype overrides, patched next, descriptor getters,
non-callables and non-object results, mid-loop edits, and moving collections
inside the getter, factory and next getter. The generated-code checks require
one rooted range and one acquisition at entry.

Native iterator next capture now shares the receiver/prototype edge proof with
intrinsic Step and reads the actual next descriptor, including replacements and
accessors. Entry retains one iterator handle across that read instead of opening
a second handle scope.
