// WASI has no process ids and std::process::id() panics there. The Perry
// diagnostic heap snapshot puts the process id in its header; the module must
// run to its end and report 0, like process.pid.
const retained: any[] = [];
for (let i = 0; i < 100; i++) retained.push({ text: `value-${i}`, values: [i, i + 1] });

const snapshot = Bun.generateHeapSnapshot("v8", "string") as string;
console.log("snapshot pid 0", snapshot.includes('"pid":0'));
console.log("retained", retained.length);
