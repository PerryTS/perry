// #321 / #64 / Console-bare-global: `console` referenced as a bare identifier
// (not a method receiver) at module-top-level resolved to 0 (typeof "number")
// instead of an object handle whose `.log`/`.error`/etc. accessors work.
// effect's `defaultConsole.unsafe = console` (line 99 of
// node_modules/effect/src/internal/defaultServices/console.ts) crashed downstream
// inside `Effect.runSync(Console.log("x"))` with "(number).log is not a function"
// because the stored value was the bare-fallthrough 0 sentinel.
//
// The fix adds "console" to `is_builtin_global_value_name` so the bare Ident
// lowers to PropertyGet{GlobalGet(0), "console"} like other builtins (`Date`,
// `Array`, `process`, etc.), which codegen routes through the native-module
// helper and downstream method-dispatch works.

const c = console;
const obj = { unsafe: console };

console.log("typeof c:", typeof c);
console.log("typeof obj.unsafe:", typeof obj.unsafe);
console.log("typeof obj.unsafe.log:", typeof obj.unsafe.log);

obj.unsafe.log("via_alias");
c.log("via_direct_alias");
