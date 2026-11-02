Fix two whole-program facts gaps on arrays: a folded `indexOf`/`lastIndexOf`/`includes`
now reads the array elements instead of letting the array escape, and a store whose
key is a number is an element store rather than a named-key store.
