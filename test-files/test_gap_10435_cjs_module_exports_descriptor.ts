// #10435 — a CommonJS module may define its complete export through an
// accessor descriptor on the `module` object without spelling
// `module.exports` anywhere in executable code.
import logger from "./fixtures/issue_10435_descriptor_export.cjs";

console.log("info:", typeof (logger as any).info);
console.log("error:", typeof (logger as any).error);
console.log("own info:", Object.prototype.hasOwnProperty.call(logger, "info"));
(logger as any).child = () => logger;
console.log("child:", typeof (logger as any).child);
console.log("child identity:", (logger as any).child() === logger);
