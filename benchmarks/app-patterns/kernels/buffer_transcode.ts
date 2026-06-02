// Pattern: Buffer ↔ string transcoding. Models reading binary
// network frames into UTF-8 strings, base64 / hex encoding, file
// IO when the on-disk encoding differs from the in-memory one.

const N = 5000;

// 4 KB sample body
let sample = "Hello, world! ";
for (let i = 0; i < 8; i++) sample += sample;

let total = 0;
for (let i = 0; i < N; i++) {
    // utf8 round-trip
    const buf = Buffer.from(sample, "utf8");
    const back = buf.toString("utf8");
    total += back.length;

    // base64 round-trip
    const b64 = buf.toString("base64");
    const buf2 = Buffer.from(b64, "base64");
    total += buf2.length;

    // hex round-trip
    const hex = buf.toString("hex");
    const buf3 = Buffer.from(hex, "hex");
    total += buf3.length;
}

console.log("checksum:", total);
