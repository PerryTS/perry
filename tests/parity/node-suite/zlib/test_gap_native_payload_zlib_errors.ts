import * as z from "node:zlib";

// Each codec has an independent worker. Their relative completion order is
// unspecified even in Node; preserve each codec's complete event sequence.
async function main() {
  for (const name of ["Gunzip", "Inflate", "InflateRaw", "Unzip", "BrotliDecompress", "ZstdDecompress"]) {
    await new Promise<void>((resolve) => {
      const codec: any = (z as any)["create" + name]();
      codec.on("error", (error: any) => {
        console.log(name, error.message, error.code, error.errno);
      });
      codec.on("close", () => {
        console.log(name, "close");
        resolve();
      });
      codec.write(Buffer.alloc(100, 255), (error: any) => {
        console.log(name, "callback", error?.code);
      });
      codec.end();
    });
  }
}

main();
