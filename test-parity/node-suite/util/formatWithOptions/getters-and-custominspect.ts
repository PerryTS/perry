import { formatWithOptions, inspect } from "node:util";

const obj: any = {};
Object.defineProperty(obj, "x", { get() { return 7; }, enumerable: true });
obj[inspect.custom] = () => "CUSTOM";
console.log(formatWithOptions({ getters: true }, "%O", obj));
console.log(formatWithOptions({ customInspect: false }, "%O", obj));

const literal: any = { marker: true, [inspect.custom]: () => "LITERAL" };
console.log(formatWithOptions({ customInspect: false }, "%O", literal));
