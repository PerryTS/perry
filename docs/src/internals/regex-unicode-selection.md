# Selecting RegExp Unicode membership

Perex's `selectable-unicode` feature puts nontrivial Unicode property membership
behind an install-once function slot. `js_runtime_install_regex_unicode` fills
that slot before module initialization. The default/full runtime installer also
fills it. The engine installer does not retain this database on its own.

The compiler retains the installer for Unicode-mode literals, non-ASCII patterns,
property escapes, Unicode escapes and named-group syntax. Every dynamic RegExp
construction retains it, including literal patterns whose flags are dynamic.
String-to-RegExp coercion, exposed constructors, prototype `compile`, computed
API members and escaped global objects also retain it. Native modules, IPC,
workers, addons, plugins, UI and deferred dynamic code are open producers and
retain the database conservatively. These are whole-program unions collected
before code generation; the entry installer is part of the object cache input.

Omission is limited to programs whose remaining producers are ASCII literals
without those syntactic forms. ASCII group validation returns before membership;
ordinary character ranges, whitespace classes and case-fold tables do not access
the property database. The selector deliberately over-includes some patterns.
It does not infer runtime strings from TypeScript annotations or mutable bindings.

Membership callers in Perex include both executors, atom filtering, candidate
construction, named-group parsing and binary-program validation. They all use
the same slot. Property descriptors/name lookup, case-fold data and properties
of strings have separate references and remain available. A missing installation
panics at the first membership request instead of returning an incorrect match;
the compiler must therefore retain installation for every open producer.
Native `perry-perex` clients accept arbitrary patterns and install unconditionally.

The slot holds only an immutable native function pointer, never a GC value. It is
process-wide, idempotent, thread-safe and cannot be replaced after installation.
