import * as z from "node:zlib";

for (const name of ["Gunzip", "Inflate", "InflateRaw", "Unzip", "BrotliDecompress", "ZstdDecompress"]) {
  const codec: any = (z as any)["create" + name]();
  codec.on("error", (error: any) => {
    console.log(name, error.message, error.code, error.errno);
  });
  codec.on("close", () => console.log(name, "close"));
  codec.write(Buffer.alloc(100, 255), (error: any) => {
    console.log(name, "callback", error?.code);
  });
  codec.end();
}
