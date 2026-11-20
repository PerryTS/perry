// Prototype lookup and argument mutation must use the method read before args.
class Holder { children: number[] = []; }
function append(h: Holder, value: () => number): void { h.children.push(value()); }
const h = new Holder();
let log = '';
// DEC 250 (intrinsic-only for this half): the push replace on Array.prototype
// is refused (expected file); the user-prototype half below keeps the order check.
try { Array.prototype.push = function(): number { return 123; }; log += 'no throw;'; } catch (e) { log += (e instanceof TypeError ? 'locked;' : 'other;'); console.log('DEC 250: ' + (e as Error).message); }
append(h, () => { log += 'arg;'; return 5; });
console.log('prototype lock', log, h.children.length);

const custom = new Holder();
const proto = { push(v: number) { log += 'custom:' + v + ';'; return 789; } };
Object.setPrototypeOf(custom.children, proto);
log = '';
append(custom, () => 6);
console.log('custom prototype', log, custom.children.length);
