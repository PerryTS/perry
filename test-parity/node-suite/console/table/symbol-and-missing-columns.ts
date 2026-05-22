const sym = Symbol("s");
const row: any = { a: 1, b: 2 };
row[sym] = 3;
console.table([row], ["a", "missing"]);
console.table(new Map([["x", { a: 1 }], ["y", { a: 2, b: 3 }]]));
