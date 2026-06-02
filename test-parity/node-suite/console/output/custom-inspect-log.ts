import { inspect } from "node:util";
const obj = { name: "p", [inspect.custom]: () => "x" };
console.log(obj);
