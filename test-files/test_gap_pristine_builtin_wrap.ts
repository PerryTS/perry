// #11915: the common monkey-patch wrapper calls the saved original.
const origPush = Array.prototype.push;
let calls = 0;
(Array.prototype as any).push = function (this: any[], ...items: any[]) {
  calls++;
  return origPush.apply(this, items);
};
const a: number[] = [1];
function add(x: number[], v: number) { return x.push(v); }
console.log(add(a, 2), add(a, 3), a.join(','), calls);
const origJoin = Array.prototype.join;
(Array.prototype as any).join = function (this: any[], sep?: string) {
  return '<' + origJoin.call(this, sep) + '>';
};
console.log(a.join('-'), [7, 8].join('+'));
const origSlice = String.prototype.slice;
(String.prototype as any).slice = function (this: string, s?: number, e?: number) {
  return '[' + origSlice.call(this, s, e) + ']';
};
function cut(s: string) { return s.slice(1, 3); }
console.log(cut('abcdef'));
