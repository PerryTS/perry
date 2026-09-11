Own-property membership no longer stops recognizing keys when an object grows
past 65,536 properties. The shared keys-array lookup now uses its complete
shape index to prove both hits and misses, while retaining a dense-slot fallback
when the index is missing, incomplete, or stale.

This also removes the growing destination scan from ordinary missing-key checks
inside `Object.assign`: after the small-object threshold, each new key is
rejected or appended using the maintained index instead of comparing it with
every key already copied. Strict-set behavior for descriptors, accessors,
symbols, and non-extensible targets is unchanged.
