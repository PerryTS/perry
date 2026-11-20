// #4917 — zlib options are honored, not silently dropped: `level` must
// change compressor output on the stream factories and on deflateRawSync.
// Byte-for-byte parity vs `node --experimental-strip-types`: only
// relational facts are printed (absolute compressed sizes legitimately
// differ between flate2 and Node's zlib).
import * as zlib from "node:zlib";

const data = "abcdefghij-0123456789-".repeat(20000);

// ── one-shot: deflateRawSync(data, { level }) ──
const raw1 = zlib.deflateRawSync(data, { level: 1 });
const raw9 = zlib.deflateRawSync(data, { level: 9 });
console.log("deflateRawSync level1 >= level9:", raw1.length >= raw9.length);
console.log("deflateRawSync levels differ:", raw1.length !== raw9.length);
console.log(
  "deflateRawSync roundtrip l1:",
  zlib.inflateRawSync(raw1).toString() === data
);
console.log(
  "deflateRawSync roundtrip l9:",
  zlib.inflateRawSync(raw9).toString() === data
);

// out-of-range level still throws Node's RangeError first
try {
  zlib.deflateRawSync(data, { level: 99 });
  console.log("deflateRawSync level 99: no throw");
} catch (e) {
  console.log("deflateRawSync level 99 throws:", (e as Error).name);
}

// ── stream factories: createGzip({ level }) / createDeflate({ level }) ──
function collect(
  stream: any,
  input: string,
  done: (total: number) => void
): void {
  let total = 0;
  stream.on("data", (chunk: Buffer) => {
    total += chunk.length;
  });
  stream.on("end", () => done(total));
  stream.end(input);
}

collect(zlib.createGzip({ level: 1 }), data, (gz1) => {
  collect(zlib.createGzip({ level: 9 }), data, (gz9) => {
    console.log("createGzip level1 >= level9:", gz1 >= gz9);
    console.log("createGzip levels differ:", gz1 !== gz9);
    collect(zlib.createDeflate({ level: 1 }), data, (df1) => {
      collect(zlib.createDeflate({ level: 9 }), data, (df9) => {
        console.log("createDeflate level1 >= level9:", df1 >= df9);
        console.log("createDeflate levels differ:", df1 !== df9);
        // level-9 stream output must still gunzip back to the input
        const gzBuf = zlib.gzipSync(data, { level: 9 });
        console.log(
          "gzipSync l9 roundtrip:",
          zlib.gunzipSync(gzBuf).toString() === data
        );
        console.log("done");
      });
    });
  });
});
