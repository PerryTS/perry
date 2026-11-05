import * as z from "node:zlib";
import * as st from "node:stream";

const names = ["Gzip", "Gunzip", "Deflate", "Inflate", "DeflateRaw", "InflateRaw",
  "Unzip", "BrotliCompress", "BrotliDecompress", "ZstdCompress", "ZstdDecompress"];
let lateCloses = 0;
for (const name of names) {
  const a: any = (z as any)["create" + name]({ chunkSize: 64 });
  const b: any = (z as any)["create" + name]({ chunkSize: 64 });
  console.log(name, "identity", a !== b, "family", a instanceof (z as any)[name],
    "stream", a instanceof st.Transform && a instanceof st.Duplex &&
      a instanceof st.Readable && a instanceof st.Writable);
  a._outBuffer.fill(0);
  console.log("own", JSON.stringify(Object.keys(a)));
  console.log("proto", JSON.stringify(Object.getOwnPropertyNames(Object.getPrototypeOf(a))));
  console.log("json-own", JSON.stringify(Object.keys(JSON.parse(JSON.stringify(a)))));
  a.close();
  console.log(name, "released", a._handle === null);
  a.on("close", function () {
    lateCloses++;
    console.log(name, "late-close", this === a);
  });
  try { a.reset(); console.log(name, "reset", "reopened"); }
  catch (error: any) { console.log(name, "reset", error.code); }
  b.close();
}
class Child extends z.Gzip {}
const child: any = new Child({ chunkSize: 64 });
console.log("subclass", child instanceof Child, child instanceof z.Gzip, child instanceof st.Transform);
console.log("subclass-own", JSON.stringify(Object.keys(child)));
console.log("subclass-proto", JSON.stringify(Object.getOwnPropertyNames(Object.getPrototypeOf(child))));
child.close();
setImmediate(() => console.log("late-closes", lateCloses));
