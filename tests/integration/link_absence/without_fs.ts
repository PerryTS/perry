// Warning argument validation uses the shared Node error renderer.
try { process.emitWarning(1 as any); }
catch (error: any) { console.log(error.code, error.message); }
console.log("ordinary".replace("ordinary", "ok"));
