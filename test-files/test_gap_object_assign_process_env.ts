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
