// process.report.getReport().header names the C library on glibc Linux
// (glibcVersionRuntime, glibcVersionCompiler, right after nodejsVersion) and
// leaves both out elsewhere. Native-addon loaders pick their glibc or musl
// build from it: a missing key means musl.
const header = process.report.getReport().header as Record<string, unknown>;
const keys = Object.keys(header);
const runtime = header.glibcVersionRuntime;
const compiler = header.glibcVersionCompiler;
console.log(typeof runtime, typeof compiler);
if (typeof runtime === "string" && typeof compiler === "string") {
  console.log(/^\d+\.\d+/.test(runtime), /^\d+\.\d+/.test(compiler));
  const at = keys.indexOf("nodejsVersion");
  console.log(keys[at + 1], keys[at + 2], keys[at + 3]);
} else {
  console.log(keys.includes("glibcVersionRuntime"), keys.includes("glibcVersionCompiler"));
}
const isMusl = !runtime && process.platform === "linux";
console.log("musl", isMusl);
