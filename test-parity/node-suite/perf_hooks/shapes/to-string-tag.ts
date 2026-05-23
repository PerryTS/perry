import { performance } from "node:perf_hooks";
// Performance has a Symbol.toStringTag of "Performance", so
// Object.prototype.toString returns "[object Performance]".
console.log("tag:", Object.prototype.toString.call(performance));
