Whole-program facts: an identifier the program never declares (`window`,
`document`, `__THREE_DEVTOOLS__`, a `declare const`) now reads what the program
stored on the global object under that name, exactly as the runtime does
(`[[Get]](globalThis, name)`; nothing stored is a `ReferenceError`). Before, such
a read was an unknown value, and every object it touched was treated as held by
unseen code: one `typeof __THREE_DEVTOOLS__ !== "undefined"` guard in a
constructor made every instance of the class unprovable. Names the realm
defines (`setTimeout`, `Map`, ...) keep their built-in values.

A program that imports a module the compile does not contain (an unresolved,
non-builtin specifier, so its code runs at load unseen) still opens every global
key, realm globals included: that code may store or define anything there.
Per-unit facts keep the previous rule.
