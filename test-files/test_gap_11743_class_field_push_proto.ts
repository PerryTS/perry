// Prototype lookup and argument mutation must use the method read before args.
class Holder { children: number[] = []; }
function append(h: Holder, value: () => number): void { h.children.push(value()); }
const h = new Holder();
let log = '';
const saved = Array.prototype.push;
Array.prototype.push = function(v: number): number {
  log += 'old:' + v + ':' + (this === h.children) + ';';
  return 123;
};
append(h, () => {
  log += 'arg;';
  Array.prototype.push = function(): number { log += 'new;'; return 456; };
  return 5;
});
Array.prototype.push = saved;
console.log('prototype snapshot', log, h.children.length);

const custom = new Holder();
const proto = { push(v: number) { log += 'custom:' + v + ';'; return 789; } };
Object.setPrototypeOf(custom.children, proto);
log = '';
append(custom, () => 6);
console.log('custom prototype', log, custom.children.length);
