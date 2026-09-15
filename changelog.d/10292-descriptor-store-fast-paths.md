A receiver that carries a property descriptor keeps the object-store fast paths
for its other keys, and a class-less receiver whose prototype was set explicitly
(`new F()`, `Object.create`, `setPrototypeOf`) is vetted per key instead of
being rejected wholesale. `Object.defineProperty` adding a new key now reuses
the learned shape transition.

Descriptor transitions also share their successor shape. The generation that
gives a descriptor install or removal its identity is now a pure function of
the transition — predecessor shape, key and attributes — so every receiver
that repeats the same install lands on the same successor instead of minting a
private one. The per-key descriptor summary is consulted exactly: a set Bloom
bit is confirmed against the descriptor tables rather than surrendering the
fast path, because a store that falls back appends to a private keys array and
takes the receiver off the shared transition chain permanently, costing every
later store on that object as well.

Building 5,000 objects that define one non-enumerable property and then assign
60 methods — zod v4's schema constructor shape — drops from 13.9-18.7 billion
instructions and ~255 MB to 1.2-4.4 billion and ~44 MB. Constructing 300 real
zod v4 `z.object` schemas drops from 21.9 to 12.7 billion instructions
(-41.8%); the same fixture without a descriptor is unchanged.
