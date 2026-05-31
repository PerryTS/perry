// Parity coverage for #2612 (TextDecoder encodings/options) and
// #2959 (Blob/File constructor coercion). #2606 (FormData) is dropped
// from this PR (needs new object-model + method dispatch infra).

import { Blob, File } from "node:buffer";

// --- #2612: TextDecoder labels + options ----------------------------
console.log(new TextDecoder().encoding);
console.log(new TextDecoder("utf-8").encoding);
console.log(new TextDecoder("latin1").encoding);
console.log(new TextDecoder("windows-1252").encoding);
console.log(new TextDecoder("utf-16le").encoding);

const dec = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
console.log(dec.fatal, dec.ignoreBOM);
console.log(new TextDecoder().fatal, new TextDecoder().ignoreBOM);

// latin1 decode: 0xE9 -> U+00E9
console.log(new TextDecoder("latin1").decode(Uint8Array.from([0xe9])));
// utf-16le decode: "A" then U+20AC (euro)
console.log(new TextDecoder("utf-16le").decode(Uint8Array.from([0x41, 0x00, 0xac, 0x20])));
// utf-8 roundtrip
console.log(new TextDecoder().decode(new TextEncoder().encode("héllo")));

// --- #2959: Blob/File part coercion ---------------------------------
(async () => {
  const parts = [123, true, false, null, "x", ["a", "b"]];
  const b = new Blob(parts);
  console.log(JSON.stringify(await b.text()));
  console.log(b.size);

  const f = new File([123, "y"], 456, {
    type: "TEXT/PLAIN;Charset=UTF-8",
    lastModified: "42.9",
  });
  console.log(JSON.stringify(f.name));
  console.log(JSON.stringify(f.type));
  console.log(f.lastModified);
  console.log(JSON.stringify(await f.text()));

  const b2 = new Blob(["a", "b"]);
  console.log(JSON.stringify(await b2.text()), b2.size);
})();
