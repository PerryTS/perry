// `Object.assign(process.env, parsed)` — how `@next/env` (and dotenv, and most
// config loaders) install a parsed `.env` file. `process.env` is not an ordinary
// object: reads go through the runtime's env lookup, so keys merged in by the
// generic object-assign path landed in a plain object that nothing ever consulted
// and every `process.env.X` read came back `undefined`.

const parsed: Record<string, string> = {
  MY_APP_KEY: "abc123",
  DATABASE_URL: "mysql://user:pw@localhost/db",
};
Object.assign(process.env, parsed);

console.log("direct read    :", process.env.MY_APP_KEY);
console.log("bracket read   :", process.env["DATABASE_URL"]);
console.log("in operator    :", "MY_APP_KEY" in process.env);

// a plain assignment must still work
process.env.SET_DIRECTLY = "yes";
console.log("set directly   :", process.env.SET_DIRECTLY);

// assigning over an existing key
Object.assign(process.env, { MY_APP_KEY: "overwritten" });
console.log("overwritten    :", process.env.MY_APP_KEY);

// a later read through a helper (not a direct member expression)
function readEnv(name: string): string | undefined {
  return process.env[name];
}
console.log("dynamic key    :", readEnv("DATABASE_URL"));

// multi-source assign
Object.assign(process.env, { A_ONE: "1" }, { A_TWO: "2" });
console.log("multi-source   :", process.env.A_ONE, process.env.A_TWO);

// Object.assign onto an ordinary object must be unaffected
const plain: any = { a: 1 };
const ret = Object.assign(plain, { b: 2 }, { c: 3 });
console.log("plain object   :", JSON.stringify(plain), ret === plain);

// A `process.env` target must not make Object.assign lose the spec's handling
// of odd sources. The first implementation special-cased the env target at the
// TOP of js_object_assign_one and re-implemented source decoding there, which
// got all three of these wrong.

// Nullish sources are SKIPPED, not an error (the env fast path enumerated them
// with js_object_keys_value, which throws ToObject's TypeError).
Object.assign(process.env, null);
Object.assign(process.env, undefined);
console.log("nullish source :", "ok");

// A primitive source exposes index keys. The fast path cast ANY source pointer
// to an ObjectHeader, so a string source was read through the wrong layout.
Object.assign(process.env, "ab");
console.log("string source  :", "ok");

// std::env::set_var PANICS (and, from an extern "C" frame, ABORTS the process)
// on a name that is empty or contains '=' or NUL. `Object.assign(process.env,
// parsed)` feeds it arbitrary keys, so one malformed line in a .env file used
// to take the whole server down. Node accepts these silently.
Object.assign(process.env, { "": "empty" });
Object.assign(process.env, { "A=B": "equals" });
console.log("odd env keys   :", "ok");
