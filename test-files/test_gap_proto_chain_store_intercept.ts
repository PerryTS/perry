// [[Set]] on an instance whose prototype chain is several ordinary objects deep:
// a setter, a read-only data property or a frozen link anywhere in the chain
// must intercept exactly as in node.
const log: string[] = [];
const root: any = { base: 1 };
Object.defineProperty(root, 'deepSetter', {
  set(v: number) { log.push('deepSetter ' + v); },
  get() { return 0; },
  configurable: true,
});
Object.defineProperty(root, 'deepReadOnly', { value: 7, writable: false, configurable: true });
const mid: any = Object.create(root);
mid.midKey = 1;
const top: any = Object.create(mid);
top.topKey = 2;
const a: any = Object.create(top);
a.deepSetter = 5;
try { a.deepReadOnly = 9; } catch (e) { log.push('ro threw'); }
a.fresh = 3;
a.midKey = 4;
console.log(log.join('|'), Object.keys(a).join(','), a.deepReadOnly, a.midKey, mid.midKey);

const frozenMid: any = Object.create(root);
frozenMid.k = 1;
Object.freeze(frozenMid);
const b: any = Object.create(frozenMid);
try { b.k = 2; } catch (e) { log.push('frozen threw'); }
b.other = 3;
console.log(Object.keys(b).join(','), b.k);

const late: any = Object.create(top);
late.x = 1;
Object.defineProperty(mid, 'x', { set(v: number) { log.push('late x ' + v); }, configurable: true });
late.x = 2;
const c: any = Object.create(top);
c.x = 3;
console.log(log.join('|'), Object.keys(late).join(','), Object.keys(c).join(','));

const reparent: any = Object.create(top);
Object.setPrototypeOf(top, { deepSetter: 1 });
reparent.deepSetter = 8;
console.log(Object.keys(reparent).join(','), log.length);
