// Every collection kind behind every collection annotation, and the controls node answers through the
// ordinary Get + Call: a parameter (the HIR folds its Map / Set calls) and a class field (`this.k.get(K)`,
// `this.k.size`: the calls the HIR leaves unfolded) annotated Map / Set / WeakMap / WeakSet, given a Map, a Set, a
// WeakMap, a WeakSet, a plain object with same-named members, a Map subclass instance and null, for get / set / has /
// delete / add / clear / size / forEach / keys / values / entries (Symbol.iterator and for-of: probe cm, one case per
// process; see the PR). Before, a WeakMap behind
// `Map<K, V>` threw on get / has (node: undefined / false) and the field form answered a non-Map's `size` with 0.
// Negative controls: the real collection behind its own annotation, and a field that only holds real Maps.
const K = {};
class SubMap extends Map<any, any> {}
function mk(v: string): any {
  switch (v) {
    case 'map': { const m = new Map<any, any>(); m.set(K, 1); return m; }
    case 'set': { const s = new Set<any>(); s.add(K); return s; }
    case 'wmap': { const m = new WeakMap<object, any>(); m.set(K, 1); return m; }
    case 'wset': { const s = new WeakSet<object>(); s.add(K); return s; }
    case 'plain': return { get: () => 'pg', set: () => 'ps', has: () => 'ph', delete: () => 'pd', add: () => 'pa', clear: () => 'pc', size: 'psz', forEach: (f: any) => { f(); }, keys: () => [1], values: () => [1, 2], entries: () => [], [Symbol.iterator]: function* () { yield 1; } };
    case 'sub': { const m = new SubMap(); m.set(K, 1); return m; }
  }
  return null;
}
function show(r: any): string { return typeof r === 'object' && r !== null ? 'obj:' + Object.prototype.toString.call(r) : String(r); }
const F: [string, (v: any) => any][] = [];
F.push(['M.get', (x: Map<any, any>): any => { return x.get(K); }]);
class G_M_get { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.get(K); } }
F.push(['M.get.field', (v: any): any => new G_M_get(v).run()]);
F.push(['M.set', (x: Map<any, any>): any => { const r = x.set(K, 5); return r === x ? 'self' : r; }]);
class G_M_set { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.set(K, 5); return r === this.k ? 'self' : r; } }
F.push(['M.set.field', (v: any): any => new G_M_set(v).run()]);
F.push(['M.has', (x: Map<any, any>): any => { return x.has(K); }]);
class G_M_has { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.has(K); } }
F.push(['M.has.field', (v: any): any => new G_M_has(v).run()]);
F.push(['M.delete', (x: Map<any, any>): any => { return x.delete(K); }]);
class G_M_delete { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.delete(K); } }
F.push(['M.delete.field', (v: any): any => new G_M_delete(v).run()]);
F.push(['M.add', (x: Map<any, any>): any => { const r = x.add(K); return r === x ? 'self' : r; }]);
class G_M_add { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.add(K); return r === this.k ? 'self' : r; } }
F.push(['M.add.field', (v: any): any => new G_M_add(v).run()]);
F.push(['M.clear', (x: Map<any, any>): any => { return x.clear(); }]);
class G_M_clear { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.clear(); } }
F.push(['M.clear.field', (v: any): any => new G_M_clear(v).run()]);
F.push(['M.size', (x: Map<any, any>): any => { return x.size; }]);
class G_M_size { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.size; } }
F.push(['M.size.field', (v: any): any => new G_M_size(v).run()]);
F.push(['M.forEach', (x: Map<any, any>): any => { let n = 0; x.forEach(() => { n++; }); return n; }]);
class G_M_forEach { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { let n = 0; this.k.forEach(() => { n++; }); return n; } }
F.push(['M.forEach.field', (v: any): any => new G_M_forEach(v).run()]);
F.push(['M.keys', (x: Map<any, any>): any => { return [...x.keys()].length; }]);
class G_M_keys { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.keys()].length; } }
F.push(['M.keys.field', (v: any): any => new G_M_keys(v).run()]);
F.push(['M.values', (x: Map<any, any>): any => { return [...x.values()].length; }]);
class G_M_values { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.values()].length; } }
F.push(['M.values.field', (v: any): any => new G_M_values(v).run()]);
F.push(['M.entries', (x: Map<any, any>): any => { return [...x.entries()].length; }]);
class G_M_entries { k: Map<any, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.entries()].length; } }
F.push(['M.entries.field', (v: any): any => new G_M_entries(v).run()]);
F.push(['S.get', (x: Set<any>): any => { return x.get(K); }]);
class G_S_get { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return this.k.get(K); } }
F.push(['S.get.field', (v: any): any => new G_S_get(v).run()]);
F.push(['S.set', (x: Set<any>): any => { const r = x.set(K, 5); return r === x ? 'self' : r; }]);
class G_S_set { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.set(K, 5); return r === this.k ? 'self' : r; } }
F.push(['S.set.field', (v: any): any => new G_S_set(v).run()]);
F.push(['S.has', (x: Set<any>): any => { return x.has(K); }]);
class G_S_has { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return this.k.has(K); } }
F.push(['S.has.field', (v: any): any => new G_S_has(v).run()]);
F.push(['S.delete', (x: Set<any>): any => { return x.delete(K); }]);
class G_S_delete { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return this.k.delete(K); } }
F.push(['S.delete.field', (v: any): any => new G_S_delete(v).run()]);
F.push(['S.add', (x: Set<any>): any => { const r = x.add(K); return r === x ? 'self' : r; }]);
class G_S_add { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.add(K); return r === this.k ? 'self' : r; } }
F.push(['S.add.field', (v: any): any => new G_S_add(v).run()]);
F.push(['S.clear', (x: Set<any>): any => { return x.clear(); }]);
class G_S_clear { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return this.k.clear(); } }
F.push(['S.clear.field', (v: any): any => new G_S_clear(v).run()]);
F.push(['S.size', (x: Set<any>): any => { return x.size; }]);
class G_S_size { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return this.k.size; } }
F.push(['S.size.field', (v: any): any => new G_S_size(v).run()]);
F.push(['S.forEach', (x: Set<any>): any => { let n = 0; x.forEach(() => { n++; }); return n; }]);
class G_S_forEach { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { let n = 0; this.k.forEach(() => { n++; }); return n; } }
F.push(['S.forEach.field', (v: any): any => new G_S_forEach(v).run()]);
F.push(['S.keys', (x: Set<any>): any => { return [...x.keys()].length; }]);
class G_S_keys { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.keys()].length; } }
F.push(['S.keys.field', (v: any): any => new G_S_keys(v).run()]);
F.push(['S.values', (x: Set<any>): any => { return [...x.values()].length; }]);
class G_S_values { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.values()].length; } }
F.push(['S.values.field', (v: any): any => new G_S_values(v).run()]);
F.push(['S.entries', (x: Set<any>): any => { return [...x.entries()].length; }]);
class G_S_entries { k: Set<any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.entries()].length; } }
F.push(['S.entries.field', (v: any): any => new G_S_entries(v).run()]);
F.push(['WM.get', (x: WeakMap<object, any>): any => { return x.get(K); }]);
class G_WM_get { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.get(K); } }
F.push(['WM.get.field', (v: any): any => new G_WM_get(v).run()]);
F.push(['WM.set', (x: WeakMap<object, any>): any => { const r = x.set(K, 5); return r === x ? 'self' : r; }]);
class G_WM_set { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.set(K, 5); return r === this.k ? 'self' : r; } }
F.push(['WM.set.field', (v: any): any => new G_WM_set(v).run()]);
F.push(['WM.has', (x: WeakMap<object, any>): any => { return x.has(K); }]);
class G_WM_has { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.has(K); } }
F.push(['WM.has.field', (v: any): any => new G_WM_has(v).run()]);
F.push(['WM.delete', (x: WeakMap<object, any>): any => { return x.delete(K); }]);
class G_WM_delete { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.delete(K); } }
F.push(['WM.delete.field', (v: any): any => new G_WM_delete(v).run()]);
F.push(['WM.add', (x: WeakMap<object, any>): any => { const r = x.add(K); return r === x ? 'self' : r; }]);
class G_WM_add { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.add(K); return r === this.k ? 'self' : r; } }
F.push(['WM.add.field', (v: any): any => new G_WM_add(v).run()]);
F.push(['WM.clear', (x: WeakMap<object, any>): any => { return x.clear(); }]);
class G_WM_clear { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.clear(); } }
F.push(['WM.clear.field', (v: any): any => new G_WM_clear(v).run()]);
F.push(['WM.size', (x: WeakMap<object, any>): any => { return x.size; }]);
class G_WM_size { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return this.k.size; } }
F.push(['WM.size.field', (v: any): any => new G_WM_size(v).run()]);
F.push(['WM.forEach', (x: WeakMap<object, any>): any => { let n = 0; x.forEach(() => { n++; }); return n; }]);
class G_WM_forEach { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { let n = 0; this.k.forEach(() => { n++; }); return n; } }
F.push(['WM.forEach.field', (v: any): any => new G_WM_forEach(v).run()]);
F.push(['WM.keys', (x: WeakMap<object, any>): any => { return [...x.keys()].length; }]);
class G_WM_keys { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.keys()].length; } }
F.push(['WM.keys.field', (v: any): any => new G_WM_keys(v).run()]);
F.push(['WM.values', (x: WeakMap<object, any>): any => { return [...x.values()].length; }]);
class G_WM_values { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.values()].length; } }
F.push(['WM.values.field', (v: any): any => new G_WM_values(v).run()]);
F.push(['WM.entries', (x: WeakMap<object, any>): any => { return [...x.entries()].length; }]);
class G_WM_entries { k: WeakMap<object, any>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.entries()].length; } }
F.push(['WM.entries.field', (v: any): any => new G_WM_entries(v).run()]);
F.push(['WS.get', (x: WeakSet<object>): any => { return x.get(K); }]);
class G_WS_get { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return this.k.get(K); } }
F.push(['WS.get.field', (v: any): any => new G_WS_get(v).run()]);
F.push(['WS.set', (x: WeakSet<object>): any => { const r = x.set(K, 5); return r === x ? 'self' : r; }]);
class G_WS_set { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.set(K, 5); return r === this.k ? 'self' : r; } }
F.push(['WS.set.field', (v: any): any => new G_WS_set(v).run()]);
F.push(['WS.has', (x: WeakSet<object>): any => { return x.has(K); }]);
class G_WS_has { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return this.k.has(K); } }
F.push(['WS.has.field', (v: any): any => new G_WS_has(v).run()]);
F.push(['WS.delete', (x: WeakSet<object>): any => { return x.delete(K); }]);
class G_WS_delete { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return this.k.delete(K); } }
F.push(['WS.delete.field', (v: any): any => new G_WS_delete(v).run()]);
F.push(['WS.add', (x: WeakSet<object>): any => { const r = x.add(K); return r === x ? 'self' : r; }]);
class G_WS_add { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { const r = this.k.add(K); return r === this.k ? 'self' : r; } }
F.push(['WS.add.field', (v: any): any => new G_WS_add(v).run()]);
F.push(['WS.clear', (x: WeakSet<object>): any => { return x.clear(); }]);
class G_WS_clear { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return this.k.clear(); } }
F.push(['WS.clear.field', (v: any): any => new G_WS_clear(v).run()]);
F.push(['WS.size', (x: WeakSet<object>): any => { return x.size; }]);
class G_WS_size { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return this.k.size; } }
F.push(['WS.size.field', (v: any): any => new G_WS_size(v).run()]);
F.push(['WS.forEach', (x: WeakSet<object>): any => { let n = 0; x.forEach(() => { n++; }); return n; }]);
class G_WS_forEach { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { let n = 0; this.k.forEach(() => { n++; }); return n; } }
F.push(['WS.forEach.field', (v: any): any => new G_WS_forEach(v).run()]);
F.push(['WS.keys', (x: WeakSet<object>): any => { return [...x.keys()].length; }]);
class G_WS_keys { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.keys()].length; } }
F.push(['WS.keys.field', (v: any): any => new G_WS_keys(v).run()]);
F.push(['WS.values', (x: WeakSet<object>): any => { return [...x.values()].length; }]);
class G_WS_values { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.values()].length; } }
F.push(['WS.values.field', (v: any): any => new G_WS_values(v).run()]);
F.push(['WS.entries', (x: WeakSet<object>): any => { return [...x.entries()].length; }]);
class G_WS_entries { k: WeakSet<object>; constructor(v: any) { this.k = v; } run(): any { return [...this.k.entries()].length; } }
F.push(['WS.entries.field', (v: any): any => new G_WS_entries(v).run()]);
for (const v of ['map', 'set', 'wmap', 'wset', 'plain', 'sub', 'null']) {
  let line = v + ':';
  for (const [name, f] of F) {
    let r: string;
    try { r = show(f(mk(v))); } catch (e: any) { r = 'THROW ' + e.name; }
    line += ' ' + name + '=' + r;
  }
  console.log(line);
}
// A field that only ever holds a real Map, and a real WeakMap behind its own annotation.
class Holder {
  table: Map<string, number> = new Map<string, number>();
  run(k: string): string { this.table.set(k, k.length); return this.table.size + '/' + String(this.table.get(k)) + '/' + String(this.table.has('zz')); }
}
const w: WeakMap<object, number> = new WeakMap<object, number>();
w.set(K, 3);
console.log(new Holder().run('abc'), w.get(K), w.has({}), w.delete(K), w.has(K));
