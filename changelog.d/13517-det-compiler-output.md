Compiling the same program twice now emits the same code. The private generator
frame numbered the bindings of an inlined resume body in the iteration order of a
hash set seeded per process, which changed the capture slot offsets, the IR and the
register allocation of the user code from one compile to the next (43-generators
alternated between 1.71 G and 2.43 G cycles). The bindings are now numbered in
ascending original-id order. Add a test that compiles a generator program several
times and requires identical IR.
