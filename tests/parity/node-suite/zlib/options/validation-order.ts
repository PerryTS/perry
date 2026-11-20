import { createGzip } from "node:zlib";

const reads: string[] = [];
const options: any = {};
for (
  const key of [
    "flush",
    "finishFlush",
    "chunkSize",
    "maxOutputLength",
    "rejectGarbageAfterEnd",
    "windowBits",
    "level",
    "memLevel",
    "strategy",
    "dictionary",
  ]
) {
  Object.defineProperty(options, key, {
    enumerable: true,
    get() {
      reads.push(key);
      return undefined;
    },
  });
}

const stream = createGzip(options);
console.log("reads:", reads.join(","));
stream.destroy();
