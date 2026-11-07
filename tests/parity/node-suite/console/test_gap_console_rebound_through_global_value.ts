// A computed store on globalThis rebinds console: console.log reads the new binding.
const k = ["con", "sole"].join("");
const g: any = globalThis;
g[k] = { log: (s: string) => { process.stdout.write("patched " + s + "\n"); } };
console.log("hello");
