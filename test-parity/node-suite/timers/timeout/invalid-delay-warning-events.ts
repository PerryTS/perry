const events: string[] = [];

const registered = process.on("warning", (warning: any) => {
  const firstLine = String(warning.message).split("\n")[0];
  events.push(`${warning.name}:${firstLine}`);
}) === process;

console.log("registered:", registered);
setTimeout(() => {}, -5).unref();
setTimeout(() => {}, NaN as any).unref();
setTimeout(() => {}, Infinity).unref();
const interval = setInterval(() => {}, Infinity);
clearInterval(interval);

console.log("scheduled:", events.join("|"));
setImmediate(() => {
  console.log("warnings:", events.join("|"));
});
