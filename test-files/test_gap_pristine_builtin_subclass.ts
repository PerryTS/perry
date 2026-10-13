// #11915: prototype mutation and actual method selection.
class A extends Array<number> {
  map(fn: any): any { return ['override']; }
  indexOf(value: number): number { return 123; }
}
const a = new A(1, 2);
console.log(a.map(x => x + 1).join(','));
console.log(a.indexOf(1));
console.log(Array.prototype.indexOf.call(a, 1));
