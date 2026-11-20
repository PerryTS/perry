Emit the existing Eden bump and limit check for exactly described class births
below 16 KiB with a visible constructor chain, even when a callback or
cross-module factory hides the caller's loop. Cold births with imported or
unresolved constructors retain runtime sizing and its learned widths. Empty
array literals now share the small-literal birth protocol, including
live GC birth color and four initialized hole slots for later appends. This
removes per-birth runtime constructor and layout-reset work without adding a
cache or side table; larger allocations retain their existing admission rule.

Cover indirect class factories, instance identity and inherited slots, empty
array identity, alias-preserving growth, initialized slack, and allocation
pressure across explicit full collections in gap tests. Emitted-IR tests assert
that the bump mechanism is live.

Share class and empty-array initialization, overflow allocation, and live GC
birth color in one out-of-line body per kind. Callers keep the Eden bump,
limit check and base header store; PreserveMost carries the same ABI through
definitions, split declarations, calls and EH invokes. Existing class header
images remain the size/shape authority. No compile tier or mutable cache is
introduced. Tests cover shared bodies, GC-leaf fast calls, live color, initialized
slots before seeding, and the native LLVM calling convention.
