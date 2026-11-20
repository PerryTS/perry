// `Response.json(data, init)` and `new Response(body, init)` extracted the init's
// `status` / `statusText` / `headers` ONLY when `init` was an object LITERAL the
// codegen could read at compile time. A runtime init — a bound variable, a
// function parameter, or another `Response` used as init — was dropped, so the
// response silently defaulted to status 200.
//
// `Response.json(x, {status: 401})` therefore returned 401 at module scope
// (literal init) but 200 the instant the init flowed through a variable — e.g.
// inside `NextResponse.json`, which is `class NextResponse extends Response {}`
// doing `new NextResponse(Response.json(body, init).body, thatResponse)`. Under
// Next.js every authenticated route's intended 401 became a 200.

// literal init — always worked
console.log("literal    :", Response.json({ e: 1 }, { status: 401 }).status);

// runtime init variable at module scope — worked
const init = { status: 402, statusText: "custom" };
console.log("module var :", Response.json({ e: 1 }, init).status);

// runtime init inside a function — this is what regressed to 200
function inFn(i: any): number {
  return Response.json({ e: 1 }, i).status;
}
console.log("in function:", inFn({ status: 403 }));

class C {
  static inStatic(i: any): number {
    return Response.json({ e: 1 }, i).status;
  }
  inMethod(i: any): number {
    return Response.json({ e: 1 }, i).status;
  }
}
console.log("in static  :", C.inStatic({ status: 404 }));
console.log("in method  :", new C().inMethod({ status: 405 }));

// a Response used AS the init, forwarded through a subclass `super(body, init)`
// — the exact `NextResponse.json` shape
class NR extends Response {
  constructor(body: any, i: any = {}) {
    super(body, i);
  }
  static json(body: any, i?: any): NR {
    const r = Response.json(body, i);
    return new NR(r.body, r);
  }
}
const a = NR.json({ error: "Unauthorized" }, { status: 401 });
console.log("subclass   :", a.status, a.headers.get("content-type"));

// statusText and a runtime-object headers init must also survive
const withText = Response.json({ e: 1 }, { status: 406, statusText: "Not Acceptable" });
console.log("statusText :", withText.status, withText.statusText);

// default (no init) is unchanged
console.log("default    :", Response.json({ ok: 1 }).status);

// plain new Response with a runtime init
function makeResp(i: any): Response {
  return new Response("body", i);
}
console.log("new+runtime:", makeResp({ status: 418 }).status);
