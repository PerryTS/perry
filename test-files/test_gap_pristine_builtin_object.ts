// #11915: prototype mutation and actual method selection.
function run(x: any) { console.log(x.hasOwnProperty('x')); }
run({x: 1});
const op: any = Object.prototype;
op.hasOwnProperty = function (key: string) { return 'patched:' + key; };
run({x: 1});
run([]);
run(new Map());
console.log(Object.prototype.hasOwnProperty.call({x: 1}, 'x'));
