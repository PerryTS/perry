import { parse } from "node:querystring";

const upper = (s: string): string => s.toUpperCase();
console.log(JSON.stringify(parse("a=hello&b=world", undefined, undefined, { decodeURIComponent: upper })));

const tag = (s: string): string => `<${s}>`;
console.log(JSON.stringify(parse("x=1&y=2&x=3", undefined, undefined, { decodeURIComponent: tag })));
