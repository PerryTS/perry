// process.execArgv — runtime CLI flags. Perry binaries are AOT so the
// list is always empty, and the strip-types invocation Node uses here
// would otherwise put `--experimental-strip-types` in it. The shape
// assertions (Array.isArray + every-string) are what callers actually
// branch on. Regression cover for #1349.
const argv = process.execArgv;
console.log("is array:", Array.isArray(argv));
console.log("all strings:", argv.every((v) => typeof v === "string"));
console.log("length type:", typeof argv.length);
