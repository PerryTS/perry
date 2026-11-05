// Regression: the `in` operator on a Web Fetch `Request` / `Response` handle
// must report its real properties.
//
// Perry represents `Request` / `Response` / `Headers` as native handle-band ids
// (not heap objects). `js_object_has_property` (the `in` operator) took a
// blanket shortcut and reported `false` for ALL handle-band values to avoid
// dereferencing the id as a pointer — but a `Request` genuinely has `body` /
// `method` / `url` / `headers`. Auth.js's request-body parser gates on
// `"body" in request` (`if (!("body" in e) || !e.body …) return`), so the blanket
// `false` made it skip parsing the credentials POST body — the `csrfToken` never
// reached the CSRF check and every login failed with `MissingCSRF`. The fix
// delegates a string key to the same handle property dispatcher property *reads*
// use. Byte-identical to `node --experimental-strip-types`.

const req = new Request("http://example.com/", {
  method: "POST",
  body: "csrfToken=abc123",
  headers: { "content-type": "application/x-www-form-urlencoded" },
});
console.log("body in req: " + ("body" in req));
console.log("method in req: " + ("method" in req));
console.log("url in req: " + ("url" in req));
console.log("headers in req: " + ("headers" in req));
console.log("bodyUsed in req: " + ("bodyUsed" in req));
console.log("nonexistent in req: " + ("zzTotallyNotAProp" in req));

const res = new Response("hello", { status: 201 });
console.log("body in res: " + ("body" in res));
console.log("status in res: " + ("status" in res));
console.log("ok in res: " + ("ok" in res));
console.log("nonexistent in res: " + ("zzTotallyNotAProp" in res));

// Next.js's NextRequest extends Request — the `in` check must forward through
// the subclass's underlying native handle too.
class MyReq extends Request {}
const sub = new MyReq("http://example.com/", { method: "POST", body: "csrfToken=abc123", headers: {} });
console.log("body in subclass: " + ("body" in sub));
console.log("method in subclass: " + ("method" in sub));
console.log("nonexistent in subclass: " + ("zzTotallyNotAProp" in sub));
