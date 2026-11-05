const key = Symbol('shared accessor');
function getter(this: any) { return this.value; }
function setter(this: any, value: number) { this.value = value; }
function check(owner: any, label: string) {
  owner.value = 3;
  Object.defineProperty(owner, key, { get: getter, set: setter, configurable: true, enumerable: true });
  owner[key] = 7;
  const d = Object.getOwnPropertyDescriptor(owner, key)!;
  console.log(label, owner[key], d.get === getter, d.set === setter, d.enumerable, d.configurable);
  const child: any = Object.create(owner);
  child.value = 11;
  child[key] = 13;
  console.log('inherited', child[key], owner[key], Object.hasOwn(child, key));
  Object.defineProperty(owner, key, { get: undefined, set: setter });
  owner[key] = 19;
  console.log('setter only', owner[key] === undefined, owner.value);
  Object.defineProperty(owner, key, { get: getter, set: undefined });
  console.log('getter only', owner[key], Object.getOwnPropertyDescriptor(owner, key)!.set === undefined);
  delete owner[key];
  owner[key] = 23;
  console.log('data again', owner[key], Object.getOwnPropertyDescriptor(owner, key)!.value);
}
check({}, 'object');
check([], 'array');
check(function holder() {}, 'function');

const a: any = [1, 2];
let gets = 0;
Object.defineProperty(a, Symbol.iterator, {
  get() { gets++; return Array.prototype.values; }, configurable: true
});
let total = 0;
for (const value of a) total += value;
console.log('iterator accessor', total, gets);
Object.defineProperty(a, Symbol.iterator, { get: undefined });
try { for (const value of a) total += value; }
catch (e) { console.log('missing iterator', e instanceof TypeError); }
