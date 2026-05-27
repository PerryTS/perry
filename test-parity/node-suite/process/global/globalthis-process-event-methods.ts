const gproc: any = (globalThis as any).process;

console.log("global process typeof:", typeof gproc);
console.log("global process on typeof:", typeof gproc?.on);
console.log("global process emit typeof:", typeof gproc?.emit);
console.log("global process once typeof:", typeof gproc?.once);
console.log("global process off typeof:", typeof gproc?.off);

let seen = "";
gproc.on("global-process-event", (value: string) => {
  seen = value;
});
console.log("global process emit returned:", gproc.emit("global-process-event", "payload"));
console.log("global process listener seen:", seen);
