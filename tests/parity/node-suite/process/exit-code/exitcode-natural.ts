// #6666: process.exitCode set at top level with no explicit process.exit().
// The natural-exit epilogue (event loop drained / main returned) must return
// it as the process status. Node exits 1 here; pre-fix Perry exited 0.
process.exitCode = 1;
console.log("done");
