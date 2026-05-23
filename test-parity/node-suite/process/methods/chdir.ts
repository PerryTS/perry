// process.chdir(dir) changes the cwd; chdir-ing to the current dir is a no-op.
const before = process.cwd();
process.chdir(before);
console.log("cwd unchanged:", process.cwd() === before);
console.log("chdir is function:", typeof process.chdir === "function");
