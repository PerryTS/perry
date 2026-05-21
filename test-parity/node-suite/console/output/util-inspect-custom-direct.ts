import { inspect } from "node:util";
const obj = { id: 1, [inspect.custom]: () => "<hooked>" };
console.log(inspect(obj));
console.log(inspect(obj, { customInspect: false }));
