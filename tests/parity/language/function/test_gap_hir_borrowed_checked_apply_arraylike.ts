const list: any = { 0: 3, 1: 7, length: 2 };
list[Symbol.iterator] = function* () { yield 99; };
console.log(Math.max.apply(null, list));
let effects = 0;
function extra() { effects++; return 0; }
console.log((Math.max.apply as any)(null, [3, 7], extra()), effects);
