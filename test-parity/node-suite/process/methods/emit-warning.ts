// process.emitWarning is a callable function. Calling it would write to
// stderr (ordering of stdout vs stderr + Node's trace-warnings hint differ
// across runtimes), so the test only probes the surface.
console.log("is function:", typeof process.emitWarning === "function");
