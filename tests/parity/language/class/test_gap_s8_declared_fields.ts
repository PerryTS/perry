// S8: declared class fields and empty-born classes admitted into families.
const log: string[] = [];
const keys = (o: object) => Object.keys(o).join(",");
const show = (l: string, o: any) => log.push(l + " " + keys(o) + " " + JSON.stringify(o));

class Base {
  a = 1;
  b: string = "x";
  c;
  d = 0;
}
show("base", new Base());

class Ctor {
  a = 1;
  z;
  constructor(p: number, q?: number) {
    this.z = p;
    if (q !== undefined) (this as any).late = q;
    (this as any).k = 5;
  }
}
show("ctor1", new Ctor(1));
show("ctor2", new Ctor(1, 2));

// fields before ctor-body store of the same key keep the field position
class Same {
  a = 1;
  b = 2;
  constructor() {
    (this as any).b = 20;
    (this as any).a = 10;
    (this as any).c = 30;
  }
}
show("same", new Same());

// subclass adding fields: parent fields, then derived fields after super()
class P1 { x = 1; y = 2; }
class C1 extends P1 { z = 3; constructor() { super(); (this as any).w = 4; } }
class C2 extends C1 { v = 5; }
show("sub1", new C1());
show("sub2", new C2());

// this escaping before a later field assignment (key order must match node)
let seen: string[] = [];
function peek(o: any) { seen.push(Object.keys(o).join(",")); return 7; }
class Esc {
  a = 1;
  b = peek(this);
  c = 3;
  d = peek(this);
}
show("esc", new Esc());
log.push("seen " + seen.join("|"));

class Esc2 {
  a = 1;
  constructor() { peek(this); (this as any).b = 2; }
}
show("esc2", new Esc2());
log.push("seen2 " + seen.join("|"));

// field value reads earlier field
class Dep { a = 2; b = this.a * 2; c = this.b + 1; }
show("dep", new Dep());

// conditional ctor assignment
class Cond {
  a = 0;
  constructor(f: boolean) {
    if (f) { (this as any).x = 1; }
    (this as any).y = 2;
  }
}
show("cond-t", new Cond(true));
show("cond-f", new Cond(false));

// delete and re-add
const d = new Base();
delete (d as any).b;
(d as any).b = "again";
show("del", d);
(d as any).extra = 1;
show("extra", d);

// numeric-ish and accessor keys
class Acc {
  a = 1;
  get g() { return 5; }
  b = 2;
}
show("acc", new Acc());
class NumKey { 1 = "one"; "two" = 2; three = 3; }
show("numkey", new NumKey());
class NumKey2 { a = 1; 2 = "two"; b = 3; }
show("numkey2", new NumKey2());
class NumKey3 { a = 1; constructor() { (this as any)["7"] = 7; (this as any).b = 2; } }
show("numkey3", new NumKey3());

// empty classes filled afterwards
class Empty {}
const e1: any = new Empty();
for (let i = 0; i < 5; i++) e1["f" + i] = i;
show("empty-loop", e1);
function fill(flag: boolean) {
  const o: any = new Empty();
  o.p = 1;
  if (flag) o.q = 2;
  o.r = 3;
  return o;
}
show("empty-cond-t", fill(true));
show("empty-cond-f", fill(false));
class Sub extends Empty { s = 1; }
show("empty-sub", new Sub());
class Sub2 extends Empty { }
const s2: any = new Sub2();
s2.m = 1; s2.n = 2;
show("empty-sub2", s2);

// objects created by a loop of many instances
const many: any[] = [];
for (let i = 0; i < 100; i++) { const b = new Base(); b.a = i; if (i % 2) (b as any).odd = i; many.push(b); }
log.push("many " + many.map(m => keys(m)).filter((v, i, a) => a.indexOf(v) === i).join(" | "));

// Object.entries / for-in / spread / assign order
const sp = { ...new C2() };
log.push("spread " + keys(sp));
const forin: string[] = [];
for (const k in new C2()) forin.push(k);
log.push("forin " + forin.join(","));
log.push("json " + JSON.stringify(new C2()));
console.log(log.join("\n"));
