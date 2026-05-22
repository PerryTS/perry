import { Buffer } from "node:buffer";

const valueOfObj = { valueOf() { return "6869"; } };
try { console.log("valueOf hex:", Buffer.from(valueOfObj as any, "hex").toString()); } catch (err: any) { console.log("valueOf err:", err?.name, err?.code || "no-code"); }

const primitiveObj = { [Symbol.toPrimitive]() { return "ok"; } };
try { console.log("toPrimitive:", Buffer.from(primitiveObj as any).toString()); } catch (err: any) { console.log("toPrimitive err:", err?.name, err?.code || "no-code"); }

const arrayLike: any = { 0: 65, 2: 67, length: 4 };
console.log("arrayLike sparse:", Buffer.from(arrayLike).toJSON().data.join(","));
