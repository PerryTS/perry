// Generic helper spellings are witnesses, never analysis selectors.
class Leaf { value: number; constructor(value: number) { this.value = value; } }
const __defProp = Object.defineProperty;
function __export(target: any, all: any) {
  for (const name in all) __defProp(target, name, { get: all[name], enumerable: true });
  return target;
}
function __copyProps(to: any, from: any) {
  for (const key in from) __defProp(to, key, { get: () => from[key], enumerable: true });
  return to;
}
function __assign(t: any, s: any) { for (const p in s) t[p] = s[p]; return t; }
function _extends(t: any, s: any) { for (const k in s) t[k] = s[k]; return t; }
function pipe(value: any, f: any) { return f(value); }
function dual(arity: number, body: any) { return (...args: any[]) => args.length >= arity ? body(...args) : (self: any) => body(self, ...args); }
function forEachChild(node: any, visit: any) { visit(node.left); visit(node.right); }
const a = __export({}, {one: () => new Leaf(11), two: () => new Leaf(12)});
const b = __export({}, {three: () => new Leaf(13)});
console.log(a.one.value, a.two.value, b.three.value, Object.keys(a).join(','), Object.keys(b).join(','));
const c = __copyProps({}, {one: new Leaf(21), two: new Leaf(22)});
const d = __assign({}, {left: new Leaf(31), right: new Leaf(32)});
const e = _extends({}, {left: new Leaf(41), right: new Leaf(42)});
console.log(c.one.value, c.two.value, d.left.value, d.right.value, e.left.value, e.right.value);
console.log(pipe(new Leaf(51), (x: any) => x.value));
const add = dual(2, (self: any, n: number) => self.value + n);
console.log(add(new Leaf(61), 1), add(2)(new Leaf(62)));
let sum = 0;
forEachChild({left: new Leaf(71), right: new Leaf(72)}, (x: any) => { sum += x.value; });
console.log(sum);
