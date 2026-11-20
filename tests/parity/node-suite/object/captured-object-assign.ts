const assign = Object.assign;

console.log("assign typeof:", typeof assign);
console.log("assign name:", assign.name);
console.log("assign length:", assign.length);

const target: any = { base: 1 };
const out: any = assign(target, { a: 2 }, { b: 3 });

console.log("same target:", out === target);
console.log("keys:", Object.keys(out).join(","));
console.log("values:", [out.base, out.a, out.b].join(","));
console.log("fresh copy:", JSON.stringify(assign({}, { x: 1 }, { y: 2 })));
