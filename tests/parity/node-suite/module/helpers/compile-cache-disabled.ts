import { flushCompileCache, getCompileCacheDir } from "node:module";

console.log("directory before enable:", String(getCompileCacheDir()));
console.log("flush before enable:", String(flushCompileCache()));
console.log("directory after flush:", String(getCompileCacheDir()));
