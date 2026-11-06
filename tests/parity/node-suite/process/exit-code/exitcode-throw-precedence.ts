// #6666: an uncaught throw exits 1 regardless of a set process.exitCode —
// the uncaught exception takes precedence over the natural-exit code (node
// rc=1). This path terminates before the natural-exit epilogue runs.
process.exitCode = 3;
console.log("before throw");
throw new Error("boom");
