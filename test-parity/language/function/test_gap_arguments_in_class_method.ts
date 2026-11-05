// #8040: legacy `arguments` inside a CLASS method body, where the method also
// declares named parameters.
//
// The object-literal twin of this file
// (`test_gap_arguments_in_object_literal_method.ts`, case 3) has asserted since
// #321 that "`arguments` reflects ALL passed values, not just the trailing
// ones". The class-method spelling was never covered, and it did not hold: the
// `arguments` slot #677 synthesizes is a trailing `is_rest` param, which is
// exactly how a user `...rest` is spelled, so every class-method CALL SITE
// bundled it from `declared - 1` — the offset a user rest wants. `m(a, b)`
// called as `m(1, 2, 3)` therefore saw `arguments === [3]`.
//
// The freestanding-function and object-literal paths already emitted the other
// shape (bundle from argument 0, then mark the array), which is why the defect
// was invisible to every existing arguments test.
//
// Found via a production Next.js App Route. Next.js bundles OpenTelemetry's
// `NoopTracer.startActiveSpan`, which opens with
// `if (arguments.length < 2) return;` — under the conflation that guard fired
// on every well-formed 3-argument call, so `tracer.trace()` returned
// `undefined` without invoking its callback and the route resolved its handler
// having never entered `routeModule.handle` (empty 200).
//
// Compared byte-for-byte against `node --experimental-strip-types`.

class Base {
  // (1) named params + `arguments`. The discriminating call passes MORE
  // arguments than the method declares.
  two(a: number, b: number) {
    return arguments.length;
  }

  // (2) more declared params than arguments passed — `arguments.length` is the
  // number PASSED, never the number declared, and never a negative-clamped 0.
  four(a: number, b: number, c: number, d: number) {
    return arguments.length;
  }

  // (3) indexing, not just length: `arguments[0]` is the FIRST passed value.
  idx(a: number, b: number) {
    return `${String(arguments[0])},${String(arguments[1])},${String(arguments[2])}`;
  }

  // (4) a real `...rest` AND `arguments` over the same list at two offsets.
  both(a: number, ...rest: number[]) {
    return `args=${arguments.length} rest=${rest.length}`;
  }

  // (4b) the same shape, by VALUE not just by length: `rest` must be a real
  // array of the trailing args while `arguments` holds every one — a
  // resolution that hands the rest slot a scalar (or the full list) fails
  // this line even where the lengths happen to agree.
  bothIdx(a: number, ...rest: number[]) {
    return [
      a,
      rest.length,
      arguments.length,
      arguments[0],
      arguments[1],
      arguments[2],
    ].join(",");
  }

  // (5) async methods lower through the async-to-generator transform, which
  // rewrites the body's locals — the synthesized param has to survive it.
  async asy(a: number, b: number) {
    return arguments.length;
  }

  // (6) static methods take a different call-site path than instance methods.
  static stat(a: number, b: number) {
    return arguments.length;
  }

  // (6b) the rest+`arguments` both-shape through the static call-site path.
  static statBoth(a: number, ...rest: number[]) {
    return `args=${arguments.length} rest=${rest.length} rest0=${rest[0]}`;
  }
}

// (7) an inherited method resolves through the parent, and must bundle using
// the parent's declared arity.
class Derived extends Base {}

// (10) `super.m(…)` reaches the parent body through its own call site, which
// passed every argument POSITIONALLY — so the parent's trailing array slot
// received a raw scalar rather than an array.
class Override extends Base {
  two(a: number, b: number) {
    return `own=${arguments.length} super=${super.two(1, 2, 3)}`;
  }
  fromSibling() {
    return super.two(1, 2, 3);
  }
  // (10b) the rest+`arguments` both-shape through the `super.m(…)` call site.
  superBoth() {
    return super.both(1, 2, 3);
  }
}

// (11) generator methods lower through yet another body transform.
class Gen {
  *g(a: number, b: number) {
    yield arguments.length;
  }
}

const b = new Base();
console.log("(1) two(1,2,3):", (b as any).two(1, 2, 3));
console.log("(1) two(1):", (b as any).two(1));
console.log("(1) two():", (b as any).two());
console.log("(2) four(1,2,3):", (b as any).four(1, 2, 3));
console.log("(2) four(1..6):", (b as any).four(1, 2, 3, 4, 5, 6));
console.log("(3) idx(7,8,9):", (b as any).idx(7, 8, 9));
console.log("(4) both(1,2,3):", (b as any).both(1, 2, 3));
console.log("(4b) bothIdx(1,2,3):", (b as any).bothIdx(1, 2, 3));
console.log("(6) Base.stat(1,2,3):", (Base as any).stat(1, 2, 3));
console.log("(6b) Base.statBoth(1,2,3):", Base.statBoth(1, 2, 3));
console.log("(7) Derived.two(1,2,3):", (new Derived() as any).two(1, 2, 3));

// (8) the same method reached dynamically and through call/apply — these went
// through the runtime dispatch table, which already carried the synthesized-
// arguments bit, so they are the control arm that was ALREADY correct.
const name = "two";
console.log("(8) b[name](1,2,3):", (b as any)[name](1, 2, 3));
console.log("(8) two.call(1,2,3):", (b as any).two.call(b, 1, 2, 3));
console.log("(8) two.apply([1,2,3]):", (b as any).two.apply(b, [1, 2, 3]));

// (9) the shape that broke Next.js: a guard on `arguments.length` that decides
// whether the callback runs at all.
class NoopTracer {
  startActiveSpan(name: string, arg2: any, arg3: any, arg4: any) {
    let fn: any;
    if (arguments.length < 2) return "BAILED";
    if (arguments.length === 2) fn = arg2;
    else if (arguments.length === 3) fn = arg3;
    else fn = arg4;
    return fn();
  }
}
class ProxyTracer {
  inner: NoopTracer;
  constructor(inner: NoopTracer) {
    this.inner = inner;
  }
  startActiveSpan(name: string, arg2: any, arg3: any, arg4: any) {
    const t = this.inner;
    return Reflect.apply(t.startActiveSpan, t, arguments as any);
  }
}
const tracer = new ProxyTracer(new NoopTracer());
// Three arguments against four declared params — the exact Next.js call.
console.log("(9) startActiveSpan:", (tracer as any).startActiveSpan("span", {}, () => "CALLBACK-RAN"));

console.log("(10) Override.two(9):", (new Override() as any).two(9));
console.log("(10) Override.fromSibling():", new Override().fromSibling());
console.log("(10b) Override.superBoth():", new Override().superBoth());
console.log("(11) Gen.g(1,2,3):", (new Gen() as any).g(1, 2, 3).next().value);

(async () => {
  console.log("(5) asy(1,2,3):", await (b as any).asy(1, 2, 3));
})();
