Fix `isDarkMode()` returning garbage instead of a JavaScript boolean. Native backends return an integer flag; the shared dispatch table now declares that ABI and converts zero/nonzero values to the proper `false`/`true` tags. Native instance-call handling and generated fallback stubs support the same return kind.

Add host/Android code-generation coverage and ABI checks against eight native backends. Controlled native execution verifies false for zero and true for positive and negative flags. No version bump.
