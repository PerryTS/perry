Named property stores can answer existing own slots and inherited key-add
proofs from the receiver shape, including receivers at megamorphic sites.
Prototype descriptor lookup uses the existing shared key index instead of
repeatedly scanning builtin prototype keys. Shape-owned proofs are retired by
the existing prototype and descriptor mutation machinery.

Array and function named-property stores project onto their existing bags.
Array length/index keys and function intrinsic keys retain descriptor paths.
Function prototype stores use the ordinary descriptor walk so a readonly
inherited key makes Reflect.set return false.

Saturated store sites use each receiver shape's own writable binding without
adding a site memo. Function bags admit the matched writable property even
when another intrinsic is readonly. Finite Number stores reuse the emitted
slot-write admission; SPECIAL values retain the representation funnel.
Receiver-shape overwrites run before the existing add-site memo.
Runtime stores use the emitted receiver-shape kind proof before allocator
classification; ordinary overwrites and transitions avoid the repeated header
probe. Array and function projections retain their header checks.
Canonical String indices on arrays use the same existing dense own-element
proof as Number indices; holes, growth, descriptors and length keep Set.
An Any own slot reuses the alias/barrier funnel after its representation and
numeric-prefix facts are answered by the shape, avoiding their second probe.
Plain-object additions reuse the existing null/default prototype answer rather
than recording a second inherited-store fact. Custom and class chains retain
their shape-owned facts. Recreated function data keys clear the existing deleted
key marker together with publishing the new bag binding.
