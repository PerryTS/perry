const key = Symbol('owned-data');
const owner: any = Promise.resolve(12);
owner[key] = {value: 37};
for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
console.log(owner[key].value);
console.log(Object.getOwnPropertySymbols(owner).length);
console.log((Promise.resolve(1) as any)[key] === undefined);
console.log(delete owner[key], owner[key] === undefined);
