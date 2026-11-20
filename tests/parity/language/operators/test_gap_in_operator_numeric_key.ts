// The `in` operator runs ToPropertyKey on its left operand. Object property
// names are strings, so a NUMBER key must be coerced to its string form:
// `307 in {307: …}` is `"307" in {…}` and must be true. Perry only matched the
// key when it was already a string, so `307 in obj` was false while `"307" in
// obj` was true.
//
// Next.js's `isRedirectError` does `Number(digest.at(-2)) in RedirectStatusCode`
// (a `{307: …, 308: …}` map), so a `redirect()` thrown from a Server Component
// was never recognized — Next treated it as a real error and every authenticated
// page 500'd instead of redirecting.

const obj: any = { 307: "TemporaryRedirect", 308: "PermanentRedirect", 200: "OK" };

console.log("keys          :", Object.keys(obj).join(","));
console.log("307 in obj    :", 307 in obj);
console.log("'307' in obj  :", "307" in obj);
console.log("200 in obj    :", 200 in obj);
console.log("999 in obj    :", 999 in obj);
console.log("Number() in   :", Number("307") in obj);

// a computed numeric key
const k = 308;
console.log("computed k in :", k in obj);

// a float that names an integer key
console.log("307.0 in obj  :", 307.0 in obj);

// a non-integer number is not a key here
console.log("3.5 in obj    :", 3.5 in obj);

// arrays: numeric index membership
const arr = ["a", "b", "c"];
console.log("0 in arr      :", 0 in arr);
console.log("2 in arr      :", 2 in arr);
console.log("3 in arr      :", 3 in arr);
console.log("'1' in arr    :", "1" in arr);

// string keys still work, and a missing string key is still false
const s: any = { foo: 1, bar: 2 };
console.log("string key    :", "foo" in s, "baz" in s);

// the exact isRedirectError shape from Next.js
const RedirectStatusCode: any = { 307: "TemporaryRedirect", 308: "PermanentRedirect", 303: "SeeOther" };
function isRedirectError(e: any): boolean {
  if ("object" != typeof e || null === e || !("digest" in e) || "string" != typeof e.digest) return false;
  const t = e.digest.split(";");
  const [r, n] = t;
  const a = t.slice(2, -2).join(";");
  const o = Number(t.at(-2));
  return r === "NEXT_REDIRECT" && ("replace" === n || "push" === n) && "string" == typeof a && !isNaN(o) && o in RedirectStatusCode;
}
const err: any = new Error("NEXT_REDIRECT");
err.digest = "NEXT_REDIRECT;replace;/;307;";
console.log("isRedirectError:", isRedirectError(err));

// a symbol key still round-trips through `in`
const sym = Symbol("s");
const withSym: any = { [sym]: 1 };
console.log("symbol in     :", sym in withSym);
