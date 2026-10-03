// `Object.setPrototypeOf` on a prototype an instance already inherits
// through: the read must follow the NEW chain. The relink transitions the
// relinked prototype's shape, so a site's holder facts decline on their own;
// what this pins is the read those sites fall back to and confirm a prime
// against. For a declared class that read used to walk the registered parent
// class id (fixed at declaration) and answer from the old parent's
// prototype, at a fresh site whether or not another site had primed.
//
// Each line is `name value`, compared with node. Primed cases read once at a
// different site before the relink; same-site cases relink mid-loop at a
// parameter receiver with an argv trip count (a fixed small loop is unrolled
// and a literal receiver refined, so neither would reach the site).
const N = Number(process.argv[2] ?? "40");
const H = N >> 1;
function show(name: string, v: any): void {
  console.log(name + " " + String(v));
}
function readK(o: any): any { return o.k; }
function loopK(o: any, n: number, at: number, relink: () => void): string {
  let before: any = undefined;
  let after: any = undefined;
  for (let i = 0; i < n; i++) {
    if (i === at) relink();
    const v = o.k;
    if (i < at) before = v; else after = v;
  }
  return String(before) + "/" + String(after);
}

// 1. declared class, unprimed (first, so nothing earlier has touched the classes)
{
  class B2 {} class C2 extends B2 {}
  (B2.prototype as any).k = 5;
  const inst: any = new C2();
  Object.setPrototypeOf(C2.prototype, { k: 7 });
  show("class-unprimed", inst.k);
}
// 2. declared class, depth 1 relink, primed at another site
{
  class B1 {} class C1 extends B1 {}
  (B1.prototype as any).k = 5;
  const inst: any = new C1();
  show("class-primed-before", inst.k);
  Object.setPrototypeOf(C1.prototype, { k: 7 });
  show("class-primed-other-site", inst.k);
  show("class-primed-fn-site", readK(inst));
}
// 3. declared class, same site, relink mid-loop
{
  class B3 {} class C3 extends B3 {}
  (B3.prototype as any).k = 5;
  show("class-same-site", loopK(new C3(), N, H, () => Object.setPrototypeOf(C3.prototype, { k: 7 })));
}
// 4. declared class, holder at depth 3, an intermediate class prototype relinked
{
  class A4 {} class B4 extends A4 {} class C4 extends B4 {}
  (A4.prototype as any).k = 5;
  const inst: any = new C4();
  show("class-deep-before", inst.k);
  Object.setPrototypeOf(B4.prototype, { k: 9 });
  show("class-deep-other-site", inst.k);
  show("class-deep-same-site", loopK(new C4(), N, H, () => Object.setPrototypeOf(B4.prototype, { k: 10 })));
}
// 5. relink to null ends the chain
{
  class B5 {} class C5 extends B5 {}
  (B5.prototype as any).k = 5;
  const inst: any = new C5();
  show("class-null-before", inst.k);
  Object.setPrototypeOf(C5.prototype, null);
  show("class-null-other-site", inst.k);
}
// 6. relink onto another class's prototype, and onto a getter that sees the instance
{
  class B6 {} class C6 extends B6 {} class D6 {}
  (B6.prototype as any).k = 5;
  (D6.prototype as any).k = 11;
  const inst: any = new C6();
  show("class-to-class-before", inst.k);
  Object.setPrototypeOf(C6.prototype, D6.prototype);
  show("class-to-class-after", inst.k);
  inst.tag = 12;
  Object.setPrototypeOf(C6.prototype, { get k() { return (this as any).tag; } });
  show("class-to-getter", inst.k);
}
// 7. Object.create chains: an intermediate hop relinked (depth 2 and 3)
{
  const P: any = { k: 1 };
  const mid: any = Object.create(P);
  const o: any = Object.create(mid);
  show("create-before", o.k);
  Object.setPrototypeOf(mid, { k: 2 });
  show("create-other-site", o.k);
  const P3: any = { k: 1 };
  const m3: any = Object.create(Object.create(P3));
  show("create-same-site", loopK(Object.create(m3), N, H, () => Object.setPrototypeOf(m3, { k: 3 })));
  const top: any = { k: 1 };
  const deep: any = Object.create(Object.create(Object.create(top)));
  show("create-deep-before", deep.k);
  Object.setPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(deep)), { k: 4 });
  show("create-deep-other-site", deep.k);
}
// 8. a constructor function's prototype (not a class) relinked
{
  function F(this: any) {}
  (Object.prototype as any).k = 1;
  const o: any = new (F as any)();
  show("ctor-before", o.k);
  Object.setPrototypeOf(F.prototype, { k: 5 });
  show("ctor-other-site", o.k);
  function G(this: any) {}
  show("ctor-same-site", loopK(new (G as any)(), N, H, () => Object.setPrototypeOf(G.prototype, { k: 6 })));
  delete (Object.prototype as any).k;
  show("ctor-after-delete", o.k);
}
