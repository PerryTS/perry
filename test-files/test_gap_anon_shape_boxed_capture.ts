// #5951 boxes a mutable local that a class captures AND mutates into a
// one-element array cell, rewriting every value read `o` into `o[0]`. An object
// literal also lowers to `Expr::New` — against a synthetic `__AnonShape_*` whose
// constructor args are the PROPERTY VALUES, not the capture handles a real class
// construction carries. Passing the bare box there stored the CELL in the field:
// `{ routing: o }` yielded `routing === [o]`, so the reader saw
// `routing.locales === undefined` while every other read of `o` was fine.

function factory() {
  let o: any;

  // a class that captures AND mutates `o` -> `o` becomes a shared cell
  class Holder {
    snapshot: any;
    constructor() {
      this.snapshot = o;
    }
    update(x: any) {
      o = x;
    }
  }

  o = { locales: ["en", "de", "es"], defaultLocale: "en" };
  const h = new Holder();

  return function (req: string) {
    // a 5-property object literal -> lowered to `New { __AnonShape_… }`
    const lit: any = {
      routing: o,
      internalTemplateName: undefined,
      localizedPathnames: undefined,
      request: req,
      resolvedLocale: "en",
    };

    console.log("direct read   :", o.locales.length);
    console.log("field isArray :", Array.isArray(lit.routing));
    console.log("identity kept :", lit.routing === o);
    console.log("locales       :", lit.routing.locales.join(","));
    console.log("other fields  :", lit.request, lit.resolvedLocale);

    // reading the field back through a destructured parameter (how it is consumed)
    const read = function ({ routing: a, request: r }: any) {
      return a.locales.join("|") + " / " + r;
    };
    console.log("via destructure:", read(lit));

    // the cell must still be SHARED with the class (the reason the box exists)
    h.update({ locales: ["fr"], defaultLocale: "fr" });
    console.log("after update  :", o.locales.join(","), Array.isArray(o));
    const lit2: any = { routing: o, a: 1, b: 2, c: 3, d: 4 };
    console.log("relit isArray :", Array.isArray(lit2.routing), lit2.routing.locales.join(","));
    return "ok";
  };
}

console.log("result        :", factory()("REQ"));

// a real class construction must still receive its capture handle by reference:
// writes through the class stay visible to the declaring function.
function sharedCell() {
  let count = 0;
  class Counter {
    bump() {
      count++;
    }
    read() {
      return count;
    }
  }
  const c = new Counter();
  c.bump();
  c.bump();
  count += 10;
  return c.read() + "," + count;
}
console.log("shared cell   :", sharedCell());
