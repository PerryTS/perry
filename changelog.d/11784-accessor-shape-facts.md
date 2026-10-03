Class getters and setters are answered from shape facts at the read and
store sites (#10498). A site that inherits a compiled class accessor checks
the receiver's ShapeId (the key is not own, and the prototype identity names
the holder), the holder's ShapeId (the key is still an accessor lane) and the
lane's value against the accessor pair it primed, then calls the compiled
getter or setter directly: inline in the emitted read and store towers, and
first thing in the collecting miss entries. The class-registry link from a
class to its declared prototype is written once; a replacement or a
generic-origin redirect retires the displaced prototype's ShapeId, so the
accessor read and setter sites no longer compare the global
`class_lookup_surface_generation`, `proto_validity` or `vtable_generation`
words. getter_read2 1,540 -> 223 instructions per iteration, setter_write2
1,207 -> 313, setter_ctor 3.6x -> 1.5x field_ctor.
