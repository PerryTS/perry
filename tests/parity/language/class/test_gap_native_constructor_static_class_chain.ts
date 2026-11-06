class Base {
  static value: any = 17;
  static method() { return this.value; }
  static get receiver() { return this; }
}
class Mid extends Base {}
class Leaf extends Mid {}
const B: any = Base;
const M: any = Mid;
const L: any = Leaf;
console.log('inherit', L.value, L.method === B.method, L.method(), L.receiver === L);
M.value = 23;
console.log('shadow', L.value, L.method(), B.value);
delete M.value;
console.log('delete', L.value, L.method());
M.value = undefined;
console.log('undefined', typeof L.value, typeof L.method());
M.value = null;
console.log('null', L.value === null, L.method() === null);
delete M.value;
const replacement = { value: 31, method() { return this.value + 1; } };
Object.setPrototypeOf(M, replacement);
console.log('reparent', L.value, L.method(), L.method === replacement.method, typeof L.receiver);
Object.setPrototypeOf(M, null);
console.log('null-parent', typeof L.value, typeof L.method);
Object.setPrototypeOf(M, B);
console.log('restore', L.value, L.method(), L.receiver === L);

class One extends Base {}
function callOne() { return One.method(); }
console.log('one-link-warm', callOne());
Object.setPrototypeOf(One, replacement);
console.log('one-link-reparent', callOne(), (One as any).method === replacement.method);
Object.setPrototypeOf(One, B);
console.log('one-link-restore', callOne());
