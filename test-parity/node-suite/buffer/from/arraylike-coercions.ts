import { Buffer } from "node:buffer";

for (const value of [{ length: "3", 0: 65, 1: 66, 2: 67 }, { length: -1, 0: 65 }, { length: 2.8, 0: 65, 1: 66, 2: 67 }]) {
  try { console.log("arraylike:", Buffer.from(value as any).toJSON().data.join(",")); } catch (err: any) { console.log("arraylike err:", err?.name, err?.code || "no-code"); }
}
