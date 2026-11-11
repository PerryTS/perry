A local object stored into a field of another local object, such as the `p`
in `const b = new Box(); put(b, new P(i, 1)); s += b.p.x` after `put` is
inlined, no longer goes to the heap when both die in the same block. When the
owner's construction is closed, the field is one the class defines with no
accessor, the field is stored once by a statement of the block and nothing
else touches the owner, later reads of the field are the stored value and the
store is removed, so the stored object is only read where it was created.
