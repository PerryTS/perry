import { stringify } from "node:querystring";

const upper = (s: string): string => s.toUpperCase();
console.log(stringify({ a: "hello", b: "world" }, undefined, undefined, { encodeURIComponent: upper }));

const tag = (s: string): string => `<${s}>`;
console.log(stringify({ x: "1", y: ["2", "3"] }, undefined, undefined, { encodeURIComponent: tag }));
