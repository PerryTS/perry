// #11635: `F.prototype.x = <call>` must not hold F in a register across the
// call.
//
// HIR lowers `F.prototype.x = v` (and the aliased `var proto = F.prototype;
// proto.x = v`) for a function declaration F into a runtime registration keyed
// by F's closure. Codegen evaluated F first, then the value, then called the
// registration with F's register. When the value is a call that collects, an
// evacuating minor moves F while the register keeps its old address, and the
// registration reads the retired closure. moment 2.31.0 does exactly this at
// module init: `proto.toIsoString = deprecate(msg, toISOString$1)` with
// `proto = Duration.prototype`, where Duration is a function declaration
// inside the UMD factory. Under a seeded GC schedule the process faulted in
// `synthetic_class_id_for_function`; without the from-space quarantine the
// method could be registered under the stale address, so instances never saw
// it.
//
// The shape is kept: F is declared inside a factory and captured by nested
// functions (so it is a boxed heap closure that can move), the prototype is
// aliased, and each value comes from a
// helper that allocates enough garbage for a collection to land inside it.
//
// Output must be byte-identical to node.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}

let churned = 0;

function factory(): any {
  const tag = "D";
  function Duration(this: any, v: number) {
    this.v = v;
  }
  function deprecate(msg: string, fn: (this: any) => string): (this: any) => string {
    churned += churn(20000);
    return function (this: any) {
      return msg + ":" + fn.call(this);
    };
  }
  function show(this: any) {
    return tag + this.v;
  }
  // Nested functions that capture Duration, as moment's do: that is what
  // puts Duration in a box whose read is not re-derivable from a root.
  function isDuration(o: any): boolean {
    return o instanceof Duration;
  }
  function make(v: number): any {
    return new (Duration as any)(v);
  }
  const proto = Duration.prototype;
  proto.a = deprecate("a", show);
  proto.b = deprecate("b", show);
  Duration.prototype.c = deprecate("c", show);
  proto.d = deprecate("d", show);
  (Duration as any).isDuration = isDuration;
  (Duration as any).make = make;
  return Duration;
}

const D = factory();
churn(20000);
const out: string[] = [];
for (let i = 0; i < 3; i++) {
  const d = new D(i);
  out.push(d.a(), d.b(), d.c(), d.d());
  out.push(String(d instanceof D), typeof D.prototype.a, typeof d.d);
  out.push(String(D.isDuration(d)), D.make(i + 10).c());
}
console.log(out.join(" "));
console.log("churned", churned > 0);
