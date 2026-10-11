Reuse the allocator generation proof and the incremental-mark gate when storing
into a forwarding-resolved array. Young arena parents with no active mark owe
no barrier work. Old/malloc parents and stores during incremental marking keep
the shared slot barrier. The continuation is shared across append and resolved
array stores; generated call sites gain no inline guard sequence.

Tests cover old-to-young remembering and shading a child of a young array
during incremental marking.
