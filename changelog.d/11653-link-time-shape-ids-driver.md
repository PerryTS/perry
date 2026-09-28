The compiler now assigns class shapes their ShapeIds before code generation.
A whole-program pass collects the content of every class birth (key names,
key count, live bound, class or literal prototype and, for a typed layout,
its masks) from the same code the module initializer uses, and gives each
distinct content one id in the reserved band, probing on collision. Each
module initializer passes its id to the shape mint. Typed class layouts carry
their static id and install their descriptor in each agent at module init, so
the process-wide typed-shape registry and the runtime rewrite of globals in importing
modules are gone. A cached object is reused only while the ids it
embeds are unchanged.
