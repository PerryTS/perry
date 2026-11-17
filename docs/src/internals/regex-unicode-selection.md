# Selecting RegExp Unicode membership

Perex's `selectable-unicode` feature puts nontrivial Unicode property membership
behind an install-once function slot. `js_runtime_install_regex_unicode` fills
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
construction, named-group parsing and binary-program validation. They all use
the same slot. Property descriptors/name lookup, case-fold data and properties
of strings have separate references and remain available. A missing installation
panics at the first membership request instead of returning an incorrect match;
the compiler must therefore retain installation for every open producer.
Native `perry-perex` clients accept arbitrary patterns and install unconditionally.

The slot holds only an immutable native function pointer, never a GC value. It is
process-wide, idempotent, thread-safe and cannot be replaced after installation.
