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
let receivers: any[] = [];
let anchor: any = null;
let checksum = 0;
for (let i = 0; i < 128; i++) {
  const r: any = new Receiver();
  r["key" + i] = i;
  receivers.push(r);
  if (i === 16) anchor = r;
  if (absent(r) === undefined) checksum++;
}
for (const r of receivers) if (absent(r) === undefined) checksum++;
receivers = [];
collect();
for (let generation = 0; generation < 160; generation++) {
  let r: any = new Receiver();
  r["new" + generation] = generation;
  if (absent(r) === undefined) checksum++;
  if (absent(anchor) === undefined) checksum++;
  r = null;
  collect();
}
console.log("shared", checksum);
(Root.prototype as any).missing = 23;
console.log("changed", absent(anchor), absent(new Receiver()));
delete (Root.prototype as any).missing;
console.log("restored", absent(anchor) === undefined);
