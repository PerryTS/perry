// #11915: prototype mutation and actual method selection.
function run(a: number[]) { console.log(a.indexOf(2)); }
const a = [1, 2];
run(a);
function install(p: any, key: string) { p[key] = function () { return 77; }; }
install(Object.getPrototypeOf(a), 'indexOf');
run(a);
