import querystring from "node:querystring";

console.log("number bool:", querystring.stringify({ a: 1, b: true, c: false }));
console.log("bigint symbol:", querystring.stringify({ a: 1n as any, b: Symbol("s") as any }));
console.log("date regexp:", querystring.stringify({ d: new Date(0) as any, r: /x/ as any }));
