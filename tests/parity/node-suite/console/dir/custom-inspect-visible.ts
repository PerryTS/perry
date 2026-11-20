import { inspect } from "node:util";
const custom = { foo: "bar", [inspect.custom]: () => "inspect" };
console.dir(custom);
