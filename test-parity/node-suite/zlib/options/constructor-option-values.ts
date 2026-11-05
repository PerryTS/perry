import { createGzip, Gzip } from "node:zlib";
import { Buffer } from "node:buffer";

// Constructor-only semantic observations, independent of DEFLATE backend bytes.
// Each case owns exactly one factory call;
// no one-shot consumer, private stream field, or auxiliary codec is consulted.
const labels = [
  "window-first-invalid-later-valid",
  "window-second-invalid",
  "window-second-throw",
  "dictionary-invalid-before-chunk-throw",
  "dictionary-throw-before-chunk-invalid",
  "level-first-zero-later-nine",
  "level-first-nine-later-zero",
  "level-third-throw",
];
const input = "constructor-phase-payload:".repeat(4);
let completed = 0;

function run(index: number) {
  if (index === labels.length) {
    console.log("complete", completed, "of", labels.length);
    return;
  }
  const label = labels[index];
  const sentinel = new Error(label + " sentinel");
  const reads: string[] = [];
  const options: any = {};
  let windowReads = 0;
  let levelReads = 0;
  function getter(key: string, value: () => any) {
    Object.defineProperty(options, key, {
      enumerable: true,
      get() {
        reads.push(key);
        return value();
      },
    });
  }
  if (index < 3) {
    getter("windowBits", () => {
      windowReads++;
      if (index === 0) return windowReads === 1 ? 8 : 15;
      if (windowReads === 1) return undefined;
      if (index === 2) throw sentinel;
      return 8;
    });
  } else if (index < 5) {
    getter("dictionary", () => {
      if (index === 4) throw sentinel;
      return "not a dictionary";
    });
    getter("chunkSize", () => {
      if (index === 3) throw sentinel;
      return 63;
    });
  } else {
    getter("level", () => {
      levelReads++;
      if (index === 7 && levelReads === 3) throw sentinel;
      if (index === 6) return levelReads === 1 ? 9 : 0;
      return levelReads === 1 ? 0 : 9;
    });
  }
  let stream: any;
  try {
    stream = createGzip(options);
  } catch (error: any) {
    console.log(label, "throw", error.name, error.code, error === sentinel);
    console.log(label, "reads", reads.join(","));
    completed++;
    run(index + 1);
    return;
  }
  console.log(label, "constructed", stream instanceof Gzip, stream.destroyed);
  console.log(label, "reads", reads.join(","));
  const chunks: any[] = [];
  stream.on("data", (chunk: any) => { chunks.push(chunk); });
  stream.on("error", (error: any) => {
    console.log(label, "error", error.name, error.code, error === sentinel);
    stream.destroy();
  });
  stream.on("end", () => {
    const output = Buffer.concat(chunks);
    console.log(label, "end", "XFL", output[8]);
    console.log(label, "bytesWritten", stream.bytesWritten);
    stream.destroy();
  });
  stream.on("close", () => {
    console.log(label, "closed", stream.destroyed);
    completed++;
    run(index + 1);
  });
  stream.end(input);
}
run(0);
