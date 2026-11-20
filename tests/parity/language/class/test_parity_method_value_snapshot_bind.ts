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

// A method read off `this` is the plain inherited function: it does NOT
// capture the receiver (node: a bare call runs with `this` undefined), and
// replacing the own property afterwards changes nothing about the value
// already read.
class C {
  tag = "t";
  r(): string { const captured = (this as any).m; (this as any).m = "SHADOW"; return captured.call(this); }
  other(): string { const captured = (this as any).m; return captured.call({ tag: "o" }); }
  same(): boolean { return (this as any).m === C.prototype.m; }
  m() { return "C:" + this.tag; }
}
if (new C().r() !== "C:t") throw new Error("captured value lost after own replacement: " + new C().r());
if (new C().other() !== "C:o") throw new Error("this.m captured its receiver: " + new C().other());
if (!new C().same()) throw new Error("this.m is not the prototype's method");

class D {
  tag = "d";
  constructor() { (this as any).m = () => "OWN:" + this.tag; }
  m() { return "PROTO"; }
}
if (new D().m() !== "OWN:d") throw new Error("arrow override regressed");

function plain(this: any, x: number) { return this.base + x; }
if (plain.bind({ base: 100 })(5) !== 105) throw new Error("plain bind");
console.log("OK");
