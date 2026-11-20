Native executables proven to have no Worker agents omit worker-termination
polls from their optimized loops. The proof uses the existing whole-program
Worker analysis and survives every LLVM unit; reusable libraries and
worker-capable programs retain their interrupts. This removes loop overhead
introduced by #12633 without weakening worker termination.

Scalar stores into pointer-free array and closure payloads return before the
outlined layout-transition helper, preserving element-shape and forwarding
maintenance while avoiding redundant transition calls introduced by #12797.
