// #11915: prototype mutation and actual method selection.
function run(s: string) { console.log(s.slice(1, 3)); }
run('abcd');
// DEC 250 (intrinsic-only): a primitive string's methods come only from
// String.prototype. The patch is refused
// now (expected file); the reads after it still run.
const sp: any = String.prototype;
try { sp.slice = function (a: number, b: number) { return 'patched:' + a + ':' + b; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
run('abcd');
console.log('abcd'.slice(1, 3));
