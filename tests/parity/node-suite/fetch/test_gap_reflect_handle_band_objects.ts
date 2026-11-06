// Perry represents native web objects — Headers, Request, Response, sockets,
// streams — as handle ids (a POINTER_TAG'd small integer), not heap ObjectHeaders.
// `reflect_value_is_object` classified every address below 4 GB as a NON-object, so
// the whole `Reflect.*` family refused them:
//
//   Reflect.get(headers, "get")   ->  TypeError: Reflect.get called on non-object
//
// They are objects to JS, and Next's app-route runtime wraps the request in a Proxy
// whose `get` trap forwards through `Reflect.get(target, …)` — so every route that
// touched the request 500'd.

const h = new Headers();
h.set("x-test", "v1");
h.set("content-type", "application/json");

console.log("get is fn    :", typeof Reflect.get(h, "get"));
console.log("has          :", Reflect.has(h, "get"), Reflect.has(h, "nope"));

// the method read through Reflect, invoked against its real receiver
const getter = Reflect.get(h, "get") as (n: string) => string | null;
console.log("read header  :", getter.call(h, "x-test"));
console.log("read header2 :", getter.call(h, "content-type"));

// a Proxy whose get trap forwards to Reflect.get — Next's app-route shape
const proxied: any = new Proxy(h, {
  get(target, prop) {
    return Reflect.get(target, prop);
  },
});
console.log("via proxy    :", typeof proxied.get, typeof proxied.set, typeof proxied.has);

// Reflect on a Response handle
const res = new Response("body", { status: 401 });
console.log("response     :", Reflect.get(res, "status"), Reflect.has(res, "headers"));

// Reflect on a Request handle
const req = new Request("http://localhost/api/sites");
console.log("request      :", Reflect.get(req, "method"), Reflect.get(req, "url"));

// a plain heap object must still work, and a primitive must still be refused
const plain = { a: 1, b: 2 };
console.log("plain object :", Reflect.get(plain, "a"), Reflect.has(plain, "b"));
try {
  Reflect.get(42 as any, "x");
  console.log("primitive    : NO THROW");
} catch (e: any) {
  console.log("primitive    : threw", e instanceof TypeError);
}
