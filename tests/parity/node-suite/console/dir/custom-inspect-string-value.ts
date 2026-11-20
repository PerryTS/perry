import { inspect } from "node:util";
const obj = { id: 1, [inspect.custom]: "not-a-function" };
console.dir(obj);
