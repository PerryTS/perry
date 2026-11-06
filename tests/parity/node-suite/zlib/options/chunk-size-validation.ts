import { createDeflate } from "node:zlib";

for (const value of ["64", 0, 63, 64, NaN, Infinity] as any[]) {
  try {
    const stream = createDeflate({ chunkSize: value });
    console.log(String(value), "ok");
    stream.destroy();
  } catch (error: any) {
    console.log(String(value), error.name, error.code);
  }
}
