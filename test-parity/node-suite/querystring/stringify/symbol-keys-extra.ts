import querystring from "node:querystring";

const sym = Symbol("s");
const obj: any = { a: 1 };
obj[sym] = 2;
Object.defineProperty(obj, "hidden", { value: 3, enumerable: false });
console.log("stringify:", querystring.stringify(obj));
console.log("keys:", Object.keys(obj).join(","));
