// A store through a parameter replaces console.log: the direct call must read it.
function patch(o: any, k: string, v: any) { o[k] = v; }
patch(console, process.argv.length > 99 ? "x" : "log", (s: string) => { process.stdout.write("patched " + s + "\n"); });
console.log("hello");
console.log("text".replace("t", "T"));
