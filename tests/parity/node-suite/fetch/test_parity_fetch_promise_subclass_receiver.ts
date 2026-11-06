// A subclass of a native base whose state lives beside an ordinary object
// (the WHATWG Fetch handles Request/Response/Headers/Blob, and Promise) keeps
// the receiver its `new` allocated: `super()` must not replace `this` with a
// fresh native value carrying the base's intrinsic prototype. Next.js's
// `NextRequest extends Request` keeps `nextUrl` behind `this[INTERNALS]` and a
// prototype getter; with `this` replaced, `req.nextUrl` read undefined and
// every route handler answered 500.
const INTERNALS = Symbol("internal");

function report(label: string, o: any, C: any, B: any, extra: unknown) {
  console.log(
    label,
    o instanceof C,
    o instanceof B,
    Object.getPrototypeOf(o) === C.prototype,
    o.constructor === C,
    o.g,
    typeof o.m === "function" ? o.m() : "no m",
    extra,
  );
}

class NReq extends Request {
  constructor(input: string, init?: RequestInit) {
    super(input, init);
    (this as any)[INTERNALS] = { nextUrl: new URL(input) };
  }
  get nextUrl(): URL {
    return (this as any)[INTERNALS].nextUrl;
  }
  get g() {
    return "req-g";
  }
  m() {
    return "req-m";
  }
}
const rq = new NReq("http://localhost/api/hello?name=perry", { method: "POST" });
report("Request", rq, NReq, Request, [rq.url, rq.method, rq.nextUrl.pathname, rq.nextUrl.searchParams.get("name")].join(" "));

class BareReq extends Request {
  get g() {
    return "bare-g";
  }
}
const brq = new BareReq("http://localhost/bare");
report("Request ctor-less", brq, BareReq, Request, brq.url);

class NRes extends Response {
  tag: number;
  constructor(body: string, init?: ResponseInit) {
    super(body, init);
    this.tag = 7;
  }
  get g() {
    return this.tag + 1;
  }
  m() {
    return "res-m";
  }
}
const rs = new NRes("hi", { status: 201, headers: { "x-a": "1" } });
report("Response", rs, NRes, Response, [rs.status, rs.ok, rs.headers.get("x-a")].join(" "));
// The native backing the receiver keeps is an internal slot: no reflection
// sees it.
const forIn: string[] = [];
for (const k in rs) forIn.push(k);
console.log(
  "Response own keys",
  Object.keys(rs).join(","),
  Object.getOwnPropertyNames(rs).join(","),
  Reflect.ownKeys(rs).map(String).join(","),
  forIn.filter((k) => Object.prototype.hasOwnProperty.call(rs, k)).join(","),
  JSON.stringify(rs),
  "__perry_fetch_handle__" in rs,
  Object.prototype.hasOwnProperty.call(rs, "__perry_fetch_handle__"),
);

class NHeaders extends Headers {
  tag = 3;
  get g() {
    return this.tag;
  }
  m() {
    return "headers-m";
  }
}
const hd = new NHeaders({ "x-b": "2" });
hd.set("x-c", "3");
report("Headers", hd, NHeaders, Headers, [hd.get("x-b"), hd.get("x-c"), hd.has("x-c"), hd.has("x-d")].join(" "));
hd.append("x-b", "4");
hd.delete("x-c");
const hget = hd.get;
console.log("Headers methods", hd.get("x-b"), hd.has("x-c"), hget.call(hd, "x-b"), Headers.prototype.has.call(hd, "x-b"));

class NBlob extends Blob {
  get g() {
    return "blob-g";
  }
}
const bl = new NBlob(["abc"], { type: "text/plain" });
report("Blob", bl, NBlob, Blob, [bl.size, bl.type].join(" "));

function spread(B: any, ...args: any[]) {
  const C = class extends B {
    tag: number;
    constructor(...a: any[]) {
      super(...a);
      this.tag = 7;
    }
    get g() {
      return this.tag + 1;
    }
  };
  const o = new C(...args);
  console.log("spread", B.name, o instanceof C, o instanceof B, o.g);
}
spread(Request, "http://a/");
spread(Response, "x");
spread(Headers);
spread(Blob, []);
spread(Promise, () => {});

class TaggedPromise<T> extends Promise<T> {
  tag = "tp";
  get g() {
    return this.tag;
  }
}
const tp = new TaggedPromise<number>((resolve) => resolve(5));
report("Promise", tp, TaggedPromise, Promise, tp.tag);

async function main() {
  console.log("Request body", await new NReq("http://localhost/", { method: "POST", body: "payload" }).text());
  console.log("Response body", await rs.text(), rs.bodyUsed);
  console.log("Blob text", await bl.text());
  console.log("Promise await", await tp);
}
main();
