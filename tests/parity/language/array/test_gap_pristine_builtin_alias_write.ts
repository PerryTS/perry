// #11915: prototype mutation and actual method selection.
function run(a: number[]) { console.log(a.indexOf(2)); }
// DEC 250 locks Array.prototype; the patch lands on a user array prototype
// that the array adopts, so the direct method sites must still see it.
const a: number[] = Object.setPrototypeOf([1, 2], Object.create(Array.prototype));
run(a);
function install(p: any, key: string) { p[key] = function () { return 77; }; }
install(Object.getPrototypeOf(a), 'indexOf');
run(a);
