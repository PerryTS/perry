// Issues #3054 / #3055 — WHATWG URL static helpers and the `URL` constructor
// must apply `String(value)` (Web-IDL / ECMAScript) coercion to their
// arguments BEFORE parsing: numbers, `null`, and objects with a custom
// `toString` are stringified, and Symbols throw
// `TypeError: Cannot convert a Symbol value to a string`.
//
// Previously Perry routed these arguments through plain string-pointer
// extraction, so non-string values became a null/garbage pointer (lost the
// argument) and Symbols silently produced the wrong result instead of
// throwing. All lines compare byte-for-byte against
// `node --experimental-strip-types`.
//
// NOTE: invalid-URL throws print only `err.name` (Perry's message is
// "Invalid URL: <input>", Node's is "Invalid URL" — a separate, pre-existing
// formatting gap unrelated to argument coercion).

const base = "http://example.com/root/";
const obj = { toString() { return "child"; } };
const sym = Symbol("x");

function run(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", String(fn()));
  } catch (err) {
    const e = err as { name: string; message: string };
    // Only the Symbol-conversion message is asserted verbatim; invalid-URL
    // messages differ in a pre-existing way, so just report the name.
    if (e.message === "Cannot convert a Symbol value to a string") {
      console.log(label, "THROW", e.name, e.message);
    } else {
      console.log(label, "THROW", e.name);
    }
  }
}

// ---- #3055: new URL(input[, base]) ----
run("new number base", () => new URL(123 as unknown as string, base).href);
run("new null base", () => new URL(null as unknown as string, base).href);
run("new object base", () => new URL(obj as unknown as string, base).href);
run("new symbol base", () => new URL(sym as unknown as string, base).href);
run("new number no base", () => new URL(123 as unknown as string).href);
run("new bad base number", () => new URL("/x", 123 as unknown as string).href);

// ---- #3054: URL.canParse(input[, base]) ----
run("canParse number no base", () => URL.canParse(123 as unknown as string));
run("canParse number base", () => URL.canParse(123 as unknown as string, base));
run("canParse null base", () => URL.canParse(null as unknown as string, base));
run("canParse object base", () => URL.canParse(obj as unknown as string, base));
run("canParse symbol base", () => URL.canParse(sym as unknown as string, base));
run("canParse bad base number", () => URL.canParse("/x", 123 as unknown as string));

// ---- #3054: URL.parse(input[, base]) ----
run("parse number no base", () => URL.parse(123 as unknown as string));
run("parse number base", () => URL.parse(123 as unknown as string, base)?.href);
run("parse null base", () => URL.parse(null as unknown as string, base)?.href);
run("parse object base", () => URL.parse(obj as unknown as string, base)?.href);
run("parse symbol base", () => URL.parse(sym as unknown as string, base)?.href);
run("parse bad base number", () => URL.parse("/x", 123 as unknown as string));
