# Selecting RegExp Unicode membership

Perex's `selectable-unicode` feature puts nontrivial Unicode property membership
behind an install-once provider slot. `js_runtime_install_regex_unicode` fills
that slot before module initialization. The default/full runtime installer also
fills it. The engine installer does not retain this database on its own.

The compiler retains the installer for Unicode-mode literals, non-ASCII patterns,
property escapes, Unicode escapes and named-group syntax. Every dynamic RegExp
construction retains it, including literal patterns whose flags are dynamic.
Escaping regex values and subclasses retain it because they expose constructors
and recompilation. String-to-RegExp coercion, exposed constructors, prototype `compile`, escaped global objects, reflective mutation, proxies and
first-class functions also retain it. Computed or extracted calls, dynamic
construction and reflective application retain it regardless of key annotations. Native modules, IPC,
workers, addons, plugins, UI and deferred dynamic code are open producers and
retain the database conservatively. These are whole-program unions collected
before code generation; the entry installer is part of the object cache input.

Omission is limited to programs whose remaining producers are ASCII literals
without those syntactic forms, consumed immediately by split, test or exec.
Retaining or passing a literal as a value retains the tables. Prototype
reflection and exposure also retain them, covering replacement of those methods. ASCII group validation returns before membership;
ordinary character ranges, whitespace classes and case-fold tables do not access
the property database. The selector deliberately over-includes some patterns. An opaque read of a data
container alone does not create a regex capability: every producer and escape
is scanned across the program, including native and dynamic code boundaries.
For search/match/matchAll coercion, an immediate primitive literal or a fresh
closed object with no conversion hooks can be checked structurally. An ordinary
object converts to `[object Object]`; its data fields do not supply the pattern.
Unknown arguments and conversion hooks retain the tables. This avoids treating
a user method taking a plain options object as an unknown pattern producer.
The rule uses actual HIR object construction and `Class::is_literal_shape`, not
TypeScript annotations or an assumed class name. It does not infer runtime
strings from annotations or mutable bindings.

Membership callers in Perex include both executors, atom filtering, candidate
construction, named-group parsing and binary-program validation. Compilation and
validation resolve the slot once, when they first need membership. The validated
Program view and binding witness retain one reference to an immutable provider table. Evaluator and atom queries
project its membership function once into their execution state. The single DFA
implementation is generic over its zero-sized provider, so membership calls
inline into character-class construction. Its cached entry selects the entire
instantiation before the search loops. Neither executor reads or checks the slot.
Property-free programs never resolve it. Property descriptors/name lookup, case-fold data and properties
of strings have separate references and remain available. A missing installation
panics during compilation/validation instead of returning an incorrect match;
the compiler must therefore retain installation for every open producer.
Native `perry-perex` clients accept arbitrary patterns and install unconditionally.

The slot holds only immutable native functions, never a GC value. Its selected
DFA entry table is retained by a native pointer in the program capability. Installation is
process-wide, idempotent, thread-safe and cannot be replaced after installation.
The VM projects only the word view and membership query into its execution
state; the generic DFA projects only the word view. The linker order lists the
specialized DFA symbols beside their shared predecessors so selection preserves
the hot code layout. Its read-only jump-table and dispatch-table keys likewise
stay beside the existing regex data entries, so monomorphization preserves data
placement as well as code placement.

Perex program words remain relocatable property IDs. The process-local witness
carries native code, not a GC-managed address, and witnessed relocation reuses
it without slot access. Perry stores witnesses as typed MaybeUninit values
behind a presence tag. Perex exposes an explicit padding check; compile-time
assertions guarantee every byte in a recorded witness is initialized for heap
scanning. Arbitrary integer images must never be read as witnesses because a
native provider reference has validity and provenance requirements.
