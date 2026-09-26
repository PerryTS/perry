Polymorphic key-adding stores are now served inline. A store site that holds
several key-add memos (one per receiver pre-shape, e.g. a base-class
constructor seeing each subclass) places each further memo at its pre-shape's
home way, a hash of the ShapeId, and the emitted hit compares that one way
after the primary memo: one extra compare whatever the number of shapes. The
key-add memo is also compared before the existing-key ways, the per-object
header checks are one test on the hot path, and a key-add on a typed-layout
receiver (an object literal) no longer takes the typed-feedback registry lock
when typed feedback is off, which cut about 180 instructions from each such
add.
A memo the runtime serves from beyond the two ways the emitted hit compares
(its home was taken by an earlier, often transient, shape) now moves into one
of them, so a site's hot shapes end up served inline; on tsc this cut
runtime-served key-adds per transpile from 406,464 to 152,424.
