On the web target a class definition runs its computed member names and
static initializers: `function f() { let n = 0; class K { [(n++, "m")]() {} } return n; }`
returns 1, computed methods, accessors and fields are installed under their
evaluated keys (a Symbol-keyed computed accessor is not installed yet), static
fields and `static { }` blocks run in source order with `this` bound to the
class, and each evaluation of a class in a function body gets its own statics.
