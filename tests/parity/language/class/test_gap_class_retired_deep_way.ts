declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }
class Root {
  method(): number { return 7; }
}
class A extends Root {}
class B extends A {}
class C extends B {}
class D extends C {}
class E extends D {}
class F extends E {}
class G extends F {}
class Receiver extends G { own = 1; }

function read(r: any): any { return r.method; }
const survivors: any[] = [];
for (let i = 0; i < 16; i++) {
  const r: any = new Receiver();
  r["live" + i] = i;
  survivors.push(r);
  console.log("prime", i, read(r).call(r));
}
for (let generation = 0; generation < 32; generation++) {
  let r: any = new Receiver();
  r["temp" + generation] = generation;
  console.log("temporary", generation, read(r).call(r));
  r = null;
  collect();
  console.log("survivor", read(survivors[generation % 16]).call(survivors[generation % 16]));
}
Root.prototype.method = function(): number { return 11; };
console.log("replacement", read(survivors[0]).call(survivors[0]));
Object.setPrototypeOf(survivors[1], { method() { return 13; } });
console.log("relink", read(survivors[1]).call(survivors[1]));
