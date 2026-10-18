// Inherited and late fields, protocol effects, and invalidated correlations.
function copy(t: any, o: any) { for (const k in o) t[k] = o[k]; return t; }
const proto: any = {inherited: 10};
const source: any = Object.create(proto); source.own = 20;
const result: any = copy({}, source);
console.log(result.inherited, result.own, Object.keys(result).sort().join(','));
function mutate(t: any, o: any, other: any) {
  for (let k in o) { o = other; k = 'replacement'; t[k] = o[k]; }
  return t;
}
console.log(mutate({}, {old: 1}, {replacement: 30}).replacement);
function usedValue(t: any, o: any) { let sum = 0; for (const k in o) sum += (t[k] = o[k]); return sum; }
console.log(usedValue({}, {a: 4, b: 5}));
let calls = 0;
const accessorSource: any = {get a() { calls++; return 40; }};
console.log(copy({}, accessorSource).a, calls);
const target: any = {};
function forward(to: any, from: any) { for (const k in from) Object.defineProperty(to, k, {get: () => from[k], enumerable: true}); }
const live: any = {a: 50}; forward(target, live); live.a = 51;
console.log(target.a);
let other = 0;
function extra(to: any, from: any) { for (const k in from) Object.defineProperty(to, k, {get: from[k], enumerable: (other++, true)}); }
const getters: any = {a: () => 60, b: () => 61}; const exports: any = {}; extra(exports, getters);
console.log(exports.a, exports.b, other);
let enumerations = 0; let reads = 0;
const proxied: any = new Proxy({x: 81}, {
  ownKeys(t) { enumerations++; return Reflect.ownKeys(t); },
  get(t, k, r) { reads++; return Reflect.get(t, k, r); }
});
console.log(copy({}, proxied).x, enumerations, reads);
const keyOnly = {get x() { reads++; return 82; }};
let keyCount = 0; for (const k in keyOnly) keyCount++;
console.log(keyCount, reads);
function movedSource(t: any, o: any) {
  for (const k in o) Object.defineProperty(t, k, {get: () => o[k]});
  o = {};
  return t;
}
console.log(movedSource({}, {x: 83}).x === undefined);
class AliasLeaf { value = 84; }
function reboundRead(t: any, o: any, other: any) {
  let a = o;
  for (const k in o) { a = other; t[k] = a[k]; }
  return t;
}
function reboundEnumeration(t: any, o: any, other: any) {
  let a = o; a = other;
  for (const k in a) t[k] = o[k];
  return t;
}
try { console.log(reboundRead({p:new AliasLeaf()}, {p:new AliasLeaf()}, {q:1}).p.value); }
catch { console.log('read-alias-threw'); }
try { console.log(reboundEnumeration({q:new AliasLeaf()}, {p:new AliasLeaf()}, {q:1}).q.value); }
catch { console.log('enumeration-alias-threw'); }
