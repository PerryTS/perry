// #11915: prototype mutation and actual method selection.
const a = [1, 2];
function run() { console.log(a.push(3), a.map(x => x + 1).join(','), a.indexOf(2)); }
run();
const ap: any = Array.prototype;
function patch(p: any, key: string, value: any) { Reflect.set(p, key, value); }
patch(ap, 'push', function (v: number) { return 701 + v; });
patch(ap, 'map', function (fn: any) { return [801]; });
patch(ap, 'indexOf', function (v: number) { return 901 + v; });
run();
