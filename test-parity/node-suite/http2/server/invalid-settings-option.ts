import { createServer } from "node:http2";

for (const value of [1, true, "test", null]) {
  try {
    createServer({ settings: value as any });
  } catch (error: any) {
    console.log(typeof value, error.name, error.code);
  }
}
