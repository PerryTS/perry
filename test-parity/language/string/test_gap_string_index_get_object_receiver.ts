// Regression: an indexed read `X.message[k]` / `X.name[k]` where the property
// actually holds an OBJECT (not a string) returned `undefined` for every key.
//
// Codegen's name-only Error-field heuristic (type_analysis/strings.rs) types
// `.message`/`.name` on an unknown receiver as String, so the IndexGet lowers
// to `js_string_index_get` — a char-at read — even when the property holds a
// plain object. The `statuses` npm package stores exactly that shape:
// `status.message = { 100: "Continue", ... }` as a static on the module's
// export FUNCTION; http-errors then reads `statuses.message[code]` at module
// init and got `undefined` for every code, so `toIdentifier(undefined)` threw
// "Cannot read properties of undefined (reading 'split')" — killing a Next.js
// standalone server at BOOT (its bundled `send` package inlines both).
//
// The runtime fix: `js_string_index_get` sniffs the receiver's GC header and
// delegates non-string heap objects to the polymorphic index-get (an ordinary
// property lookup, per JS semantics).
//
// The observable is byte-for-byte identical to `node --experimental-strip-types`.

// --- the statuses/http-errors shape: map stored as a function static ---
function status(code: number): string {
  return "" + code;
}
(status as any).message = { 100: "Continue", 200: "OK", 404: "Not Found", 500: "Internal Server Error" };
(status as any).codes = Object.keys((status as any).message).map(function mapCode(x) {
  return Number(x);
});

function toIdentifier(str: string): string {
  return str
    .split(" ")
    .map(function (w) {
      return w.charAt(0).toUpperCase() + w.slice(1);
    })
    .join("")
    .replace(/[^ _0-9a-z]/gi, "");
}

const names: string[] = [];
(status as any).codes.forEach(function forEachCode(code: number) {
  const msg = (status as any).message[code];
  names.push(code + ":" + (msg === undefined ? "UNDEFINED" : toIdentifier(msg)));
});
console.log(names.join(","));

// --- .name holding an object, numeric + string keys ---
function f() {}
(f as any).name2 = 0; // avoid TS complaints; the heuristic key is `.name`
(f as any).message = { 7: "seven", nested: { 1: "one" } };
console.log("num-key:", (f as any).message[7]);
console.log("str-key:", (f as any).message["7"]);
console.log("nested:", (f as any).message.nested[1]);

// --- the heuristic's intended case still works: real Error fields ---
const e = new Error("boom message");
console.log("err msg char:", (e as any).message[0], (e.message as any)[1]);
console.log("err msg slice:", e.message.slice(0, 4));

// --- a genuine string in .message: char-at semantics preserved ---
(f as any).note = null;
(f as any).message = "hello";
console.log("string case:", (f as any).message[1], (f as any).message[99]);
