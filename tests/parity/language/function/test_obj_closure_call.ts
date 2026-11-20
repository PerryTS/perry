function maker(x: number): number { return x * 2; }
const obj = { f: maker };
console.log("typeof obj.f:", typeof obj.f);
console.log("obj.f(10):", obj.f(10));
