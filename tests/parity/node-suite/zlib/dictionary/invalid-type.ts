import * as zlib from "node:zlib";

for (const value of ["dictionary", 1, true, {}, [1, 2, 3]] as any[]) {
  try {
    const stream = zlib.createDeflate({ dictionary: value });
    console.log(typeof value, "ok");
    stream.destroy();
  } catch (error: any) {
    console.log(
      Array.isArray(value) ? "array" : typeof value,
      error.name,
      error.code,
    );
  }
}
