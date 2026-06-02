// process.abort is a callable function (not invoked here — it would kill the
// process).
console.log("is function:", typeof process.abort === "function");
