A receiver that carries a property descriptor keeps the object-store fast paths
for its other keys, and a class-less receiver whose prototype was set explicitly
(`new F()`, `Object.create`, `setPrototypeOf`) is vetted per key instead of
being rejected wholesale. `Object.defineProperty` adding a new key now reuses
the learned shape transition, and a data-descriptor install shares one semantic
shape generation with every receiver that repeats it. Building 5,000 objects
that define one non-enumerable property and then assign 60 methods — zod v4's
schema constructor shape — drops from 13.9–18.7 billion instructions and
~255 MB to 1.2–4.4 billion and ~44 MB.
