import { format } from "node:util";

const circular: any = { name: "c" };
circular.self = circular;
console.log(format("s:%s d:%d i:%i f:%f", "x", "12.5", "12.5", "12.5"));
console.log(format("j:%j", circular));
console.log(format("o:%o", { a: 1 }));
console.log(format("O:%O", { a: { b: { c: 1 } } }));
console.log(format("percent:%% extra", "x", 1));
console.log(format("big:%d", 10n));
