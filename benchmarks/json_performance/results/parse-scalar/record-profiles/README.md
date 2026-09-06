These are separate three-second samples of the preceding tape-scan worker,
not timings of the scalar-parse candidate. The shared M1 quiet gate passes;
both profiled workers and both sample commands exit successfully. The worker
hash is `21a1e75beb47e2a12ae066d4991abf7878e48c297b3de55bd5514077f209e7e5`.

The 1 MB record-object capture has 2,276 main-thread samples. Its flat top-of-
stack counts include 289 in a shape-element closure, 228 in the shape-element
emitter, 244 in string escaping, 284 in memmove, 158 in array emission and 127
in the own-key toJSON probe. These are sampling counts, not isolated costs
or expected speedups. They motivate inspecting per-field traversal, root
handle re-reads and callback-free nested primitive arrays.

The small-record capture additionally shows repeated shape-prefix building
(154 top-of-stack samples), UTF-8 validation (111), and shape slot-count lookup
(99). It includes 295 dyld-start samples, so it should not be treated as a clean
steady-state percentage decomposition. The template is rebuilt for each
standalone stringify call; its setup is a candidate for elimination on plain
records. Any replacement must preserve descriptors, key order, prototype
changes, cycles and callbacks.
