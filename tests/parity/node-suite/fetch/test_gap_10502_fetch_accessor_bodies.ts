// Fetch prototype accessors are real native bodies, with no captured key.
// Their identity determines their name and property even when called directly.
const request = new Request("https://example.invalid/a", {
  method: "POST", headers: { "x-test": "request" },
});
const response = new Response("ok", { status: 201, headers: { "x-test": "response" } });
function getter(proto: any, key: string): any {
  return Object.getOwnPropertyDescriptor(proto, key)!.get;
}
for (const key of ["url", "method", "bodyUsed"]) {
  const get = getter(Request.prototype, key);
  console.log("request", key, get.name, get.length, get.call(request));
}
for (const key of ["status", "ok", "bodyUsed"]) {
  const get = getter(Response.prototype, key);
  console.log("response", key, get.name, get.length, get.call(response));
}
console.log("headers", getter(Request.prototype, "headers").call(request).get("x-test"),
  getter(Response.prototype, "headers").call(response).get("x-test"));
class SubRequest extends Request {}
class SubResponse extends Response {}
console.log("subclasses", getter(Request.prototype, "url").call(new SubRequest("https://example.invalid/sub")),
  getter(Response.prototype, "status").call(new SubResponse(null, { status: 202 })));
try {
  getter(Request.prototype, "url").call({});
  console.log("brand", false);
} catch (error) { console.log("brand", error instanceof TypeError); }
