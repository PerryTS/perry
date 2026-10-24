Preserve CommonJS receiver semantics through the compiler-generated module wrapper: sloppy functions coerce nullish receivers to `globalThis`, while function and module strict directives keep their receivers unchanged.

Keep direct-eval binding objects in the caller frame and avoid duplicate dynamic-scope lookups in their native-binding accessors. This avoids emitting extra builder functions while preserving live captured bindings.

Preserve strict CommonJS source offsets through an equal-length generated import alias, keeping computed member identities and CommonJS classification intact.
