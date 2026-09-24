// #11229: a capturing class constructed with FEWER arguments than its
// constructor declares must see `undefined` (and so its defaults) in the
// omitted parameters.
//
// A class that reads an enclosing binding receives the captured values as
// synthesized trailing constructor params, which the `new` site appends
// after the user arguments. The monomorph default-fill pass then padded the
// call out to the constructor's full param count by appending `undefined`
// AFTER those captures, so the captures landed in the omitted user
// parameters. mongodb 7.5.0's `new OnDemandDocument(this.bson, offset)` bound
// the module-scope `BSONElementOffset` object to `isArray`, and every cursor
// operation threw `Cannot convert undefined or null to object`.
//
// Also covered: construction from outside the class, `new this.constructor`,
// defaults that read earlier params, rest params, spread arguments, super()
// chains (explicit and implicit constructors), `arguments.length`, and
// `Reflect.construct` (which rejected every per-evaluation class object).
import { OnDemandDocument } from "./fixtures/issue_11229_capture_ctor_arity/document.cjs";
import * as S from "./fixtures/issue_11229_capture_ctor_arity/shapes.cjs";

const M: any = S;
const { show } = M;
function say(label: string, f: () => any) {
  try {
    console.log(label, show(f()));
  } catch (e: any) {
    console.log(label, "THREW", e?.constructor?.name, e?.message);
  }
}
const pick = (d: any) => ({ offset: d.offset, isArray: d.isArray, elements: d.elements });

// 1. The mongodb shape.
const root: any = new (OnDemandDocument as any)("abc");
say("odd.root", () => pick(root));
say("odd.child", () => pick(root.child(1)));
say("odd.childArray", () => pick(root.childArray(2)));
say("odd.direct", () => pick(new (OnDemandDocument as any)("xy", 5)));
say("odd.reflect", () => pick(Reflect.construct(OnDemandDocument as any, ["r", 3])));

// 2. Inside/outside the class, arguments.length, new this.constructor.
const d = new M.Doc("abc");
say("doc.root", () => d);
say("doc.child", () => d.child(4));
say("doc.childArr", () => d.childArr(2));
say("doc.viaThisCtor", () => d.viaThisCtor(9));
say("doc.outside1", () => new M.Doc("xy"));
say("doc.outside2", () => new M.Doc("xy", 5));

// 3. Defaults that reference earlier params.
say("dep.make1", () => M.Dep.make1(1));
say("dep.make2", () => M.Dep.make2(1, 5));
say("dep.outside", () => new M.Dep(3));

// 4. Rest params and spread arguments.
say("rest.none", () => M.Rest.none());
say("rest.one", () => M.Rest.one());
say("rest.three", () => M.Rest.three());
say("rest.spread", () => M.Rest.spread([4, 5]));
say("rest.outsideSpread", () => new M.Rest(...[7, 8, 9]));
say("spread.s1", () => M.Spread.s([1]));
say("spread.s3", () => M.Spread.s([1, 2, 3]));

// 5. super() chains: explicit ctor forwarding fewer args, and implicit ctors.
say("mid", () => new M.Mid("m"));
say("leaf", () => new M.Leaf("l"));
say("implicit", () => new M.Implicit("i"));

// 6. Reflect.construct on per-evaluation class objects.
say("reflect.doc", () => Reflect.construct(M.Doc, ["r"]));
say("reflect.dep", () => Reflect.construct(M.Dep, [2]));
say("reflect.mid", () => Reflect.construct(M.Mid, ["rm"]));
say("reflect.newTarget", () => Reflect.construct(M.Base2, ["nt"], M.Mid) instanceof M.Mid);

// 7. Many omitted params, zero args.
say("many.zero", () => M.Many.zero());
say("many.outside", () => new M.Many(1));

// 8. The same shapes without `arguments` in the constructor.
say("na.dep1", () => M.DepNA.make1(1));
say("na.dep0", () => M.DepNA.make0());
say("na.depOutside", () => new M.DepNA(3));
say("na.mid", () => new M.MidNA("m"));
say("na.midAgain", () => M.MidNA.again("x"));
say("na.leaf", () => new M.LeafNA("l"));
say("na.rest", () => M.RestNA.one());
say("na.reflectLeaf", () => Reflect.construct(M.LeafNA, ["rl"]));

// 9. A TS factory class capturing a parameter, constructed with omitted args
//    from inside and outside.
function makeNode(tag: string) {
  return class Node {
    tag: string; a: any; b: any; c: any; n: number;
    constructor(a?: any, b: any = "B", c?: any) {
      this.tag = tag; this.a = a; this.b = b; this.c = c; this.n = arguments.length;
    }
    kid() { return new Node(1); }
  };
}
const N: any = makeNode("t");
say("ts.outside", () => new N());
say("ts.kid", () => new N(0).kid());
say("ts.reflect", () => Reflect.construct(N, [5]));
