// The uncaught-exception capture API: hasUncaughtExceptionCaptureCallback()
// returns a boolean, setUncaughtExceptionCaptureCallback is a function.
console.log("has() is function:", typeof process.hasUncaughtExceptionCaptureCallback === "function");
console.log("has() returns boolean:", typeof process.hasUncaughtExceptionCaptureCallback() === "boolean");
console.log("set is function:", typeof process.setUncaughtExceptionCaptureCallback === "function");
