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

function absent(r: any): any { return r.missing; }
let checksum = 0;
for (let generation = 0; generation < 40; generation++) {
  let r: any = new Receiver();
  r["own" + generation] = generation;
  for (let i = 0; i < 20; i++) if (absent(r) === undefined) checksum++;
  r = null;
  collect();
}
const fresh: any = new Receiver();
console.log("absent", checksum, absent(fresh) === undefined);
(Root.prototype as any).missing = 42;
console.log("holder", absent(fresh));
delete (Root.prototype as any).missing;
fresh.missing = 19;
console.log("own", absent(fresh));
