// Symbol-keyed own properties on native fetch objects (Request / Response /
// Headers). Frameworks tag these objects with private symbols — e.g. Astro's
// node adapter does `Reflect.set(request, clientAddressSymbol, ip)` on every
// request — so the write must behave like on any ordinary object.

const registered = Symbol.for("parity.fetch.registered");
const local = Symbol("parity.fetch.local");

function exercise(label: string, obj: any) {
  console.log(label, "Reflect.set registered:", Reflect.set(obj, registered, "r1"));
  console.log(label, "Reflect.get registered:", Reflect.get(obj, registered));

  obj[local] = "l1";
  console.log(label, "direct get local:", obj[local]);

  obj[registered] = "r2";
  console.log(label, "overwrite registered:", obj[registered]);

  const syms = Object.getOwnPropertySymbols(obj);
  console.log(label, "own symbols include registered:", syms.includes(registered));
  console.log(label, "own symbols include local:", syms.includes(local));

  console.log(label, "delete local:", delete obj[local], obj[local]);
}

function viaUntypedParam(label: string, target: any) {
  // Reach the write through an untyped parameter, the shape framework code uses.
  console.log(label, "param Reflect.set:", Reflect.set(target, local, "p1"));
  console.log(label, "param get:", target[local]);
}

const req = new Request("http://localhost/path", { method: "POST" });
const res = new Response("body", { status: 201 });
const headers = new Headers({ "x-a": "1" });

exercise("request", req);
exercise("response", res);
exercise("headers", headers);

viaUntypedParam("request", new Request("http://localhost/"));
viaUntypedParam("response", new Response(null));
viaUntypedParam("headers", new Headers());

// The native state must survive the symbol writes.
console.log("request still works:", req.method, req.url);
console.log("response still works:", res.status);
console.log("headers still works:", headers.get("x-a"));
