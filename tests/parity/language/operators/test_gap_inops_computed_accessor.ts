let keyEvaluations = 0;
const options = { prop: "value" };
function key() { keyEvaluations++; return options.prop; }
let gets = 0;
let sets = 0;
class Computed {
  backing = 7;
  get [key()]() { gets++; return this.backing; }
  set [key()](v: number) { sets++; this.backing = v; }
}
const o: any = new Computed();
let sum = 0;
for (let i = 0; i < 30; i++) { o.value = i; sum += o.value; }
options.prop = "other";
console.log(sum, gets, sets, keyEvaluations, o.other);
Object.defineProperty(Computed.prototype, "value", { get() { return 101; }, configurable: true });
console.log(o.value);
Object.defineProperty(o, "value", { value: undefined, configurable: true });
console.log(o.value, "value" in o, Object.hasOwn(o, "value"));
delete o.value;
console.log(o.value);
delete Computed.prototype.value;
console.log(o.value, "value" in o);
class Thrower { get [options.prop]() { throw new Error("computed"); } }
try { console.log((new Thrower() as any).other); } catch (e) { console.log(e.message); }
const spilledKey = "spilled";
const fields: any = {};
for (let i = 0; i < 16; i++) fields["field" + i] = i;
const spilledProto: any = { ...fields, get [spilledKey]() { return this.base; }, set [spilledKey](v: number) { this.base = v; } };
const spilled: any = Object.create(spilledProto);
spilled.base = 11;
function readSpilled(o: any) { return o.spilled; }
let spilledSum = 0;
for (let i = 0; i < 30; i++) { spilled.spilled = i; spilledSum += readSpilled(spilled); }
console.log("spill", spilledSum);
Object.defineProperty(spilledProto, spilledKey, { get() { return this.base + 100; }, configurable: true });
console.log(readSpilled(spilled));
delete spilledProto.spilled;
console.log(readSpilled(spilled));

let shadowSets = 0;
class ShadowSetter { set answer(v: number) { shadowSets++; } }
const shadow: any = new ShadowSetter();
function assignShadow(o: any, v: number) { o.answer = v; }
for (let i = 0; i < 30; i++) assignShadow(shadow, i);
Object.defineProperty(shadow, "answer", { value: 100, writable: true, configurable: true });
assignShadow(shadow, 500);
console.log("setter-shadow", shadowSets, shadow.answer);
delete shadow.answer;
assignShadow(shadow, 600);
console.log("setter-restored", shadowSets, shadow.answer);
