// process is an EventEmitter: on()/emit() of a custom event invokes the
// listener synchronously with the emitted args.
let received = "";
process.on("custom-evt", (x: string, y: number) => {
  received = `${x}:${y}`;
});
const had = process.emit("custom-evt" as any, "payload", 42);
console.log("emit returned:", had);
console.log("received:", received);
console.log("missing returned:", process.emit("missing-evt" as any));
