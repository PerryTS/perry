await import("./pid.ts");
gc();
console.log(globalThis.snap21pid===process.pid ? "fresh" : "STALE");
if(globalThis.snap21pid!==process.pid) process.exit(1);
