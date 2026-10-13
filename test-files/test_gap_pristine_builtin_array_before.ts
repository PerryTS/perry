// #11915: prototype mutation and actual method selection.
const ap: any = Array.prototype;
function patch(p: any, key: string, value: any) { Reflect.set(p, key, value); }
patch(ap, 'push', function (v: number) { return 701 + v; });
patch(ap, 'map', function (fn: any) { return [801]; });
patch(ap, 'indexOf', function (v: number) { return 901 + v; });
const a = [1, 2];
console.log(a.push(3));
console.log(a.map(x => x + 1).join(','));
console.log(a.indexOf(2));
console.log(a.length);
