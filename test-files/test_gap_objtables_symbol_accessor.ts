const key = Symbol('owner-accessor');
const owner: any = [];
let writes = 0;
Object.defineProperty(owner, key, {get() { return this.length + 37; }, set(v) { writes = v; }, configurable: true});
for (let i = 0; i < 40000; ++i) { const garbage = {i, a: [i, i + 1]}; if (garbage.i < 0) console.log(garbage); }
console.log(owner[key]);
owner[key] = 12;
console.log(writes);
console.log(Object.getOwnPropertySymbols(owner).length);
console.log(([] as any)[key] === undefined);
console.log(delete owner[key], owner[key] === undefined);
