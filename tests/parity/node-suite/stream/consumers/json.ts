import { Readable } from "node:stream";
import { json } from "node:stream/consumers";

const value = await json(Readable.from(['{"a":', "1", ',"b":"ok"}']));
console.log("json a:", (value as { a: number }).a);
console.log("json b:", (value as { b: string }).b);
