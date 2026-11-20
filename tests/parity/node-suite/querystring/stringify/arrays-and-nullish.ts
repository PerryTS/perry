import querystring from "node:querystring";

console.log("array:", querystring.stringify({ a: ["x", "y"], b: 1 }));
console.log("nullish:", querystring.stringify({ a: null as any, b: undefined as any, c: false }));
console.log("object value:", querystring.stringify({ a: { x: 1 } as any }));
console.log("custom sep:", querystring.stringify({ a: 1, b: 2 }, ";", ":"));
