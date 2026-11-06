import * as stream from "node:stream";
// node:stream re-exports isReadable / isErrored / isDisturbed as
// module-level helpers (aliases of the Readable static methods).
console.log("isReadable:", typeof (stream as any).isReadable === "function");
console.log("isErrored:", typeof (stream as any).isErrored === "function");
console.log("isDisturbed:", typeof (stream as any).isDisturbed === "function");
