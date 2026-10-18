import { AWDForeign } from "./_helpers/AWD-layout-method-base.ts";
// Unknown receiver calls must observe every change to ordinary [[Get]].
class AWDBase {
  value: number;
  constructor(value: number) { this.value = value; }
  m(x: number) { return this.value + x; }
}
class AWDChild extends AWDBase {
  m(x: number) { return super.m(x) * 2; }
}
function call(o: any, x: number) { return o.m(x); }
const a: any = new AWDBase(10);
const b: any = new AWDChild(20);
for (let i = 0; i < 5; i++) console.log('warm', call(a, i), call(b, i));
a.m = function(x: number) { return this.value - x; };
console.log('own', call(a, 3), call(b, 3));
delete a.m;
AWDBase.prototype.m = function(x: number) { return this.value + x + 100; };
console.log('replace', call(a, 3), call(b, 3));
Object.setPrototypeOf(a, { m(x: number) { return this.value * x; } });
console.log('relink', call(a, 3), call(b, 3));
Object.defineProperty(AWDChild.prototype, 'm', {
  configurable: true,
  get() { console.log('getter', this.value); return function(x: number) { return this.value + x + 200; }; }
});
console.log('accessor', call(b, 3));
const p: any = new Proxy(b, { get(target, key, receiver) {
  console.log('proxy', String(key));
  return Reflect.get(target, key, receiver);
} });
console.log('proxy-call', call(p, 3));
let hits = 0;
const c: any = new AWDBase(30);
Object.defineProperty(c, 'm', { get() { hits++; return function(x: number) { return this.value + x; }; } });
console.log('own-accessor', call(c, 4), hits);
// Getter evaluation must precede an argument that changes the method.
const d: any = new AWDBase(40);
function replace(): number { d.m = function(x: number) { return -x; }; return 2; }
console.log('split', d.m(replace()), d.m(2));

// Preselection must retain the old public body after a layout mutation.
class AWDStable {
  value: number;
  constructor(value: number) { this.value = value; }
  keep(x: number) { return this.value + x; }
}
const stable: any = new AWDStable(50);
function changeOwn(): number {
  stable.keep = function(x: number) { return -x; };
  stable.extra = 1;
  return 3;
}
console.log("selected", stable.keep(changeOwn()), stable.keep(3));
// A foreign callable candidate needs an actual LLVM declaration too.
const foreign: any = new AWDForeign(60);
function changeForeign(): number {
  AWDForeign.prototype.external = function(x: number) { return -x; };
  return 4;
}
console.log("foreign", foreign.external(changeForeign()), foreign.external(4));

// Rebinding the source variable in an argument preserves the old receiver.
let snapshot: any = new AWDStable(70);
function rebindReceiver(): number { snapshot = 0; return 4; }
console.log("receiver-snapshot", snapshot.keep(rebindReceiver()), snapshot);
