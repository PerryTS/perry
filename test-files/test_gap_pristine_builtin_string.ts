// #11915: prototype mutation and actual method selection.
function run(s: string) { console.log(s.slice(1, 3)); }
run('abcd');
const sp: any = String.prototype;
sp.slice = function (a: number, b: number) { return 'patched:' + a + ':' + b; };
run('abcd');
console.log('abcd'.slice(1, 3));
