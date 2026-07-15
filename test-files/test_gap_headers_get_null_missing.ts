// `Headers.get(name)` must return `null` for an absent header (WHATWG). Perry
// returned a NULL string pointer NaN-boxed AS a string, so `typeof h.get(x)`
// was "string" and `h.get(x) !== null` — which broke nullish-coalescing:
// `h.get("x-forwarded-host") ?? h.get("host")` never fell through because the
// left operand wasn't nullish. Auth.js v5's trustHost URL builder then did
// `new URL("://" )` and threw "Invalid URL" on every authenticated page.

const h = new Headers();
h.set("host", "localhost:3000");
h.set("content-type", "application/json");

// present header — a real string
console.log("present typeof :", typeof h.get("host"));
console.log("present value  :", h.get("host"));

// absent header — must be null, not a "string"
const missing = h.get("x-forwarded-host");
console.log("missing typeof :", typeof missing);
console.log("missing ===null:", missing === null);
console.log("missing ==null :", missing == null);

// nullish coalescing must fall through
console.log("?? fallback    :", h.get("x-forwarded-host") ?? "FB");
console.log("?? chain       :", h.get("x-forwarded-host") ?? h.get("host"));
console.log("|| chain       :", h.get("nope") || "OR-FB");

// the exact Auth.js trustHost URL-builder pattern
const host = h.get("x-forwarded-host") ?? h.get("host");
const proto = h.get("x-forwarded-proto") ?? "https";
const scheme = proto.endsWith(":") ? proto : proto + ":";
const url = new URL(`${scheme}//${host}`);
console.log("built URL      :", url.href);

// String() of a missing header must be "null"
console.log("String(missing):", String(missing));

// after reading a missing one, present reads still work
console.log("present after  :", h.get("content-type"));
