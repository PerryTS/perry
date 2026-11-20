import { parse } from "node:querystring";

const parsed = parse("a=b=c&d=e&x==y");
console.log("a:", parsed.a);
console.log("d:", parsed.d);
console.log("x:", parsed.x);
