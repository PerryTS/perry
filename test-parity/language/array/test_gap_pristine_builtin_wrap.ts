// #11915: the common monkey-patch wrapper calls the saved original.
// DEC 250 locks Array.prototype: the arrays adopt a user array prototype UAP
// (below Array.prototype) that the wrappers are installed on. A primitive
// string's slice exists only on String.prototype (intrinsic-only): that
// wrapper is refused (expected file).
const UAP: any = Object.create(Array.prototype);
const origPush = Array.prototype.push;
let calls = 0;
UAP.push = function (this: any[], ...items: any[]) {
  calls++;
  return origPush.apply(this, items);
};
const a: number[] = Object.setPrototypeOf([1], UAP);
function add(x: number[], v: number) { return x.push(v); }
console.log(add(a, 2), add(a, 3), a.join(','), calls);
const origJoin = Array.prototype.join;
UAP.join = function (this: any[], sep?: string) {
  return '<' + origJoin.call(this, sep) + '>';
};
console.log(a.join('-'), Object.setPrototypeOf([7, 8], UAP).join('+'));
const origSlice = String.prototype.slice;
try {
  (String.prototype as any).slice = function (this: string, s?: number, e?: number) {
    return '[' + origSlice.call(this, s, e) + ']';
  };
} catch (e) { console.log("DEC 250: " + (e as Error).message); }
function cut(s: string) { return s.slice(1, 3); }
console.log(cut('abcdef'));
