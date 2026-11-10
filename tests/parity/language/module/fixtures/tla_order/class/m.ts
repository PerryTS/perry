let runs = 0;
function make() { runs += 1; console.log("class", runs); return runs; }
console.log("m1");
await Promise.resolve();
export class C { static value = make(); }
console.log("m2");
