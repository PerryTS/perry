// #11915: prototype mutation and actual method selection.
function run(x: any) { console.log(x.hasOwnProperty('x')); }
run({x: 1});
// DEC 250 locks Object.prototype; the patch lands on a user prototype that
// the later receiver adopts.
const op: any = Object.create(Object.prototype);
op.hasOwnProperty = function (key: string) { return 'patched:' + key; };
run(Object.setPrototypeOf({x: 1}, op));
run([]);
run(new Map());
console.log(Object.prototype.hasOwnProperty.call({x: 1}, 'x'));
