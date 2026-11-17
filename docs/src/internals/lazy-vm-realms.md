# Lazy vm realms

Each `vm` context owns a global object and a separate record of its intrinsic
constructors and prototypes. Contexts do not share a cached realm. The same
family birth code serves the main realm and every vm realm.

Realm setup binds constructors. Object and Function are root families and exist by
setup; the other families keep an unborn constructor until an observation needs
its members or prototype. Presence and key listing read declared static keys
without forcing that birth. Number's parse helpers and the corresponding global
bindings share their realm's function cells.

A built-in constructor carries its immutable identity in its header and its
realm global in capture 1. Main constructors permanently keep capture 1 empty, so they
can precede the main global. A vm constructor keeps its own global reachable;
the global's private intrinsic slot reaches the GC-traced realm record. There is
no registry keyed by realm. Built-ins cloned to another thread are born into the
destination thread's main realm, without copying the source realm graph.

Main-only prototype identity words, Object roots, Function pointers and generator
towers belong to the thread's main realm. A vm family's birth updates its own
record. TypedArray's Array toString alias reads that same record explicitly.

After realm setup, vm context setup removes `process` and binds the shared main
Buffer, preserving Perry's Buffer behavior. Reading this binding does not birth
a vm Buffer family. Runtime builtin thunks retain their existing implicit main
realm behavior; script evaluation continues to relink results to the vm realm.

The runtime unit tests cover separate contexts, lazy birth, realm-local prototype
chains, cross-realm instanceof, GC relocation, constructor captures and Buffer
identity. The vm cross-context parity cases compare the observable behavior
against Node.

A prototype without a declared parent inherits its record's Object prototype.
The canonical DEFAULT shape already expresses the main Object parent, so birth
writes a physical link only when the selected parent differs from that default.
Iterator's root is born by the same family path and stored in its realm record;
the runtime's child iterator towers continue to use the main root.
