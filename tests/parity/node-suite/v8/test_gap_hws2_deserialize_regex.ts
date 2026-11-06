import { deserialize } from "node:v8";
const value: any = deserialize(Buffer.from([255, 15, 82, 34, 2, 97, 98, 1]));
console.log(value.source, value.flags, value.test("abc"));
