// process is an EventEmitter: on()/emit() of a custom event invokes the
// listener synchronously with the emitted args.
let received = "";
process.on("custom-evt", (x: string) => {
  received = x;
});
const had = process.emit("custom-evt" as any, "payload");
console.log("emit returned:", had);
console.log("received:", received);
