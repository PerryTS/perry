class A {
  tag = "a";
  m() { return "M:" + this.tag; }
  constructor() { (this as any).m = (this as any).m.bind(this); }
}
if (new A().m() !== "M:a") throw new Error("self-bind own: " + new A().m());

class Base {
  m() { return "B:" + (this as any).tag; }
  constructor() { (this as any).m = (this as any).m.bind(this); }
}
class Child extends Base { tag = "c"; }
if (new Child().m() !== "B:c") throw new Error("self-bind inherited: " + new Child().m());

class C {
  tag = "t";
  r(): string { const captured = (this as any).m; (this as any).m = "SHADOW"; return captured(); }
  m() { return "C:" + this.tag; }
}
if (new C().r() !== "C:t") throw new Error("this-snapshot regressed: " + new C().r());

class D {
  tag = "d";
  constructor() { (this as any).m = () => "OWN:" + this.tag; }
  m() { return "PROTO"; }
}
if (new D().m() !== "OWN:d") throw new Error("arrow override regressed");

function plain(this: any, x: number) { return this.base + x; }
if (plain.bind({ base: 100 })(5) !== 105) throw new Error("plain bind");
console.log("OK");
