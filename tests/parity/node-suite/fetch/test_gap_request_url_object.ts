// `new Request(input, init)` runs ToString on `input` when it isn't already a
// Request. A URL object must stringify to its href. Perry's codegen extracted a
// raw string pointer from the first arg (`js_get_string_pointer_unified`), which
// only unwraps an actual string — handed a URL object it read the object pointer
// as a string and produced "". So `new Request(new URL(u)).url` was empty.
//
// Auth.js v5 builds its session request as `new Request(makeSessionUrl(...))`
// where the helper returns a URL object, so the session lookup got an empty URL,
// returned 400, and `auth()` yielded the wrong value — every authenticated page
// then mis-decided its redirect.

const href = "http://localhost:3000/api/auth/session";

// a string first arg — always worked
console.log("string      :", new Request(href).url);

// a URL object first arg — the regression
const u = new URL(href);
console.log("URL object  :", new Request(u).url);

// URL object + init
console.log("URL + init  :", new Request(u, { method: "GET", headers: { cookie: "" } }).url);

// URL object with a path and query
const u2 = new URL("https://example.com/a/b?x=1&y=2");
const r2 = new Request(u2);
console.log("URL w/ query:", r2.url);

// method/headers from a runtime init still apply (regression guard for #5458)
const init = { method: "POST", headers: { "content-type": "application/json" } };
const r4 = new Request(u, init);
console.log("method kept :", r4.method, r4.url);

// String()/toString of the URL match what Request stored
console.log("consistency :", new Request(u).url === u.toString());
