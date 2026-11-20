// #11915: prototype mutation and actual method selection.
// DEC 250 locks Array.prototype; the patch lands on a user array prototype
// that the array adopts, so the direct method sites must still see it.
const ap: any = Object.create(Array.prototype);
function patch(p: any, key: string, value: any) { Reflect.set(p, key, value); }
patch(ap, 'push', function (v: number) { return 701 + v; });
patch(ap, 'map', function (fn: any) { return [801]; });
patch(ap, 'indexOf', function (v: number) { return 901 + v; });
const a: number[] = Object.setPrototypeOf([1, 2], ap);
console.log(a.push(3));
console.log(a.map(x => x + 1).join(','));
console.log(a.indexOf(2));
console.log(a.length);
