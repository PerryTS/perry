import * as util from "node:util";
const custom = { foo: "bar", [util.inspect.custom]: () => "inspect" };
console.dir(custom);
