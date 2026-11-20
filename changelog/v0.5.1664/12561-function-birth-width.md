Ordinary compiled-function construction now uses the same keyless shape birth
allocator for first, changed-width and replay births. The existing saved
ShapeId is passed as a scalar proof, removing the separate replay allocator,
its repeated prototype-identity resolution and descriptor validation (#12561).

Width still comes from the live shape learner, including tracking and late
spill growth. A saved shape must match the actual prototype serial and chosen
width; collecting allocation keeps post-collection validation and prototype
shading. No cache, latch, table, synthetic identity or per-site emission is
added. Object.create retains its existing state model.

The prototype shape's existing weak relationship now resolves its complete
immutable birth facts once. Every reuse still checks record presence and the
actual prototype serial, and reads tracking and learned width from that live
record. Retirement resolves a new proof through the existing interner.
