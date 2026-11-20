import { deflateSync } from "node:zlib";
import * as zlib from "node:zlib";

// Type validation only: dictionary application is a separate codec contract.
const captured = zlib.deflateSync;
const routes: Array<[string, (options: any) => void]> = [
  ["direct", (options) => { deflateSync("payload", options); }],
  ["namespace", (options) => { zlib.deflateSync("payload", options); }],
  ["captured", (options) => { captured("payload", options); }],
  ["gzipSync", (options) => { zlib.gzipSync("payload", options); }],
  ["deflateRawSync", (options) => { zlib.deflateRawSync("payload", options); }],
];

for (const name of [
  "createDeflate", "createInflate", "createDeflateRaw", "createInflateRaw",
  "createGzip", "createGunzip", "createUnzip",
] as const) {
  routes.push([name, (options) => { zlib[name](options).destroy(); }]);
}

function result(label: string, call: () => void) {
  try {
    call();
    console.log(label, "accepted");
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}

const bytes = new Uint8Array([1, 2, 3, 4]);
const values: Array<[string, any]> = [
  ["undefined", undefined], ["null", null],
  ["buffer", Buffer.from([1, 2])], ["typedarray", bytes],
  ["typedarray-offset", new Uint16Array(bytes.buffer, 2, 1)],
  ["dataview", new DataView(bytes.buffer, 1, 2)],
  ["arraybuffer", bytes.buffer],
  ["string", "dictionary"], ["number", 1], ["boolean", true],
  ["object", {}], ["array", [1, 2, 3]],
];

for (const [name, call] of routes) {
  for (const [label, value] of values) {
    result(`${name} ${label}`, () => call({ dictionary: value }));
  }
  let reads = 0;
  const options = {
    get dictionary() {
      reads++;
      return bytes;
    },
  };
  result(`${name} getter`, () => call(options));
  result(`${name} throwing getter`, () => call({
    get dictionary() { throw new Error("dictionary getter"); },
  }));
  result(`${name} level before dictionary`, () => call({
    level: 100, dictionary: "invalid",
  }));
}

// These consumers share Perry's validator but are different codec families.
// Preserve their oracle behavior rather than applying DEFLATE rules to them.
for (const name of [
  "createBrotliCompress", "createBrotliDecompress",
  "createZstdCompress", "createZstdDecompress",
] as const) {
  result(`${name} dictionary control`, () => {
    zlib[name]({ dictionary: "invalid" } as any).destroy();
  });
}

// Callback compression also consumes options before scheduling work.
for (const name of ["gzip", "deflate", "deflateRaw"] as const) {
  for (const [label, value] of [["invalid", "dictionary"], ["valid", bytes]] as const) {
    const events: string[] = [];
    await new Promise<void>((resolve) => {
      try {
        zlib[name]("payload", { dictionary: value } as any, (error) => {
          events.push(error ? `callback ${error.name} ${error.code}` : "callback ok");
          resolve();
        });
        events.push("returned");
      } catch (error: any) {
        events.push(`throw ${error.name} ${error.code} ${error.message}`);
        resolve();
      }
    });
    console.log(`${name} ${label}`, events.join("|"));
  }
}

result("options before data", () => {
  zlib.deflateSync(null as any, { dictionary: "invalid" } as any);
});
